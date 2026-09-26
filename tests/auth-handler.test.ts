import { afterEach, describe, expect, mock, test } from 'bun:test';
import handler from '../src/auth-handler';

const originalFetch = globalThis.fetch;
const originalRandomUUID = crypto.randomUUID;
afterEach(() => {
  globalThis.fetch = originalFetch;
  crypto.randomUUID = originalRandomUUID;
});

function setup() {
  const values = new Map<string, string>();
  const puts: Array<{ key: string; value: string; expirationTtl?: number }> = [];
  const deletes: string[] = [];
  const completeAuthorization = mock(async (_input: unknown) => ({ redirectTo: 'https://client.example/done' }));
  const env = {
    SIMKL_CLIENT_ID: 'client-id',
    SIMKL_CLIENT_SECRET: 'do-not-leak',
    OAUTH_REDIRECT_URI: 'https://service.example/oauth/callback',
    OAUTH_PROVIDER: {
      parseAuthRequest: mock(async () => ({ clientId: 'client', redirectUri: 'https://client.example/cb', scope: 'read write' })),
      completeAuthorization,
    },
    OAUTH_KV: {
      put: mock(async (key: string, value: string, options?: { expirationTtl?: number }) => {
        values.set(key, value); puts.push({ key, value, expirationTtl: options?.expirationTtl });
      }),
      get: mock(async (key: string) => values.get(key) ?? null),
      delete: mock(async (key: string) => { values.delete(key); deletes.push(key); }),
    },
  };
  return { env, values, puts, deletes, completeAuthorization };
}

function successfulFetch(onExchange?: () => void) {
  globalThis.fetch = mock(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = new URL(String(input));
    if (url.pathname === '/oauth/token') {
      onExchange?.();
      return Response.json({ access_token: 'access', refresh_token: 'refresh', expires_in: 3600 });
    }
    if (url.pathname === '/users/settings') return Response.json({ account: { id: 42 } });
    throw new Error(`unexpected URL ${url}`);
  }) as typeof fetch;
}

