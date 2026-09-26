import { describe, expect, test } from 'bun:test';
import { simklTrendingPath } from '../src/api/trending';

describe('simklTrendingPath', () => {
  test.each([
    ['tv', 'daily', 'tv/today_100.json'],
    ['movies', 'daily', 'movies/today_100.json'],
    ['anime', 'daily', 'anime/today_100.json'],
    ['tv', 'weekly', 'tv/week_100.json'],
    ['movies', 'weekly', 'movies/week_100.json'],
    ['anime', 'weekly', 'anime/week_100.json'],
    ['tv', 'monthly', 'tv/month_100.json'],
    ['movies', 'monthly', 'movies/month_100.json'],
    ['anime', 'monthly', 'anime/month_100.json'],
  ] as const)('%s %s maps to the official public file', (type, interval, expectedPath) => {
    expect(simklTrendingPath(type, interval)).toBe(`https://data.simkl.in/discover/trending/${expectedPath}`);
  });
});
