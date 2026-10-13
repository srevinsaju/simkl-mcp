import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import { registerTools } from '../generated/tools';
import { SimklClient } from '../src/api/client';

// Exercise generated registration, validation, HTTP policy, and response shaping together.
// Removing slug validation, result bounds, attribution, or metadata must break these tests.
describe('genre trending tool', () => {
  const originalFetch = globalThis.fetch;
  let requests: Array<{ url: URL; headers: Headers }>;
  let index: unknown;
  let titles: unknown;
  let lastModified: string | undefined;
  let failure: number | undefined;
  let handler: (args: any) => Promise<any>;
  let config: any;

  beforeEach(() => {
    requests = [];
    index = [{ name: 'Drama', slug: 'drama', count: 500 }];
    titles = Array.from({ length: 125 }, (_, i) => ({
      title: `Title ${i}`, ids: { simkl: i + 1, slug: `title-${i}` },
      overview: 'Rich fields retained', ratings: { simkl: { rating: 8 } },
    }));
    lastModified = 'Tue, 13 Oct 2026 00:00:00 GMT';
    failure = undefined;
    globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = new URL(String(input));
      requests.push({ url, headers: new Headers(init?.headers) });
      if (failure) return new Response('unavailable', { status: failure });
      return new Response(JSON.stringify(url.pathname.endsWith('/index.json') ? index : titles), {
        headers: lastModified ? { 'Last-Modified': lastModified } : {},
      });
    }) as typeof fetch;
    config = undefined;
    handler = undefined as never;
    registerTools({ registerTool(name: string, toolConfig: unknown, toolHandler: any) {
      if (name === 'simkl_get_trending_by_genre') { config = toolConfig; handler = toolHandler; }
    } } as never, new SimklClient({ baseUrl: 'https://api.simkl.com', clientId: 'app-id' }), () => 'private-token');
  });

  afterEach(() => { globalThis.fetch = originalFetch; });

  async function invoke(args: unknown) {
    expect(config).toBeDefined();
    return handler(config.inputSchema.parse(args));
  }

  test.each(['tv', 'movies', 'anime'])('returns bounded rich monthly %s results without a bearer token', async type => {
    const result = await invoke({ type, genre: 'drama', maxResults: 1 });
    expect(result.isError).not.toBe(true);
    const data = JSON.parse(result.content[0].text);
    expect(requests.map(r => r.url.origin + r.url.pathname)).toEqual([
      `https://data.simkl.in/discover/trending/${type}/genre/index.json`,
      `https://data.simkl.in/discover/trending/${type}/genre/drama_month_500.json`,
    ]);
    expect(requests.every(r => !r.headers.has('Authorization'))).toBe(true);
    expect(data).toMatchObject({ type, period: 'month', updatedAt: lastModified, genre: { name: 'Drama', slug: 'drama', count: 500 }, total: 125, returned: 1 });
    expect(data.results).toHaveLength(1);
    expect(data.results[0]).toMatchObject({ overview: 'Rich fields retained', ratings: { simkl: { rating: 8 } }, simkl_url: `https://simkl.com/${type}/1/title-0` });
    expect(config.annotations).toMatchObject({ readOnlyHint: true, idempotentHint: true });
  });

  test('defaults to 20 results and permits a maximum of 100', async () => {
    expect(JSON.parse((await invoke({ type: 'tv', genre: 'drama' })).content[0].text).results).toHaveLength(20);
    expect(JSON.parse((await invoke({ type: 'tv', genre: 'drama', maxResults: 100 })).content[0].text).returned).toBe(100);
  });

  test.each([0, 101, 1.5])('rejects maxResults %s before a request', async maxResults => {
    expect(config).toBeDefined();
    await expect(invoke({ type: 'tv', genre: 'drama', maxResults })).rejects.toThrow();
    expect(requests).toHaveLength(0);
  });

  test.each(['../drama', 'drama/other', 'Drama', '', 'a'.repeat(101)])('rejects invalid slug %s before a request', async genre => {
    expect(config).toBeDefined();
    await expect(invoke({ type: 'tv', genre })).rejects.toThrow();
    expect(requests).toHaveLength(0);
  });

  test('rejects unsupported media before a request', async () => {
    expect(config).toBeDefined();
    await expect(invoke({ type: 'books', genre: 'drama' })).rejects.toThrow();
    expect(requests).toHaveLength(0);
  });

  test('returns an MCP error for a genre absent from the type index', async () => {
    const result = await invoke({ type: 'anime', genre: 'missing' });
    expect(result.isError).toBe(true);
    expect(result.content[0].text).toContain('not available for anime');
    expect(requests).toHaveLength(1);
  });

  test('uses null freshness metadata when Last-Modified is absent', async () => {
    lastModified = undefined;
    titles = [];
    const data = JSON.parse((await invoke({ type: 'tv', genre: 'drama' })).content[0].text);
    expect(data).toMatchObject({ updatedAt: null, total: 0, returned: 0, results: [] });
  });

  test.each(['index', 'titles'])('reports malformed %s payloads instead of a successful empty result', async payload => {
    if (payload === 'index') index = {}; else titles = {};
    expect((await invoke({ type: 'tv', genre: 'drama' })).isError).toBe(true);
  });

  test('returns a sanitized MCP error for failed CDN reads', async () => {
    failure = 503;
    const result = await invoke({ type: 'tv', genre: 'drama' });
    expect(result.isError).toBe(true);
    expect(result.content[0].text).toContain('503');
    expect(result.content[0].text).not.toContain('private-token');
  });
});