describe('AUTH V2 handler', () => {
  test('creates a PKCE authorization redirect and stores verifier plus OAuth request for 600 seconds', async () => {
    const { env, puts, values } = setup();
    crypto.randomUUID = (() => 'state-123') as typeof crypto.randomUUID;
    const response = await handler.fetch(new Request('https://service.example/auth/simkl?client_id=x'), env);
    expect(response.status).toBe(302);
    const authorize = new URL(response.headers.get('Location')!);
    expect(authorize.origin + authorize.pathname).toBe('https://simkl.com/oauth2/authorize');
    expect(Object.fromEntries(authorize.searchParams)).toMatchObject({
      response_type: 'code', client_id: 'client-id', redirect_uri: env.OAUTH_REDIRECT_URI,
      state: 'state-123', scope: 'media:read media:write', code_challenge_method: 'S256',
    });
    expect(authorize.searchParams.get('code_challenge')).toMatch(/^[A-Za-z0-9_-]{43}$/);
    expect(puts).toHaveLength(1);
    expect(puts[0].key).toBe('oauth_state:state-123');
    expect(puts[0].expirationTtl).toBe(600);
    const pending = JSON.parse(values.get('oauth_state:state-123')!);
    expect(pending.oauthRequest).toMatchObject({ clientId: 'client', scope: 'read write' });
    expect(pending.codeVerifier).toMatch(/^[A-Za-z0-9_-]{43,128}$/);
    expect(JSON.stringify(pending)).not.toContain(env.SIMKL_CLIENT_SECRET);
  });

  test('exchanges callback once, completes with Simkl props and deletes pending state', async () => {
    const { env, values, deletes, completeAuthorization } = setup();
    values.set('oauth_state:s', JSON.stringify({ oauthRequest: { clientId: 'c', scope: 'read write' }, codeVerifier: 'stored-verifier-abcdefghijklmnopqrstuvwxyz0123456789', createdAt: Date.now() }));
    successfulFetch();
    const response = await handler.fetch(new Request('https://service.example/oauth/callback?code=single-use&state=s&iss=https%3A%2F%2Fsimkl.com'), env);
    expect(response.status).toBe(302);
    expect(response.headers.get('Location')).toBe('https://client.example/done');
    expect(globalThis.fetch).toHaveBeenCalledTimes(2);
    const tokenCall = (globalThis.fetch as ReturnType<typeof mock>).mock.calls[0];
    const form = new URLSearchParams(String(tokenCall[1]?.body));
    expect(form.get('code_verifier')).toBe('stored-verifier-abcdefghijklmnopqrstuvwxyz0123456789');
    expect(completeAuthorization).toHaveBeenCalledWith(expect.objectContaining({
      request: { clientId: 'c', scope: 'read write' }, scope: ['read', 'write'], userId: 'simkl_user_42',
      props: expect.objectContaining({ simklToken: 'access', simklRefreshToken: 'refresh' }),
    }));
    expect(deletes).toContain('oauth_state:s');
  });

  test.each([
    ['missing code', '?state=s&iss=https%3A%2F%2Fsimkl.com'],
    ['missing state', '?code=c&iss=https%3A%2F%2Fsimkl.com'],
    ['missing issuer', '?code=c&state=s'],
    ['wrong issuer', '?code=c&state=s&iss=https%3A%2F%2Fevil.example'],
    ['Simkl error', '?error=access_denied&state=s&iss=https%3A%2F%2Fsimkl.com'],
  ])('rejects %s without an exchange', async (_label, query) => {
    const { env, values } = setup();
    values.set('oauth_state:s', JSON.stringify({ oauthRequest: {}, codeVerifier: 'abcdefghijklmnopqrstuvwxyz0123456789ABCDEFG', createdAt: Date.now() }));
    globalThis.fetch = mock(async () => { throw new Error('should not fetch'); }) as typeof fetch;
    const response = await handler.fetch(new Request(`https://service.example/oauth/callback${query}`), env);
    expect(response.status).toBe(400);
    expect(globalThis.fetch).not.toHaveBeenCalled();
  });

  test('rejects missing, expired and malformed pending state without exchanging', async () => {
    const { env, values } = setup();
    globalThis.fetch = mock(async () => { throw new Error('should not fetch'); }) as typeof fetch;
    const missing = await handler.fetch(new Request('https://service.example/oauth/callback?code=c&state=missing&iss=https%3A%2F%2Fsimkl.com'), env);
    expect(missing.status).toBe(400);
    values.set('oauth_state:bad-json', '{');
    const malformed = await handler.fetch(new Request('https://service.example/oauth/callback?code=c&state=bad-json&iss=https%3A%2F%2Fsimkl.com'), env);
    expect(malformed.status).toBe(400);
    values.set('oauth_state:bad-shape', JSON.stringify({}));
    const badShape = await handler.fetch(new Request('https://service.example/oauth/callback?code=c&state=bad-shape&iss=https%3A%2F%2Fsimkl.com'), env);
    expect(badShape.status).toBe(400);
    expect(globalThis.fetch).not.toHaveBeenCalled();
  });

  test('consumes state before one failed exchange and does not retry a replayed code', async () => {
    const { env, values, deletes } = setup();
    values.set('oauth_state:s', JSON.stringify({ oauthRequest: {}, codeVerifier: 'abcdefghijklmnopqrstuvwxyz0123456789ABCDEFG', createdAt: Date.now() }));
    let statePresentAtExchange = true;
    globalThis.fetch = mock(async (input: RequestInfo | URL) => {
      const url = new URL(String(input));
      if (url.pathname === '/oauth/token') {
        statePresentAtExchange = values.has('oauth_state:s');
        return new Response('sensitive upstream body', { status: 401 });
      }
      throw new Error('settings should not be called');
    }) as typeof fetch;
    const callback = 'https://service.example/oauth/callback?code=single-use&state=s&iss=https%3A%2F%2Fsimkl.com';
    const first = await handler.fetch(new Request(callback), env);
    const replay = await handler.fetch(new Request(callback), env);
    expect(first.status).toBe(502);
    expect(await first.text()).not.toContain('sensitive');
    expect(replay.status).toBe(400);
    expect(statePresentAtExchange).toBe(false);
    expect(globalThis.fetch).toHaveBeenCalledTimes(1);
    expect(deletes).toContain('oauth_state:s');
  });

  test('uses GET and current identification headers for user settings', async () => {
    const { env, values } = setup();
    values.set('oauth_state:s', JSON.stringify({ oauthRequest: {}, codeVerifier: 'abcdefghijklmnopqrstuvwxyz0123456789ABCDEFG', createdAt: Date.now() }));
    successfulFetch();
    await handler.fetch(new Request('https://service.example/oauth/callback?code=c&state=s&iss=https%3A%2F%2Fsimkl.com'), env);
    const settingsCall = (globalThis.fetch as ReturnType<typeof mock>).mock.calls[1];
    expect(String(settingsCall[0])).toBe('https://api.simkl.com/users/settings?client_id=client-id&app-name=simkl-mcp&app-version=1.0.0');
    expect(settingsCall[1]?.method).toBe('GET');
    const headers = new Headers(settingsCall[1]?.headers);
    expect(headers.get('User-Agent')).toBe('simkl-mcp/1.0.0');
    expect(headers.get('Authorization')).toBe('Bearer access');
  });
});



