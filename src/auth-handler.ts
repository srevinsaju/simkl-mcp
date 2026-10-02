import {
  createPkcePair,
  exchangeAuthorizationCode,
  type SimklAuthProps,
  type SimklOAuthEnv,
} from './auth/simkl-oauth';
import { APP_NAME, APP_VERSION, USER_AGENT } from './app-info';

interface Env extends WorkerEnv, SimklOAuthEnv {
  OAUTH_PROVIDER: {
    parseAuthRequest(request: Request): Promise<unknown>;
    completeAuthorization(input: { request: unknown; userId: string; scope: string[]; props: SimklAuthProps }): Promise<{ redirectTo: string }>;
  };
}

interface PendingOAuthState {
  oauthRequest: unknown;
  codeVerifier: string;
  createdAt: number;
}

const SIMKL_ISSUER = 'https://simkl.com';
const SIMKL_AUTHORIZATION_ENDPOINT = 'https://simkl.com/oauth2/authorize';
const SIMKL_SCOPE = 'media:read media:write';

function normalizeScopes(scope: unknown): string[] {
  if (Array.isArray(scope)) return scope.flatMap(value => String(value).split(/[\s,]+/).filter(Boolean));
  if (typeof scope === 'string') return scope.split(/[\s,]+/).filter(Boolean);
  return [];
}

function isPendingOAuthState(value: unknown): value is PendingOAuthState {
  if (typeof value !== 'object' || value === null) return false;
  const pending = value as Record<string, unknown>;
  return typeof pending.codeVerifier === 'string'
    && pending.codeVerifier.length >= 43
    && pending.codeVerifier.length <= 128
    && typeof pending.createdAt === 'number'
    && 'oauthRequest' in pending;
}

async function fetchSimklUserId(simklToken: string, env: Env): Promise<string | undefined> {
  const baseUrl = env.SIMKL_API_BASE_URL || 'https://api.simkl.com';
  const settingsUrl = new URL('/users/settings', baseUrl);
  settingsUrl.searchParams.set('client_id', env.SIMKL_CLIENT_ID);
  settingsUrl.searchParams.set('app-name', APP_NAME);
  settingsUrl.searchParams.set('app-version', APP_VERSION);
  try {
    const response = await fetch(settingsUrl.toString(), {
      method: 'GET',
      headers: { Authorization: `Bearer ${simklToken}`, 'User-Agent': USER_AGENT },
    });
    if (!response.ok) return undefined;
    const settings = await response.json() as { account?: { id?: string | number } };
    const id = settings?.account?.id;
    return id ? `simkl_user_${id}` : undefined;
  } catch {
    return undefined;
  }
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    if (url.pathname === '/auth/simkl') {
      try {
        const oauthRequest = await env.OAUTH_PROVIDER.parseAuthRequest(request);
        const state = crypto.randomUUID();
        const { codeVerifier, codeChallenge } = await createPkcePair();
        const pending: PendingOAuthState = { oauthRequest, codeVerifier, createdAt: Date.now() };
        await env.OAUTH_KV.put(`oauth_state:${state}`, JSON.stringify(pending), { expirationTtl: 600 });
        const authorizationUrl = new URL(SIMKL_AUTHORIZATION_ENDPOINT);
        authorizationUrl.searchParams.set('response_type', 'code');
        authorizationUrl.searchParams.set('client_id', env.SIMKL_CLIENT_ID);
        authorizationUrl.searchParams.set('redirect_uri', env.OAUTH_REDIRECT_URI);
        authorizationUrl.searchParams.set('state', state);
        authorizationUrl.searchParams.set('scope', SIMKL_SCOPE);
        authorizationUrl.searchParams.set('code_challenge', codeChallenge);
        authorizationUrl.searchParams.set('code_challenge_method', 'S256');
        return Response.redirect(authorizationUrl.toString(), 302);
      } catch {
        return new Response('Unable to start authorization', { status: 500 });
      }
    }

    if (url.pathname === '/oauth/callback') {
      const code = url.searchParams.get('code');
      const state = url.searchParams.get('state');
      if (url.searchParams.has('error') || !code || !state || url.searchParams.get('iss') !== SIMKL_ISSUER) {
        return new Response('Invalid authorization response', { status: 400 });
      }
      const key = `oauth_state:${state}`;
      const stored = await env.OAUTH_KV.get(key);
      if (!stored) return new Response('Invalid or expired authorization state', { status: 400 });

      let pending: unknown;
      try {
        pending = JSON.parse(stored);
      } catch {
        await env.OAUTH_KV.delete(key);
        return new Response('Invalid or expired authorization state', { status: 400 });
      }
      if (!isPendingOAuthState(pending)) {
        await env.OAUTH_KV.delete(key);
        return new Response('Invalid or expired authorization state', { status: 400 });
      }

      // Simkl consumes authorization codes when an exchange is attempted. Delete
      // state first so a failed exchange cannot make a replay retry that code.
      await env.OAUTH_KV.delete(key);
      try {
        const tokenSet = await exchangeAuthorizationCode({
          callbackUrl: url,
          expectedState: state,
          codeVerifier: pending.codeVerifier,
        }, env);
        const props: SimklAuthProps = {
          simklToken: tokenSet.accessToken,
          simklRefreshToken: tokenSet.refreshToken,
          simklExpiresAt: tokenSet.expiresAt,
          simklRefreshExpiresAt: tokenSet.refreshExpiresAt,
          ...(tokenSet.scope ? { simklScope: tokenSet.scope } : {}),
        };
        const requestedScopes = normalizeScopes((pending.oauthRequest as { scope?: unknown } | null)?.scope);
        const grantedScopes = requestedScopes.length ? requestedScopes : ['public'];
        const userId = await fetchSimklUserId(tokenSet.accessToken, env)
          || `simkl_user_${crypto.randomUUID()}`;
        const { redirectTo } = await env.OAUTH_PROVIDER.completeAuthorization({
          request: pending.oauthRequest, userId, scope: grantedScopes, props,
        });
        return Response.redirect(redirectTo, 302);
      } catch {
        return new Response('Authorization failed. Please start again.', { status: 502 });
      } finally {
        await env.OAUTH_KV.delete(key);
      }
    }

    if (url.pathname === '/health') {
      return new Response(JSON.stringify({ status: 'ok', service: APP_NAME }), {
        headers: { 'content-type': 'application/json' },
      });
    }
    return new Response('not found', { status: 404 });
  },
};
