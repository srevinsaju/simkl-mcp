import { afterEach, describe, expect, mock, test } from 'bun:test';
import { createPkcePair, exchangeAuthorizationCode, refreshSimklToken } from '../src/auth/simkl-oauth';
import { USER_AGENT } from '../src/app-info';

const env = {
  SIMKL_CLIENT_ID: 'client-id',
  SIMKL_CLIENT_SECRET: 'client-secret',
  OAUTH_REDIRECT_URI: 'https://example.com/oauth/callback',
};

const originalFetch = globalThis.fetch;
afterEach(() => {
  globalThis.fetch = originalFetch;
});

function discoveryResponse(): Response {
  return Response.json({
    issuer: 'https://simkl.com',
    authorization_endpoint: 'https://simkl.com/oauth2/authorize',
    token_endpoint: 'https://api.simkl.com/oauth2/token',
    response_types_supported: ['code'],
    token_endpoint_auth_methods_supported: ['client_secret_basic'],
    code_challenge_methods_supported: ['S256'],
  });
}

function encodeBase64Url(bytes: Uint8Array): string {
  return btoa(String.fromCharCode(...bytes)).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/g, '');
}

describe('Simkl OAuth utilities', () => {
  test('creates a URL-safe PKCE verifier and matching S256 challenge', async () => {
    const { codeVerifier, codeChallenge } = await createPkcePair();
    expect(codeVerifier).toMatch(/^[A-Za-z0-9_-]{43,128}$/);
    const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(codeVerifier));
    expect(codeChallenge).toBe(encodeBase64Url(new Uint8Array(digest)));
  });

  test('exchanges an authorization code with form fields and Basic client authentication', async () => {
    const requests: Array<{ url: string; init: RequestInit }> = [];
    globalThis.fetch = mock(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = new URL(String(input));
      if (url.pathname === '/.well-known/oauth-authorization-server') return discoveryResponse();
      requests.push({ url: String(input), init: init! });
      return Response.json({ access_token: 'access', token_type: 'Bearer', refresh_token: 'refresh', expires_in: 604800 });
    }) as typeof fetch;

    const expectedState = 'expected-state';
    const callbackUrl = new URL(env.OAUTH_REDIRECT_URI);
    callbackUrl.searchParams.set('code', 'one-time-code');
    callbackUrl.searchParams.set('state', expectedState);
    callbackUrl.searchParams.set('iss', 'https://simkl.com');

    const token = await exchangeAuthorizationCode({ callbackUrl, expectedState, codeVerifier: 'verifier' }, env);
    const captured = requests[0];
    expect(captured.url).toBe('https://api.simkl.com/oauth2/token');
    expect(captured.init.method).toBe('POST');
    const headers = new Headers(captured.init.headers);
    expect(headers.get('Content-Type')).toBe('application/x-www-form-urlencoded');
    expect(headers.get('User-Agent')).toBe(USER_AGENT);
    expect(headers.get('Authorization')).toBe(`Basic ${btoa('client-id:client-secret')}`);
    const body = new URLSearchParams(String(captured.init.body));
    expect(Object.fromEntries(body)).toEqual({
      code: 'one-time-code',
      code_verifier: 'verifier',
      grant_type: 'authorization_code',
      redirect_uri: env.OAUTH_REDIRECT_URI,
    });
    expect(token.accessToken).toBe('access');
    expect(token.refreshToken).toBe('refresh');
    expect(token.expiresAt).toBeGreaterThan(Date.now());
    expect(token.refreshExpiresAt - Date.now()).toBeGreaterThan(179 * 24 * 60 * 60 * 1000);
  });

  test('rejects an unexpected callback state before token exchange', async () => {
    let tokenRequests = 0;
    globalThis.fetch = mock(async (input: RequestInfo | URL) => {
      const url = new URL(String(input));
      if (url.pathname === '/.well-known/oauth-authorization-server') return discoveryResponse();
      tokenRequests++;
      return Response.json({ access_token: 'access', token_type: 'Bearer', refresh_token: 'refresh' });
    }) as typeof fetch;

    const callbackUrl = new URL(env.OAUTH_REDIRECT_URI);
    callbackUrl.searchParams.set('code', 'one-time-code');
    callbackUrl.searchParams.set('state', 'unexpected-state');
    callbackUrl.searchParams.set('iss', 'https://simkl.com');

    await expect(exchangeAuthorizationCode({
      callbackUrl,
      expectedState: 'expected-state',
      codeVerifier: 'verifier',
    }, env)).rejects.toThrow();
    expect(tokenRequests).toBe(0);
  });

  test('refreshes with a form grant and preserves the prior refresh token if omitted', async () => {
    let captured: RequestInit | undefined;
    globalThis.fetch = mock(async (input: RequestInfo | URL, init?: RequestInit) => {
      if (new URL(String(input)).pathname === '/.well-known/oauth-authorization-server') return discoveryResponse();
      captured = init;
      return Response.json({ access_token: 'new-access', token_type: 'Bearer', expires_in: 3600 });
    }) as typeof fetch;

    const token = await refreshSimklToken('long-lived-refresh', env);
    const body = new URLSearchParams(String(captured?.body));
    expect(Object.fromEntries(body)).toEqual({ grant_type: 'refresh_token', refresh_token: 'long-lived-refresh' });
    expect(token.accessToken).toBe('new-access');
    expect(token.refreshToken).toBe('long-lived-refresh');
    expect(token.expiresAt).toBeGreaterThan(Date.now());
    expect(token.refreshExpiresAt).toBeGreaterThan(Date.now());
  });

  test('rejects non-OK token responses', async () => {
    globalThis.fetch = mock(async (input: RequestInfo | URL) => {
      if (new URL(String(input)).pathname === '/.well-known/oauth-authorization-server') return discoveryResponse();
      return new Response('rejected', { status: 401 });
    }) as typeof fetch;
    await expect(refreshSimklToken('refresh', env)).rejects.toThrow(/401/);
  });
});
