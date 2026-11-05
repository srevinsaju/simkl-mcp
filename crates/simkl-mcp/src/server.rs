//! The rmcp `ServerHandler`: server identity and the tool router. The `ui://`
//! resources from [`crate::views`] join it later.

use rmcp::handler::server::tool::ToolRouter;
use rmcp::model::{Implementation, ServerCapabilities, ServerConfig};
use rmcp::{ServerHandler, tool_handler};

use crate::tools;

/// The MCP server. Stateless: `serve` builds one per request, cloning a
/// router built once at startup. The signed-in user travels with each request
/// (see [`tools::User`]), never with the server.
#[derive(Clone)]
pub struct SimklServer {
    tool_router: ToolRouter<Self>,
}

impl SimklServer {
    pub fn new() -> Self {
        Self {
            tool_router: tools::router(),
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for SimklServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "Track movies, TV shows and anime on Simkl. Link every title you \
                 mention to its simkl.com `url`. Tools that take a `title` want \
                 its `kind` and `simkl_id` (or an external ID such as IMDb); \
                 search first when you only have a name.",
            )
    }
}
