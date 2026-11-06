//! MCP Apps views (`io.modelcontextprotocol/ui`): `ui://simkl/titles`,
//! `ui://simkl/title` and `ui://simkl/stats`, served as `text/html;profile=mcp-app` from the bundle
//! built in `ui/`.
//!
//! A tool shows a view by carrying [`View::tool_meta`]. Hosts without MCP Apps
//! ignore it, so every tool's text content must stand on its own.

use rmcp::model::{MetaObject, ReadResourceResult, Resource, ResourceContents};
use serde_json::json;

/// The MIME type MCP Apps hosts render
/// ([spec](https://github.com/modelcontextprotocol/ext-apps/blob/main/specification/draft/apps.mdx)).
const MIME_TYPE: &str = "text/html;profile=mcp-app";

/// Hosts a view may load images from: posters, through Simkl's image proxy.
const RESOURCE_DOMAINS: &[&str] = &["https://wsrv.nl"];

#[derive(Debug, Clone, Copy)]
pub enum View {
    /// A poster grid, for search, trending, calendar and library lists.
    Titles,
    /// One title with its episodes.
    Title,
    /// Watching stats by kind and status.
    Stats,
}

impl View {
    const ALL: [View; 3] = [View::Titles, View::Title, View::Stats];

    fn uri(self) -> &'static str {
        match self {
            View::Titles => "ui://simkl/titles",
            View::Title => "ui://simkl/title",
            View::Stats => "ui://simkl/stats",
        }
    }

    fn name(self) -> &'static str {
        match self {
            View::Titles => "titles",
            View::Title => "title",
            View::Stats => "stats",
        }
    }

    /// The single-file bundle `ui/` builds; see `ui/vite.config.ts`.
    fn html(self) -> &'static str {
        match self {
            View::Titles => include_str!("../../../ui/dist/titles.html"),
            View::Title => include_str!("../../../ui/dist/title.html"),
            View::Stats => include_str!("../../../ui/dist/stats.html"),
        }
    }

    /// The tool `_meta` that shows this view with the tool's result.
    pub fn tool_meta(self) -> MetaObject {
        meta(json!({ "resourceUri": self.uri() }))
    }

    /// Every view, for `resources/list`.
    pub fn resources() -> Vec<Resource> {
        View::ALL
            .into_iter()
            .map(|view| {
                Resource::new(view.uri(), view.name())
                    .with_mime_type(MIME_TYPE)
                    .with_meta(resource_meta())
            })
            .collect()
    }

    /// The view at `uri`, for `resources/read`.
    pub fn read(uri: &str) -> Option<ReadResourceResult> {
        let view = View::ALL.into_iter().find(|view| view.uri() == uri)?;
        let contents = ResourceContents::text(view.html(), view.uri())
            .with_mime_type(MIME_TYPE)
            .with_meta(resource_meta());
        Some(ReadResourceResult::new(vec![contents]))
    }
}

fn resource_meta() -> MetaObject {
    meta(json!({ "csp": { "resourceDomains": RESOURCE_DOMAINS } }))
}

/// `{"ui": ui}`: MCP Apps keeps everything under the `ui` key.
fn meta(ui: serde_json::Value) -> MetaObject {
    let mut object = serde_json::Map::new();
    object.insert("ui".to_owned(), ui);
    MetaObject(object)
}

#[cfg(test)]
#[path = "views_test.rs"]
mod tests;
