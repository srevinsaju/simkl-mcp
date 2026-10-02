import { OAuthProvider } from '@cloudflare/workers-oauth-provider';
import type { TokenExchangeCallbackOptions, TokenExchangeCallbackResult } from '@cloudflare/workers-oauth-provider';
import { SimklMCP } from './mcp-agent.js';
import authHandler from './auth-handler.js';
import { refreshSimklToken, type SimklAuthProps, type SimklOAuthEnv, type SimklTokenSet } from './auth/simkl-oauth.js';

export { SimklMCP };

type Env = WorkerEnv & SimklOAuthEnv;

const REFRESH_SAFETY_WINDOW_MS = 60_000;
const inFlightRefreshes = new Map<string, Promise<SimklTokenSet>>();

function secondsUntilExpiry(expiresAt: number): number {
  return Math.max(1, Math.floor((expiresAt - Date.now()) / 1000));
}

function propsToTokenSet(props: SimklAuthProps): SimklTokenSet {
  return {
    accessToken: props.simklToken,
    refreshToken: props.simklRefreshToken,
    expiresAt: props.simklExpiresAt,
    refreshExpiresAt: props.simklRefreshExpiresAt,
    ...(props.simklScope ? { scope: props.simklScope } : {}),
  };
}

function tokenSetToProps(tokenSet: SimklTokenSet, oldProps: SimklAuthProps): SimklAuthProps {
  return {
    ...oldProps,
    simklToken: tokenSet.accessToken,
    simklRefreshToken: tokenSet.refreshToken || oldProps.simklRefreshToken,
    simklExpiresAt: tokenSet.expiresAt,
    simklRefreshExpiresAt: tokenSet.refreshExpiresAt,
    ...(tokenSet.scope ? { simklScope: tokenSet.scope } : {}),
  };
}

export function createSimklTokenExchangeCallback(env: SimklOAuthEnv) {
  return async (options: TokenExchangeCallbackOptions): Promise<TokenExchangeCallbackResult> => {
    const currentProps = options.props as SimklAuthProps;
    if (options.grantType === 'authorization_code') {
      return { newProps: currentProps, accessTokenTTL: secondsUntilExpiry(currentProps.simklExpiresAt) };
    }

    if (typeof currentProps?.simklRefreshToken !== 'string' || !currentProps.simklRefreshToken) {
      throw new Error('Unable to refresh Simkl token');
    }
    const now = Date.now();
    if (typeof currentProps.simklExpiresAt === 'number'
      && currentProps.simklExpiresAt > now + REFRESH_SAFETY_WINDOW_MS) {
      return { newProps: currentProps, accessTokenTTL: secondsUntilExpiry(currentProps.simklExpiresAt) };
    }

    let refreshPromise = inFlightRefreshes.get(currentProps.simklRefreshToken);
    if (!refreshPromise) {
      const currentTokenSet = propsToTokenSet(currentProps);
      refreshPromise = refreshSimklToken(currentTokenSet.refreshToken, env);
      inFlightRefreshes.set(currentProps.simklRefreshToken, refreshPromise);
      void refreshPromise.finally(() => {
        if (inFlightRefreshes.get(currentProps.simklRefreshToken) === refreshPromise) {
          inFlightRefreshes.delete(currentProps.simklRefreshToken);
        }
      }).catch(() => undefined);
    }

    try {
      const refreshedTokenSet = await refreshPromise;
      const updatedProps = tokenSetToProps(refreshedTokenSet, currentProps);
      return { newProps: updatedProps, accessTokenTTL: secondsUntilExpiry(refreshedTokenSet.expiresAt) };
    } catch {
      // Never pass upstream errors through the provider response; they can contain credentials.
      throw new Error('Unable to refresh Simkl token');
    }
  };
}

export function createOAuthProvider(env: Env): OAuthProvider {
  return new OAuthProvider({
    apiHandlers: {
      '/sse': SimklMCP.serveSSE('/sse'),
      '/mcp': SimklMCP.serve('/mcp'),
    },

    defaultHandler: authHandler as unknown as ExportedHandler,

    authorizeEndpoint: '/auth/simkl',
    tokenEndpoint: '/token',
    clientRegistrationEndpoint: '/register',
    tokenExchangeCallback: createSimklTokenExchangeCallback(env),
  });
}

export default {
  fetch(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
    return createOAuthProvider(env).fetch(request, env, ctx);
  },
};
