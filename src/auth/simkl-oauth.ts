import * as oauth from 'oauth4webapi';
import { USER_AGENT } from '../app-info';

const SIMKL_ISSUER = new URL('https://simkl.com');
const DEFAULT_ACCESS_TOKEN_TTL_SECONDS = 7 * 24 * 60 * 60;
const REFRESH_TOKEN_TTL_MS = 180 * 24 * 60 * 60 * 1000;

export interface SimklOAuthEnv {
  SIMKL_CLIENT_ID: string;
  SIMKL_CLIENT_SECRET: string;
  OAUTH_REDIRECT_URI: string;
}

export interface SimklTokenSet {
  accessToken: string;
  refreshToken: string;
  expiresAt: number;
  refreshExpiresAt: number;
  scope?: string;
}

export interface SimklAuthProps extends Record<string, unknown> {
  simklToken: string;
  simklRefreshToken: string;
  simklExpiresAt: number;
  simklRefreshExpiresAt: number;
  simklScope?: string;
}

export async function createPkcePair(): Promise<{ codeVerifier: string; codeChallenge: string }> {
  const codeVerifier = oauth.generateRandomCodeVerifier();
  const codeChallenge = await oauth.calculatePKCECodeChallenge(codeVerifier);
  return { codeVerifier, codeChallenge };
}

export async function exchangeAuthorizationCode(
  input: { code: string; codeVerifier: string },
  env: SimklOAuthEnv
): Promise<SimklTokenSet> {
  const { authorizationServer, client, clientAuth } = await getOAuthClient(env);
  const callbackParameters = new URLSearchParams({ code: input.code });

  let tokenResponse: oauth.TokenEndpointResponse;
  try {
    const response = await oauth.authorizationCodeGrantRequest(
      authorizationServer,
      client,
      clientAuth,
      callbackParameters,
      env.OAUTH_REDIRECT_URI,
      input.codeVerifier,
      { headers: { 'User-Agent': USER_AGENT } }
    );
    tokenResponse = await oauth.processAuthorizationCodeResponse(authorizationServer, client, response);
  } catch (error) {
    throwTokenError(error);
  }
  return toSimklTokenSet(tokenResponse);
}

export async function refreshSimklToken(refreshToken: string, env: SimklOAuthEnv): Promise<SimklTokenSet> {
  const { authorizationServer, client, clientAuth } = await getOAuthClient(env);

  let tokenResponse: oauth.TokenEndpointResponse;
  try {
    const response = await oauth.refreshTokenGrantRequest(
      authorizationServer,
      client,
      clientAuth,
      refreshToken,
      { headers: { 'User-Agent': USER_AGENT } }
    );
    tokenResponse = await oauth.processRefreshTokenResponse(authorizationServer, client, response);
  } catch (error) {
    throwTokenError(error);
  }
  return toSimklTokenSet(tokenResponse, refreshToken);
}

async function getOAuthClient(env: SimklOAuthEnv) {
  try {
    const response = await oauth.discoveryRequest(SIMKL_ISSUER, {
      algorithm: 'oauth2',
      headers: { 'User-Agent': USER_AGENT },
    });
    const authorizationServer = await oauth.processDiscoveryResponse(SIMKL_ISSUER, response);
    if (!authorizationServer.token_endpoint) {
      throw new Error('Missing token endpoint');
    }

    return {
      authorizationServer,
      client: { client_id: env.SIMKL_CLIENT_ID },
      clientAuth: oauth.ClientSecretBasic(env.SIMKL_CLIENT_SECRET),
    };
  } catch {
    // Discovery errors can contain response data. Keep credentials and protocol details out of errors.
    throw new Error('Simkl authorization server discovery failed');
  }
}

function toSimklTokenSet(
  data: oauth.TokenEndpointResponse,
  priorRefreshToken?: string
): SimklTokenSet {
  if (!data.access_token || data.token_type.toLowerCase() !== 'bearer') {
    throw new Error('Simkl token response was missing required token fields');
  }

  const now = Date.now();
  const expiresIn = typeof data.expires_in === 'number' && Number.isFinite(data.expires_in) && data.expires_in > 0
    ? data.expires_in
    : DEFAULT_ACCESS_TOKEN_TTL_SECONDS;
  const returnedRefreshToken = typeof data.refresh_token === 'string' && data.refresh_token.length > 0
    ? data.refresh_token
    : priorRefreshToken;
  if (!returnedRefreshToken) {
    throw new Error('Simkl token response was missing a refresh token');
  }

  return {
    accessToken: data.access_token,
    refreshToken: returnedRefreshToken,
    expiresAt: now + expiresIn * 1000,
    refreshExpiresAt: now + REFRESH_TOKEN_TTL_MS,
    ...(typeof data.scope === 'string' ? { scope: data.scope } : {}),
  };
}

function throwTokenError(error: unknown): never {
  const candidate = error as { status?: unknown; code?: unknown } | null;
  if (candidate && Number.isInteger(candidate.status) && (candidate.status as number) >= 100) {
    throw new Error(`Simkl token request failed with status ${candidate.status}`);
  }
  if (candidate?.code === 'OAUTH_RESPONSE_IS_NOT_JSON') {
    throw new Error('Simkl token response was not valid JSON');
  }
  throw new Error('Simkl token response was missing required token fields');
}
