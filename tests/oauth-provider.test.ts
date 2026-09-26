import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, mock } from 'bun:test';
import type { SimklAuthProps } from '../src/auth/simkl-oauth.js';

let createSimklTokenExchangeCallback: typeof import('../src/index.js').createSimklTokenExchangeCallback;
let createOAuthProvider: typeof import('../src/index.js').createOAuthProvider;

const env = (id: string, secret: string) => ({
  SIMKL_CLIENT_ID: id,
  SIMKL_CLIENT_SECRET: secret,
  OAUTH_REDIRECT_URI: 'https://example.test/callback',
});

function props(expiresAt: number, refreshToken = 'refresh-old'): SimklAuthProps {
  return {
    simklToken: 'access-old',
    simklRefreshToken: refreshToken,
    simklExpiresAt: expiresAt,
    simklRefreshExpiresAt: expiresAt + 180 * 24 * 60 * 60 * 1000,
    simklScope: 'media:read',
    custom: 'preserved',
  };
}

function tokenResponse(accessToken: string, refreshToken?: string, expiresIn = 3600) {
  return new Response(JSON.stringify({ access_token: accessToken, refresh_token: refreshToken, expires_in: expiresIn }), {
    status: 200,
    headers: { 'content-type': 'application/json' },
  });
}

const originalFetch = globalThis.fetch;
const originalNow = Date.now;

beforeEach(() => { Date.now = () => 1_000_000; });
beforeAll(async () => {
  mock.module('cloudflare:workers', () => ({ WorkerEntrypoint: class {}, DurableObject: class {}, env: {} }));
  mock.module('cloudflare:email', () => ({ EmailMessage: class {} }));
  ({ createSimklTokenExchangeCallback, createOAuthProvider } = await import('../src/index.js'));
});
afterAll(() => mock.restore());
afterEach(() => {
  globalThis.fetch = originalFetch;
  Date.now = originalNow;
});

