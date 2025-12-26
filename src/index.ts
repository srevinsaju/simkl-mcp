import { OAuthProvider } from '@cloudflare/workers-oauth-provider';
import { SimklMCP } from './mcp-agent.js';
import authHandler from './auth-handler.js';

export { SimklMCP };

export default new OAuthProvider({
  apiHandlers: {
    '/sse': SimklMCP.serveSSE('/sse'),
    '/mcp': SimklMCP.serve('/mcp'),
  },

  defaultHandler: authHandler,

  authorizeEndpoint: '/auth/simkl',
  tokenEndpoint: '/token',
  clientRegistrationEndpoint: '/register',
});
