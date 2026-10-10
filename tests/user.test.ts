import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import { SimklClient } from '../src/api/client';
import { getCurrentUserSettings, getUserStats } from '../src/api/user';

describe('current user API helpers', () => {
  let originalFetch: typeof fetch;
  let requests: Array<{ url: URL; init: RequestInit }>;

  beforeEach(() => {
    originalFetch = globalThis.fetch;
    requests = [];
    globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
      requests.push({ url: new URL(String(input)), init: init ?? {} });
      return new Response('{}', { status: 200, headers: { 'Content-Type': 'application/json' } });
    }) as typeof fetch;
  });

  afterEach(() => {
    globalThis.fetch = originalFetch;
  });

  const createClient = () => new SimklClient({ baseUrl: 'https://api.simkl.com', clientId: 'app-id' });

  test('gets the current user settings with bearer authorization', async () => {
    await getCurrentUserSettings(createClient(), 'user-token');
    expect(requests[0].url.pathname).toBe('/users/settings');
    expect(requests[0].init.method).toBe('GET');
    expect(new Headers(requests[0].init.headers).get('Authorization')).toBe('Bearer user-token');
  });

  test('gets user stats with bearer authorization', async () => {
    await getUserStats(createClient(), 123, 'user-token');
    expect(requests[0].url.pathname).toBe('/users/123/stats');
    expect(requests[0].init.method).toBe('GET');
    expect(new Headers(requests[0].init.headers).get('Authorization')).toBe('Bearer user-token');
  });
});
