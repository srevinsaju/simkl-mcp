const TOKEN_ENDPOINT = 'https://api.simkl.com/oauth2/token';
const USER_AGENT = 'simkl-mcp/1.0.0';
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
  const random = crypto.getRandomValues(new Uint8Array(32));
  const codeVerifier = base64UrlEncode(random);
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(codeVerifier));
  return { codeVerifier, codeChallenge: base64UrlEncode(new Uint8Array(digest)) };
}

export function exchangeAuthorizationCode(
  input: { code: string; codeVerifier: string },
  env: SimklOAuthEnv
): Promise<SimklTokenSet> {
  return requestToken({
    code: input.code,
    code_verifier: input.codeVerifier,
    grant_type: 'authorization_code',
    redirect_uri: env.OAUTH_REDIRECT_URI,
  }, env);
}

export function refreshSimklToken(refreshToken: string, env: SimklOAuthEnv): Promise<SimklTokenSet> {
  return requestToken({
    grant_type: 'refresh_token',
    refresh_token: refreshToken,
  }, env, refreshToken);
}

async function requestToken(
  fields: Record<string, string>,
  env: SimklOAuthEnv,
  priorRefreshToken?: string
): Promise<SimklTokenSet> {
  const response = await fetch(TOKEN_ENDPOINT, {
    method: 'POST',
    headers: {
      Authorization: `Basic ${btoa(`${env.SIMKL_CLIENT_ID}:${env.SIMKL_CLIENT_SECRET}`)}`,
      'Content-Type': 'application/x-www-form-urlencoded',
      'User-Agent': USER_AGENT,
    },
    body: new URLSearchParams(fields).toString(),
  });

  if (!response.ok) {
    throw new Error(`Simkl token request failed with status ${response.status}`);
  }

  let data: unknown;
  try {
    data = await response.json();
  } catch {
    throw new Error('Simkl token response was not valid JSON');
  }

  if (!isTokenResponse(data)) {
    throw new Error('Simkl token response was missing required token fields');
  }

  const now = Date.now();
  const expiresIn = Number.isFinite(data.expires_in) && data.expires_in! > 0
    ? data.expires_in!
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

function isTokenResponse(value: unknown): value is {
  access_token: string;
  refresh_token?: string;
  expires_in?: number;
  scope?: string;
} {
  if (typeof value !== 'object' || value === null) return false;
  const record = value as Record<string, unknown>;
  return typeof record.access_token === 'string' && record.access_token.length > 0;
}

function base64UrlEncode(bytes: Uint8Array): string {
  let binary = '';
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/g, '');
}
