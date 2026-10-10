import { describe, expect, test } from 'bun:test';
import { registerTools } from '../generated/tools';

// Goes through the generated tool handlers, because the interval values a tool
// accepts come from the Simkl spec and must be ones the URL helper understands.
describe('trending tools', () => {
  const tools = [
    ['simkl_get_trending_shows', 'tv'],
    ['simkl_get_trending_movies', 'movies'],
    ['simkl_get_trending_anime', 'anime'],
  ] as const;

  test.each(tools.flatMap(([name, type]) => ['today', 'week', 'month'].map(interval => [name, type, interval] as const)))(
    '%s requests the %s %s file',
    async (name, type, interval) => {
      const handlers: Record<string, (args: unknown) => Promise<unknown>> = {};
      const server = { registerTool: (toolName: string, _config: unknown, handler: (args: unknown) => Promise<unknown>) => { handlers[toolName] = handler; } };
      const requested: string[] = [];
      const client = { request: async (endpoint: string) => { requested.push(endpoint); return []; } };

      registerTools(server as never, client, () => 'token');
      await handlers[name]({ interval });

      expect(requested).toEqual([`https://data.simkl.in/discover/trending/${type}/${interval}_100.json`]);
    },
  );
});
