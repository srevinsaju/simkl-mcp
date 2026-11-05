//! MCP tools, one per user intent rather than one per Simkl endpoint.
//!
//! Every tool declares an `outputSchema`, returns `structuredContent` plus a
//! compact text rendering, and sets read-only/destructive/idempotent hints.
//! rmcp lists tools sorted by name, which keeps `tools/list` in a fixed order.
//!
//! A write tool is one whose `readOnlyHint` is false; that is what `serve`
//! checks before requiring `media:write`, so there is no second list.

mod catalog;
mod library;
mod title;
mod write;

use axum::http::request::Parts;
use rmcp::ErrorData;
use rmcp::handler::server::common::{AsRequestContext, FromContextPart};
use rmcp::handler::server::tool::ToolRouter;
use rmcp::model::{CallToolResult, ContentBlock, IntoContents};
use schemars::JsonSchema;
use serde::Serialize;
use simkl_api::UserClient;
use simkl_api::media::Kind;

use crate::server::SimklServer;

/// Every tool, merged from each module's router.
pub fn router() -> ToolRouter<SimklServer> {
    SimklServer::catalog_router() + SimklServer::library_router()
}

/// The signed-in user making this request. `serve` authenticates the bearer
/// token and puts their [`UserClient`] in the HTTP request's extensions before
/// rmcp sees the request.
pub struct User(pub UserClient);

impl<C: AsRequestContext> FromContextPart<C> for User {
    fn from_context_part(context: &mut C) -> Result<Self, ErrorData> {
        context
            .as_request_context()
            .extensions
            .get::<Parts>()
            .and_then(|parts| parts.extensions.get::<UserClient>())
            .cloned()
            .map(User)
            .ok_or_else(|| ErrorData::internal_error("request has no signed-in Simkl user", None))
    }
}

/// What a tool returns: structured content matching its `outputSchema`, and a
/// text rendering that stands on its own for hosts without MCP Apps.
trait Output: Serialize + JsonSchema {
    fn summary(&self) -> String;
}

fn reply(output: impl Output) -> CallToolResult {
    let summary = output.summary();
    let mut result = CallToolResult::structured(serde_json::to_value(output).unwrap());
    result.content = vec![ContentBlock::text(summary)];
    result
}

/// Why a tool call failed. Returned as an `isError` result, so the model sees
/// the message and can correct its call.
#[derive(Debug, thiserror::Error)]
enum ToolError {
    #[error(transparent)]
    Simkl(#[from] simkl_api::Error),
    #[error("invalid input: {0}")]
    Invalid(#[from] garde::Report),
    #[error("{0}")]
    Input(String),
    #[error("Simkl has no {kind} with these IDs")]
    NotFound { kind: Kind },
    /// Every title shown must link to simkl.com, which needs its Simkl ID.
    #[error("Simkl sent {title:?} without a Simkl ID")]
    NoSimklId { title: String },
}

impl IntoContents for ToolError {
    fn into_contents(self) -> Vec<ContentBlock> {
        vec![ContentBlock::text(self.to_string())]
    }
}

type ToolResult = Result<CallToolResult, ToolError>;

/// Where a page sits in a list cut into pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
struct Page {
    /// From 1.
    page: u32,
    page_count: u32,
    /// Items across all pages.
    total: u32,
}

/// Page `page` (from 1) of `items`, `per_page` at a time. A page past the end
/// is empty.
fn paginate<T>(items: Vec<T>, page: u32, per_page: u32) -> (Vec<T>, Page) {
    assert!(page >= 1 && per_page >= 1, "validated by the tool input");
    let total = items.len() as u32;
    let skip = (page - 1).saturating_mul(per_page) as usize;
    let items = items
        .into_iter()
        .skip(skip)
        .take(per_page as usize)
        .collect();
    let page_count = total.div_ceil(per_page).max(1);
    (
        items,
        Page {
            page,
            page_count,
            total,
        },
    )
}
