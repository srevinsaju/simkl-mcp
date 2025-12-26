type ResponseFormat =
  | { type: 'simple', template: (result: any, args: any) => string }
  | { type: 'json' };

interface ToolConfig {
  path: string;
  method: string;
  name?: string;
  responseFormat?: ResponseFormat;
  omitParams?: string[];
}

export const toolsWhitelist: ToolConfig[] = [
  // search
  { path: '/search/:type', method: 'get', responseFormat: {
    type: 'simple',
    template: (results: any, args: any) => results.length > 0
      ? results.slice(0, 10).map((r: any, i: number) =>
          `${i}: [SIMKL #${r.ids?.simkl_id}] - ${r.title} (${r.year || 'N/A'}) - imdb:${r.ids?.imdb || 'N/A'}`
        )
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
  { path: '/tv/trending/:interval', method: 'get', responseFormat: {
    type: 'simple',
    template: (results: any) => results.slice(0, 20).map((r: any, i: number) => `${i}: [SIMKL #${r.ids?.simkl_id || r.ids?.simkl}] - ${r.title} (${r.year || 'N/A'})`)
  }},
  { path: '/movies/trending/:interval', method: 'get', responseFormat: {
    type: 'simple',
    template: (results: any) => results.slice(0, 20).map((r: any, i: number) => `${i}: [SIMKL #${r.ids?.simkl_id || r.ids?.simkl}] - ${r.title} (${r.year || 'N/A'})`)
  }},
  { path: '/anime/trending/:interval', method: 'get', responseFormat: {
    type: 'simple',
    template: (results: any) => results.slice(0, 20).map((r: any, i: number) => `${i}: [SIMKL #${r.ids?.simkl_id || r.ids?.simkl}] - ${r.title} (${r.year || 'N/A'})`)
  }},

  { path: '/tv/:id', method: 'get', responseFormat: { type: 'json' }},
  { path: '/movies/:id', method: 'get', responseFormat: { type: 'json' }},
  { path: '/anime/:id', method: 'get', responseFormat: { type: 'json' }},

  { path: '/tv/episodes/:id', method: 'get', responseFormat: { type: 'json' }},
  { path: '/anime/episodes/:id', method: 'get', responseFormat: { type: 'json' }},

  { path: '/tv/best/:filter', method: 'get', responseFormat: {
    type: 'simple',
    template: (results: any) => results.slice(0, 20).map((r: any, i: number) =>
      `${i}: [SIMKL #${r.ids?.simkl_id || r.ids?.simkl}] - ${r.title} (${r.year || 'N/A'}) - rating: ${r.ratings?.simkl?.rating || 'N/A'}`
    )
  }},
  { path: '/anime/best/:filter', method: 'get', responseFormat: {
    type: 'simple',
    template: (results: any) => results.slice(0, 20).map((r: any, i: number) =>
      `${i}: [SIMKL #${r.ids?.simkl_id || r.ids?.simkl}] - ${r.title} (${r.year || 'N/A'}) - rating: ${r.ratings?.simkl?.rating || 'N/A'}`
    )
  }},

  { path: '/tv/airing?:date', method: 'get', responseFormat: {
    type: 'simple',
    template: (results: any) => results.slice(0, 20).map((r: any, i: number) =>
      `${i}: [SIMKL #${r.ids?.simkl_id || r.ids?.simkl}] - ${r.title} - ep ${r.episode?.episode || '?'} at ${r.date || 'TBA'}`
    )
  }},
  { path: '/anime/airing?:date', method: 'get', responseFormat: {
    type: 'simple',
    template: (results: any) => results.slice(0, 20).map((r: any, i: number) =>
      `${i}: [SIMKL #${r.ids?.simkl_id || r.ids?.simkl}] - ${r.title} - ep ${r.episode?.episode || '?'} at ${r.date || 'TBA'}`
    )
  }},

  // genre filtering
  { path: '/tv/genres/:genre/:type/:country/:network/:year/:sort', method: 'get', responseFormat: {
    type: 'simple',
    template: (results: any) => results.slice(0, 20).map((r: any, i: number) => `${i}: [SIMKL #${r.ids?.simkl_id || r.ids?.simkl}] - ${r.title} (${r.year || 'N/A'})`)
  }},
  { path: '/anime/genres/:genre/:type/:network/:year/:sort', method: 'get', responseFormat: {
    type: 'simple',
    template: (results: any) => results.slice(0, 20).map((r: any, i: number) => `${i}: [SIMKL #${r.ids?.simkl_id || r.ids?.simkl}] - ${r.title} (${r.year || 'N/A'})`)
  }},
  { path: '/movies/genres/:genre/:type/:country/:year/:sort', method: 'get', responseFormat: {
    type: 'simple',
    template: (results: any) => results.slice(0, 20).map((r: any, i: number) => `${i}: [SIMKL #${r.ids?.simkl_id || r.ids?.simkl}] - ${r.title} (${r.year || 'N/A'})`)
  }},

  // user stats
  { path: '/users/:user_id/stats', method: 'post', responseFormat: { type: 'json' }},

  // watchlist (sync)
  { path: '/sync/all-items/:type/:status', method: 'get', responseFormat: {
    type: 'simple',
    template: (response: any, args: any) => {
      const items = (response && response[args.type]) || [];
      if (items.length === 0) {
        return [`no ${args.type} with status ${args.status}`];
      }
      return items.slice(0, 20).map((item: any, i: number) => {
        const media = item.show || item.movie || item.anime || {};
        const title = media.title || 'unknown';
        const year = media.year || 'N/A';
        const simklId = media.ids?.simkl || 'unknown';
        const watched = item.watched_episodes_count || 0;
        const total = item.total_episodes_count || 0;
        const progress = total > 0 ? ` - ${watched}/${total} episodes` : '';
        return `${i}: [SIMKL #${simklId}] - ${title} (${year})${progress}`;
      });
    }
  }},
];
