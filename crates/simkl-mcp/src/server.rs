//! The rmcp `ServerHandler`: server identity, the tool router, and the
//! `ui://` views as resources.

use rmcp::handler::server::tool::ToolRouter;
use rmcp::model::{
    Implementation, ListResourcesResult, PaginatedRequestParams, ReadResourceRequestParams,
    ReadResourceResponse, ServerCapabilities, ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, tool_handler};

use crate::tools;
use crate::views::View;

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
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
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

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        Ok(ListResourcesResult::with_all_items(View::resources()))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        View::read(&request.uri)
            .map(ReadResourceResponse::from)
            .ok_or_else(|| {
                ErrorData::resource_not_found(format!("no view at {}", request.uri), None)
            })
    }
}
