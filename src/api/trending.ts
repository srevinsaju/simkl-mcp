export type SimklMediaType = 'tv' | 'movies' | 'anime';
export type SimklTrendingInterval = 'daily' | 'weekly' | 'monthly';

const timeframeByInterval: Record<SimklTrendingInterval, string> = {
  daily: 'today',
  weekly: 'week',
  monthly: 'month',
};

export function simklTrendingPath(type: SimklMediaType, interval: SimklTrendingInterval): string {
  return `https://data.simkl.in/discover/trending/${type}/${timeframeByInterval[interval]}_100.json`;
}
