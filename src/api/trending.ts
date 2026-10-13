export type SimklMediaType = 'tv' | 'movies' | 'anime';
// Same vocabulary as the `interval` parameter in the Simkl API spec, which is
// also the name of the file on the public trending host.
export type SimklTrendingInterval = 'today' | 'week' | 'month';

export function simklTrendingPath(type: SimklMediaType, interval: SimklTrendingInterval): string {
  return `https://data.simkl.in/discover/trending/${type}/${interval}_100.json`;
}

export interface SimklGenre {
  name: string;
  slug: string;
  count: number;
}

interface GenreTrendingClient {
  requestWithMetadata<T = unknown>(endpoint: string, options: {
    method: 'GET'; authorization: 'none';
  }): Promise<{ data: T; headers: { lastModified?: string } }>;
}

interface GenreTitle {
  ids?: { simkl?: number; simkl_id?: number; slug?: string };
  [key: string]: unknown;
}

export async function getTrendingByGenre(
  client: GenreTrendingClient, type: SimklMediaType, genreSlug: string, maxResults = 20,
) {
  if (!['tv', 'movies', 'anime'].includes(type)) throw new Error('Unsupported media type');
  if (typeof genreSlug !== 'string' || genreSlug.length > 100 || !/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(genreSlug)) {
    throw new Error('Invalid genre slug');
  }
  if (!Number.isInteger(maxResults) || maxResults < 1 || maxResults > 100) {
    throw new Error('maxResults must be an integer from 1 to 100');
  }
  const base = `https://data.simkl.in/discover/trending/${type}/genre`;
  const options = { method: 'GET', authorization: 'none' } as const;
  const index = await client.requestWithMetadata<SimklGenre[]>(`${base}/index.json`, options);
  if (!Array.isArray(index.data)) throw new Error('Invalid genre index response');
  const genre = index.data.find(entry => entry?.slug === genreSlug);
  if (!genre) throw new Error(`Genre slug not available for ${type}. Choose a slug from that type's genre index.`);
  const file = await client.requestWithMetadata<GenreTitle[]>(`${base}/${genreSlug}_month_500.json`, options);
  if (!Array.isArray(file.data) || file.data.some(item => !item || typeof item !== 'object' || Array.isArray(item))) {
    throw new Error('Invalid genre trending response');
  }
  const results = file.data.slice(0, maxResults).map(item => {
    const id = item.ids?.simkl_id ?? item.ids?.simkl;
    if (!Number.isInteger(id) || (id as number) <= 0) return { ...item };
    const slug = typeof item.ids?.slug === 'string' && item.ids.slug.trim() ? item.ids.slug : undefined;
    const url = `https://simkl.com/${type}/${id}${slug ? `/${encodeURIComponent(slug)}` : ''}`;
    return { ...item, simkl_url: url };
  });
  return {
    type, period: 'month', updatedAt: file.headers.lastModified ?? null, genre,
    total: file.data.length, returned: results.length, results,
  };
}