describe('Simkl provider token exchange callback', () => {
  it('uses the initial authorization token expiry without an upstream request', async () => {
    globalThis.fetch = mock(() => { throw new Error('unexpected fetch'); }) as typeof fetch;
    const initial = props(Date.now() + 12_345);
    const result = await createSimklTokenExchangeCallback(env('client', 'secret'))({
      grantType: 'authorization_code', clientId: 'local-client', userId: 'simkl-user', scope: [], props: initial,
    });
    expect(result.newProps).toEqual(initial);
    expect(result.accessTokenTTL).toBe(12);
    expect(globalThis.fetch).not.toHaveBeenCalled();
  });

  it('does not refresh a valid token and bounds its TTL to the stored expiry', async () => {
    globalThis.fetch = mock(() => { throw new Error('unexpected fetch'); }) as typeof fetch;
    const current = props(Date.now() + 90_987);
    const result = await createSimklTokenExchangeCallback(env('client', 'secret'))({
      grantType: 'refresh_token', clientId: 'local-client', userId: 'simkl-user', scope: [], props: current,
    });
    expect(result.newProps).toEqual(current);
    expect(result.accessTokenTTL).toBe(90);
    expect(globalThis.fetch).not.toHaveBeenCalled();
  });

  it.each([['inside safety window', 50_000], ['expired', -1_000]])('refreshes when token is %s', async (_label, remaining) => {
    globalThis.fetch = mock(() => Promise.resolve(tokenResponse('access-new', 'refresh-new', 120))) as typeof fetch;
    const current = props(Date.now() + remaining);
    const result = await createSimklTokenExchangeCallback(env('client', 'secret'))({
      grantType: 'refresh_token', clientId: 'local-client', userId: 'simkl-user', scope: [], props: current,
    });
    expect(globalThis.fetch).toHaveBeenCalledTimes(1);
    expect(result.accessTokenTTL).toBe(120);
    expect(result.newProps).toMatchObject({
      simklToken: 'access-new', simklRefreshToken: 'refresh-new', custom: 'preserved', simklScope: 'media:read',
    });
  });

  it('coalesces simultaneous refreshes from separate request callbacks and preserves the old refresh token', async () => {
    let resolveResponse!: (response: Response) => void;
    let authorization: string | null = null;
    globalThis.fetch = mock(((_input: RequestInfo | URL, init?: RequestInit) => {
      authorization = new Headers(init?.headers).get('Authorization');
      return new Promise<Response>(resolve => { resolveResponse = resolve; });
    }) as typeof fetch) as typeof fetch;
    const callbackA = createSimklTokenExchangeCallback(env('request-a', 'secret-a'));
    const callbackB = createSimklTokenExchangeCallback(env('request-b', 'secret-b'));
    const options = { grantType: 'refresh_token' as const, clientId: 'local-client', userId: 'simkl-user', scope: [], props: props(Date.now()) };
    const first = callbackA(options);
    const second = callbackB(options);
    await Promise.resolve();
    expect(globalThis.fetch).toHaveBeenCalledTimes(1);
    expect(atob(authorization!.slice('Basic '.length))).toBe('request-a:secret-a');
    resolveResponse(tokenResponse('access-new'));
    const [a, b] = await Promise.all([first, second]);
    expect(a.newProps).toEqual(b.newProps);
    expect(a.newProps).toMatchObject({ simklRefreshToken: 'refresh-old', simklToken: 'access-new' });
  });

  it('reports a sanitized failure without exposing credentials or refresh tokens', async () => {
    globalThis.fetch = mock(() => Promise.resolve(new Response('failure', { status: 401 }))) as typeof fetch;
    const secretEnv = env('client-secret-id', 'very-secret-client-secret');
    const callback = createSimklTokenExchangeCallback(secretEnv);
    const error = await callback({
      grantType: 'refresh_token', clientId: 'local-client', userId: 'simkl-user', scope: [], props: props(Date.now()),
    }).catch(caught => caught as Error);
    expect(error.message).toBe('Unable to refresh Simkl token');
    expect(error.message).not.toMatch(/very-secret-client-secret|refresh-old/);
    expect(String(globalThis.fetch.mock.calls[0]?.[1]?.headers)).not.toContain('very-secret-client-secret');
  });

  it('binds each callback to the Worker env supplied when its provider is created', async () => {
    let authorization: string | null = null;
    globalThis.fetch = mock(((_input: RequestInfo | URL, init?: RequestInit) => {
      authorization = new Headers(init?.headers).get('Authorization');
      return Promise.resolve(tokenResponse('access-new', 'refresh-new'));
    }) as typeof fetch) as typeof fetch;
    const opts = { grantType: 'refresh_token' as const, clientId: 'local-client', userId: 'simkl-user', scope: [], props: props(Date.now()) };
    const firstRequestCallback = createSimklTokenExchangeCallback(env('first-client', 'first-secret'));
    const secondRequestCallback = createSimklTokenExchangeCallback(env('second-client', 'second-secret'));
    await firstRequestCallback(opts);
    const firstAuthorization = authorization;
    expect(atob(firstAuthorization!.slice('Basic '.length))).toBe('first-client:first-secret');
    await secondRequestCallback(opts);
    expect(authorization).not.toBe(firstAuthorization);
    expect(atob(authorization!.slice('Basic '.length))).toBe('second-client:second-secret');
  });

  it('routes a request through the provider created for its Worker env', async () => {
    const provider = createOAuthProvider({
      ...env('worker-client', 'worker-secret'),
      OAUTH_KV: {} as KVNamespace,
    });
    const response = await provider.fetch(
      new Request('https://worker.test/health'),
      { OAUTH_KV: {} as KVNamespace },
      { waitUntil() {}, passThroughOnException() {} } as ExecutionContext,
    );
    expect(response.status).toBe(200);
    expect(await response.json()).toEqual({ status: 'ok', service: 'simkl-mcp' });
  });
});
