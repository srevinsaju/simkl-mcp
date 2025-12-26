interface Env {
  SIMKL_CLIENT_ID: string;
  SIMKL_CLIENT_SECRET: string;
  OAUTH_REDIRECT_URI: string;
  SIMKL_API_BASE_URL?: string;
  OAUTH_PROVIDER: any;
  OAUTH_KV: KVNamespace;
}

interface SimklAuthProps {
  simklToken: string;
  simklUserId?: string;
}

export async function exchangeSimklToken(
  authorizationCode: string,
  env: Env
): Promise<SimklAuthProps> {
  const tokenResponse = await fetch('https://api.simkl.com/oauth/token', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      code: authorizationCode,
      client_id: env.SIMKL_CLIENT_ID,
      client_secret: env.SIMKL_CLIENT_SECRET,
      redirect_uri: env.OAUTH_REDIRECT_URI,
      grant_type: 'authorization_code',
    }),
  });

  if (!tokenResponse.ok) {
    const error = await tokenResponse.text();
    throw new Error(`simkl token exchange failed: ${error}`);
  }

  const tokenData = await tokenResponse.json() as { access_token: string };

  return {
    simklToken: tokenData.access_token,
  };
}

function normalizeScopes(scope: unknown): string[] {
  if (Array.isArray(scope)) return scope.map(String);
  if (typeof scope === 'string') return scope.split(/\s+/).filter(Boolean);
  return [];
}

async function fetchSimklUserId(simklToken: string, env: Env): Promise<string | undefined> {
  const baseUrl = env.SIMKL_API_BASE_URL || 'https://api.simkl.com';
  const settingsUrl = new URL('/users/settings', baseUrl);
  settingsUrl.searchParams.set('client_id', env.SIMKL_CLIENT_ID);

  const response = await fetch(settingsUrl.toString(), {
    method: 'POST',
    headers: {
      'Authorization': `Bearer ${simklToken}`,
      'Content-Type': 'application/json',
    },
  });

  if (!response.ok) {
    return undefined;
  }

  try {
    const settings = await response.json() as any;
    const id = settings?.account?.id;
    return id ? `simkl_user_${id}` : undefined;
  } catch {
    return undefined;
  }
}

export default {
  async fetch(request: Request, env: any): Promise<Response> {
    const url = new URL(request.url);

    if (url.pathname === '/auth/simkl') {
      const oauthRequest = await env.OAUTH_PROVIDER.parseAuthRequest(request);
      const state = crypto.randomUUID();

      await env.OAUTH_KV.put(
        `oauth_state:${state}`,
        JSON.stringify(oauthRequest),
        { expirationTtl: 600 }
      );

      const simklAuthUrl = new URL('https://simkl.com/oauth/authorize');
      simklAuthUrl.searchParams.set('response_type', 'code');
      simklAuthUrl.searchParams.set('client_id', env.SIMKL_CLIENT_ID);
      simklAuthUrl.searchParams.set('redirect_uri', env.OAUTH_REDIRECT_URI);
      simklAuthUrl.searchParams.set('state', state);

      return Response.redirect(simklAuthUrl.toString(), 302);
    }

    if (url.pathname === '/oauth/callback') {
      const code = url.searchParams.get('code');
      const state = url.searchParams.get('state');

      if (!code || !state) {
        return new Response('missing authorization code or state', { status: 400 });
      }

      const storedRequest = await env.OAUTH_KV.get(`oauth_state:${state}`);
      if (!storedRequest) {
        return new Response('invalid or expired state', { status: 400 });
      }

      const oauthRequest = JSON.parse(storedRequest);
      const simklAuthProps = await exchangeSimklToken(code, env);
      const requestedScopes = normalizeScopes(oauthRequest.scope);
      const allowedScopes = new Set(['public']);
      const filteredScopes = requestedScopes.filter(scope => allowedScopes.has(scope));

      if (requestedScopes.length && filteredScopes.length !== requestedScopes.length) {
        return new Response('invalid scope requested', { status: 400 });
      }

      const grantedScopes = filteredScopes.length ? filteredScopes : ['public'];
      const resolvedUserId = await fetchSimklUserId(simklAuthProps.simklToken, env) || `simkl_user_${crypto.randomUUID()}`;

      const { redirectTo } = await env.OAUTH_PROVIDER.completeAuthorization({
        request: oauthRequest,
        userId: resolvedUserId,
        scope: grantedScopes,
        props: simklAuthProps,
      });

      await env.OAUTH_KV.delete(`oauth_state:${state}`);

      return Response.redirect(redirectTo, 302);
    }

    if (url.pathname === '/health') {
      return new Response(
        JSON.stringify({ status: 'ok', service: 'simkl-mcp' }),
        { headers: { 'content-type': 'application/json' } }
      );
    }

    return new Response('not found', { status: 404 });
  },
};
