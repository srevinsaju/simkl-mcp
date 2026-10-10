export type SimklMediaType = 'tv' | 'movies' | 'anime';
// Same vocabulary as the `interval` parameter in the Simkl API spec, which is
// also the name of the file on the public trending host.
export type SimklTrendingInterval = 'today' | 'week' | 'month';

export function simklTrendingPath(type: SimklMediaType, interval: SimklTrendingInterval): string {
  return `https://data.simkl.in/discover/trending/${type}/${interval}_100.json`;
}
