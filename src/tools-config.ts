import { asAiring, formatList, withImdb, withRating } from './format/media.js';

type ResponseFormat =
  | { type: 'simple', template: (result: any, args: any) => string | string[] }
  | { type: 'json' };

interface ToolConfig {
  path: string;
  method: string;
  schemaMethod?: string;
  authorization?: 'bearer' | 'none';
  requestPath?: { helper: string; args: string[] };
  name?: string;
  responseFormat?: ResponseFormat;
  omitParams?: string[];
}

export const toolsWhitelist: ToolConfig[] = [
  // search
  { path: '/search/:type', method: 'get', responseFormat: {
    type: 'simple',
    template: (results: any, args: any) => results.length > 0
      ? formatList(results, withImdb, 10)
      : [`no results for "${args.q}"`]
  }},
  { path: '/search/id', method: 'get', responseFormat: { type: 'json' }},

  // scrobble
  { path: '/scrobble/start', method: 'post', responseFormat: {
    type: 'simple',
    template: (_: any, args: any) => `started: ${args.movie?.title || args.show?.title}`
  }},
  { path: '/scrobble/pause', method: 'post', responseFormat: {
    type: 'simple',
    template: (_: any, args: any) => `paused at ${args.progress}%: ${args.movie?.title || args.show?.title}`
  }},
  { path: '/scrobble/stop', method: 'post', responseFormat: {
    type: 'simple',
    template: (_: any, args: any) => `stopped: ${args.movie?.title || args.show?.title}`
  }},

  // sync
  { path: '/sync/add-to-list', method: 'post', responseFormat: {
    type: 'simple',
    template: (_: any, args: any) => `added to watchlist: ${(args.movie || args.show || args.anime)?.title}`
  }},
  { path: '/sync/history', method: 'post', responseFormat: { 'type': 'json' }},
  { path: '/sync/history/remove', method: 'post', responseFormat: { 'type': 'json' }},
  { path: '/sync/ratings', method: 'post', responseFormat: { 'type': 'json' }},
  { path: '/sync/ratings/remove', method: 'post', responseFormat: { 'type': 'json' }},

  // discovery
  { path: '/tv/trending/:interval', method: 'get', authorization: 'none', requestPath: { helper: 'simklTrendingPath', args: ["'tv'", 'args.interval'] }, responseFormat: {
    type: 'simple',
    template: (results: any) => formatList(results)
  }},
  { path: '/movies/trending/:interval', method: 'get', authorization: 'none', requestPath: { helper: 'simklTrendingPath', args: ["'movies'", 'args.interval'] }, responseFormat: {
    type: 'simple',
    template: (results: any) => formatList(results)
  }},
  { path: '/anime/trending/:interval', method: 'get', authorization: 'none', requestPath: { helper: 'simklTrendingPath', args: ["'anime'", 'args.interval'] }, responseFormat: {
    type: 'simple',
    template: (results: any) => formatList(results)
  }},

  { path: '/tv/:id', method: 'get', authorization: 'none', responseFormat: { type: 'json' }},
  { path: '/movies/:id', method: 'get', authorization: 'none', responseFormat: { type: 'json' }},
  { path: '/anime/:id', method: 'get', authorization: 'none', responseFormat: { type: 'json' }},

  { path: '/tv/episodes/:id', method: 'get', authorization: 'none', responseFormat: { type: 'json' }},
  { path: '/anime/episodes/:id', method: 'get', authorization: 'none', responseFormat: { type: 'json' }},

  { path: '/tv/best/:filter', method: 'get', responseFormat: {
    type: 'simple',
    template: (results: any) => formatList(results, withRating)
  }},
  { path: '/anime/best/:filter', method: 'get', responseFormat: {
    type: 'simple',
    template: (results: any) => formatList(results, withRating)
  }},

  { path: '/tv/airing?:date', method: 'get', responseFormat: {
    type: 'simple',
    template: (results: any) => formatList(results, asAiring)
  }},
  { path: '/anime/airing?:date', method: 'get', responseFormat: {
    type: 'simple',
    template: (results: any) => formatList(results, asAiring)
  }},

  // genre filtering
  { path: '/tv/genres/:genre/:type/:country/:network/:year/:sort', method: 'get', responseFormat: {
    type: 'simple',
    template: (results: any) => formatList(results)
  }},
  { path: '/anime/genres/:genre/:type/:network/:year/:sort', method: 'get', responseFormat: {
    type: 'simple',
    template: (results: any) => formatList(results)
  }},
  { path: '/movies/genres/:genre/:type/:country/:year/:sort', method: 'get', responseFormat: {
    type: 'simple',
    template: (results: any) => formatList(results)
  }},

  // user stats
  { path: '/users/:user_id/stats', method: 'get', schemaMethod: 'post', responseFormat: { type: 'json' }},

  // watchlist (sync)
  { path: '/sync/all-items/:type/:status', method: 'get', responseFormat: { type: 'json' }},
];
