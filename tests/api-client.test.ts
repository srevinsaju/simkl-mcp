import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import { APP_NAME, APP_VERSION, USER_AGENT } from '../src/app-info';
import { SimklClient } from '../src/api/client';
import { toolsWhitelist } from '../src/tools-config';

describe('SimklClient request policy', () => {
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

  test('adds Simkl identification to relative API requests', async () => {
    const client = new SimklClient({ baseUrl: 'https://api.simkl.com', clientId: 'app-id' });
    await client.request('/movies/trending/today', { method: 'GET' });

    const { url, init } = requests[0];
    expect(url.origin).toBe('https://api.simkl.com');
    expect(url.pathname).toBe('/movies/trending/today');
    expect(url.searchParams.get('client_id')).toBe('app-id');
    expect(url.searchParams.get('app-name')).toBe(APP_NAME);
    expect(url.searchParams.get('app-version')).toBe(APP_VERSION);
    expect(new Headers(init.headers).get('User-Agent')).toBe(USER_AGENT);
  });

  test('adds identification to absolute HTTPS endpoints and preserves their query', async () => {
    const client = new SimklClient({ baseUrl: 'https://api.simkl.com', clientId: 'app-id' });
    await client.request('https://data.simkl.in/discover/trending/movies/today_100.json?existing=yes', {
      method: 'GET', query: { period: 'daily' },
    });

    const { url } = requests[0];
    expect(url.origin).toBe('https://data.simkl.in');
    expect(url.pathname).toBe('/discover/trending/movies/today_100.json');
    expect(url.searchParams.get('existing')).toBe('yes');
    expect(url.searchParams.get('period')).toBe('daily');
    expect(url.searchParams.get('client_id')).toBe('app-id');
    expect(url.searchParams.get('app-name')).toBe(APP_NAME);
    expect(url.searchParams.get('app-version')).toBe(APP_VERSION);
    expect(new Headers(requests[0].init.headers).get('User-Agent')).toBe(USER_AGENT);
  });

  test('uses bearer authorization by default when a token is provided', async () => {
    const client = new SimklClient({ baseUrl: 'https://api.simkl.com', clientId: 'app-id' });
    await client.request('/users/settings', { method: 'GET', token: 'user-token' });
    expect(new Headers(requests[0].init.headers).get('Authorization')).toBe('Bearer user-token');
  });

  test('omits authorization when explicitly configured as none', async () => {
    const client = new SimklClient({ baseUrl: 'https://api.simkl.com', clientId: 'app-id' });
    await client.request('/movies/42', { method: 'GET', token: 'user-token', authorization: 'none' });
    expect(new Headers(requests[0].init.headers).has('Authorization')).toBe(false);
  });

  test('refuses to send a token to a different origin', async () => {
    const client = new SimklClient({ baseUrl: 'https://api.simkl.com', clientId: 'app-id' });
    await expect(
      client.request('https://data.simkl.in/discover/trending/tv/today_100.json', { method: 'GET', token: 'user-token' })
    ).rejects.toThrow(/different origin/);
    expect(requests).toHaveLength(0);
  });

  test('allows a different origin when no token is sent', async () => {
    const client = new SimklClient({ baseUrl: 'https://api.simkl.com', clientId: 'app-id' });
    await client.request('https://data.simkl.in/discover/trending/tv/today_100.json', { method: 'GET', token: 'user-token', authorization: 'none' });
    expect(requests).toHaveLength(1);
    expect(new Headers(requests[0].init.headers).has('Authorization')).toBe(false);
  });
});

describe('generated tool request policy configuration', () => {
  test('marks all public catalog tools as unauthenticated', () => {
    const publicPaths = ['/movies/:id', '/tv/:id', '/anime/:id', '/tv/episodes/:id', '/anime/episodes/:id'];
    for (const path of publicPaths) {
      expect(toolsWhitelist.find(tool => tool.path === path)?.authorization).toBe('none');
    }
  });

  test('uses GET for stats while reading the existing POST schema', () => {
    const stats = toolsWhitelist.find(tool => tool.path === '/users/:user_id/stats');
    expect(stats?.method).toBe('get');
    expect(stats?.schemaMethod).toBe('post');
  });

  test('configures existing trending tools for public data files', () => {
    const trendingTools = [
      '/tv/trending/:interval',
      '/movies/trending/:interval',
      '/anime/trending/:interval',
    ];

    for (const path of trendingTools) {
      const tool = toolsWhitelist.find(candidate => candidate.path === path);
      expect(tool?.method).toBe('get');
      expect(tool?.authorization).toBe('none');
      expect(tool?.requestPath?.helper).toBe('simklTrendingPath');
    }
  });
});
