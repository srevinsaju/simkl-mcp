import { McpServer, ResourceTemplate } from '@modelcontextprotocol/sdk/server/mcp.js';
import { McpAgent } from 'agents/mcp';
import { z } from 'zod';
import { SimklClient } from './api/client.js';
import { registerTools } from '../generated/tools.js';

interface Env {
  SIMKL_CLIENT_ID: string;
  SIMKL_CLIENT_SECRET: string;
  OAUTH_REDIRECT_URI: string;
  SIMKL_API_BASE_URL: string;
}

interface SimklAuthProps extends Record<string, unknown> {
  simklToken: string;
  simklUserId?: string;
}

export class SimklMCP extends McpAgent<Env, unknown, SimklAuthProps> {
  server = new McpServer({
    name: 'simkl-mcp-server',
    version: '1.0.0',
  });

  private client!: SimklClient;
  private get simklToken(): string | undefined {
    return this.props?.simklToken;
  }

  async init() {
    this.client = new SimklClient({
      baseUrl: this.env?.SIMKL_API_BASE_URL || 'https://api.simkl.com',
      clientId: this.env?.SIMKL_CLIENT_ID || '',
    });

    registerTools(this.server, this.client, () => this.simklToken);
    this.registerCustomTools();
    this.registerResources();
  }

  private registerCustomTools() {
    // simkl_my_stats - get stats for current user
    this.server.registerTool(
      'simkl_my_stats',
      {
        description: 'Get watching statistics for the current authenticated user',
        inputSchema: z.object({}),
      },
      async () => {
        // get current user id from settings
        const settings = await this.client.request<any>('/users/settings', {
          method: 'POST',
          token: this.simklToken,
        });

        const userId = settings?.account?.id;
        if (!userId) {
          return {
            content: [{ type: 'text', text: 'failed to get current user id' }],
          };
        }

        // get stats for current user
        const stats = await this.client.request<any>(`/users/${userId}/stats`, {
          method: 'POST',
          token: this.simklToken,
        });

        return {
          content: [{ type: 'text', text: JSON.stringify(stats, null, 2) }],
        };
      }
    );
  }

  private registerResources() {
    const watchlistTemplate = new ResourceTemplate(
      'simkl://watchlist/{type}/{status}',
      {
        list: async () => {
          const types = ['shows', 'movies', 'anime'];
          const statuses = ['watching', 'plantowatch', 'hold', 'completed', 'dropped'];

          return {
            resources: types.flatMap(type =>
              statuses.map(status => ({
                uri: `simkl://watchlist/${type}/${status}`,
                name: `${type} ${status}`,
                description: `${status} ${type}`,
                mimeType: 'application/json' as const,
              }))
            )
          };
        },
        complete: {
          type: async () => ['shows', 'movies', 'anime'],
          status: async () => ['watching', 'plantowatch', 'hold', 'completed', 'dropped'],
        },
      }
    );

    this.server.registerResource(
      'watchlist',
      watchlistTemplate,
      {
        mimeType: 'text/plain',
        description: 'user watchlist by type and status',
      },
      async (uri, variables) => {
        const { type, status } = variables;
        const typeKey = type as string;
        const fetchPage = async (page: number) =>
          this.client.requestPaginated<any>(`/sync/all-items/${typeKey}/${status}`, {
            method: 'GET',
            query: { extended: 'full' },
            token: this.simklToken,
            pagination: { page, limit: 100 },
          });

        const firstPage = await fetchPage(1);
        const items = ((firstPage.data && firstPage.data[typeKey]) || []) as any[];
        const pagination = firstPage.pagination;

        if (pagination && pagination.pageCount > 1) {
          for (let page = 2; page <= pagination.pageCount; page++) {
            const pageData = await fetchPage(page);
            const pageItems = ((pageData.data && pageData.data[typeKey]) || []) as any[];
            items.push(...pageItems);
          }
        }

        const baseUri = uri.toString();

        // return empty content if no items
        if (items.length === 0) {
          return {
            contents: [{
              uri: baseUri,
              mimeType: 'text/plain',
              text: `no ${type} with status ${status}`,
            }],
          };
        }

        return {
          contents: items.map((item: any, index: number) => {
            const show = item.show || item.movie || item.anime || {};
            const simklId = show.ids?.simkl || 'unknown';
            const title = show.title || 'unknown';
            const year = show.year || 'N/A';
            const addedDate = item.added_to_watchlist_at?.split('T')[0] || 'unknown';
            const text = `${index}: [SIMKL #${simklId}] - ${title} (${year}) - added on ${addedDate}`;

            return {
              uri: `${baseUri}#${index}`,
              mimeType: 'text/plain',
              text,
            };
          }),
        };
      }
    );

    const trendingTemplate = new ResourceTemplate(
      'simkl://trending/{type}/{interval}',
      {
        list: async () => {
          const types = ['tv', 'movies', 'anime'];
          const intervals = ['daily', 'weekly', 'monthly'];

          return {
            resources: types.flatMap(type =>
              intervals.map(interval => ({
                uri: `simkl://trending/${type}/${interval}`,
                name: `trending ${type} (${interval})`,
                description: `${interval} trending ${type}`,
                mimeType: 'application/json' as const,
              }))
            )
          };
        },
        complete: {
          type: async () => ['tv', 'movies', 'anime'],
          interval: async () => ['daily', 'weekly', 'monthly'],
        },
      }
    );

    this.server.registerResource(
      'trending',
      trendingTemplate,
      {
        mimeType: 'text/plain',
        description: 'trending content by type and interval',
      },
      async (uri, variables) => {
        const { type, interval } = variables;
        const response = await this.client.request<any[]>(`/${type}/trending/${interval}`, {
          method: 'GET',
          query: { extended: 'full' },
        });

        const items = (response || []) as any[];
        const baseUri = uri.toString();

        // return empty content if no items
        if (items.length === 0) {
          return {
            contents: [{
              uri: baseUri,
              mimeType: 'text/plain',
              text: `no trending ${type} for ${interval}`,
            }],
          };
        }

        return {
          contents: items.map((item: any, index: number) => {
            const simklId = item.ids?.simkl || item.ids?.simkl_id || 'unknown';
            const title = item.title || 'unknown';
            const year = item.year || 'N/A';
            const rating = item.ratings?.simkl?.rating || 'N/A';
            const text = `${index}: [SIMKL #${simklId}] - ${title} (${year}) - rating: ${rating}`;

            return {
              uri: `${baseUri}#${index}`,
              mimeType: 'text/plain',
              text,
            };
          }),
        };
      }
    );
  }
}
