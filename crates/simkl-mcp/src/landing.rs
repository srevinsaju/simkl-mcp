//! The page at `/`: what this server is, the URL to add to Claude, its tools
//! and how to run your own, with the head tags link previews read.
//!
//! The icon and the link-preview image are PNGs rendered from the SVGs next
//! to them in `assets/`. The icon is a placeholder. After changing an SVG,
//! render it again from the dev shell:
//!
//! ```sh
//! resvg --use-fonts-dir "$PLEX_FONTS" --use-fonts-dir "$FIRA_FONTS" assets/og.svg assets/og.png
//! ```

use askama::Template;
use askama_web::WebTemplate;
use axum::Router;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::routing::get;

use crate::config::Config;
use crate::tools;

/// One line on what this is, for the page and its link previews.
const DESCRIPTION: &str = "Your Simkl watchlist, history and ratings, in Claude.";

const ICON: &[u8] = include_bytes!("../assets/icon.png");
const OG_IMAGE: &[u8] = include_bytes!("../assets/og.png");
/// A day: long enough for link previews, short enough for the placeholder
/// icon to be replaced.
const IMAGE_CACHE: &str = "public, max-age=86400";

/// `templates/landing.html`.
#[derive(Template, WebTemplate, Clone)]
#[template(path = "landing.html")]
struct LandingPage {
    /// The public origin, such as `https://simkl.xwx.cat`.
    origin: String,
    description: &'static str,
    tools: Vec<ToolRow>,
}

/// One tool on the page.
#[derive(Clone)]
struct ToolRow {
    name: String,
    /// The first sentence of the tool's description.
    description: String,
    /// Whether calling it needs `media:write`.
    write: bool,
}

/// Crawlers may index the landing page and the images its link previews
/// use, and nothing else: `/authorize`, `/mcp` and the OAuth endpoints are
/// for MCP clients, not search results.
const ROBOTS: &str = "User-agent: *\nAllow: /$\nAllow: /icon.png\nAllow: /og.png\nDisallow: /\n";

/// `/`, `/robots.txt`, `/icon.png` and `/og.png`.
pub fn router(config: &Config) -> Router {
    let page = LandingPage {
        origin: config.issuer().to_owned(),
        description: DESCRIPTION,
        tools: tool_rows(),
    };
    Router::new()
        .route("/", get(move || async move { page.clone() }))
        .route("/robots.txt", get(|| async { ROBOTS }))
        .route(
            "/icon.png",
            get(|| async {
                (
                    [(CONTENT_TYPE, "image/png"), (CACHE_CONTROL, IMAGE_CACHE)],
                    ICON,
                )
            }),
        )
        .route(
            "/og.png",
            get(|| async {
                (
                    [(CONTENT_TYPE, "image/png"), (CACHE_CONTROL, IMAGE_CACHE)],
                    OG_IMAGE,
                )
            }),
        )
}

/// Every tool, in name order.
fn tool_rows() -> Vec<ToolRow> {
    let write_tools = tools::write_tool_names();
    let mut rows: Vec<ToolRow> = tools::router()
        .list_all()
        .into_iter()
        .map(|tool| ToolRow {
            write: write_tools.contains(tool.name.as_ref()),
            description: first_sentence(tool.description.as_deref().unwrap_or_default()),
            name: tool.name.into_owned(),
        })
        .collect();
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    rows
}

/// The first sentence of a description, unwrapped: descriptions are written
/// for the model, and the page only needs what each tool is for.
fn first_sentence(description: &str) -> String {
    let text = description.split_whitespace().collect::<Vec<_>>().join(" ");
    match text.find(". ") {
        Some(end) => text[..=end].to_owned(),
        None => text,
    }
}

#[cfg(test)]
#[path = "landing_test.rs"]
mod tests;
