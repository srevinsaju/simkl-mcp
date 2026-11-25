//! The page at `/`: what this server is and the URL to add to Claude, with
//! the head tags link previews read.
//!
//! The icon is Simkl's own mark, from simkl.com's `safari-pinned-tab.svg`,
//! rendered from `assets/icon.svg`. After changing it, render it again from
//! the dev shell:
//!
//! ```sh
//! resvg -w 512 -h 512 assets/icon.svg assets/icon.png
//! ```
//!
//! The link-preview image is the README's title screenshot, embedded from
//! `docs/screenshots`, so the two never drift apart.

use askama::Template;
use askama_web::WebTemplate;
use axum::Router;
use axum::http::header::CONTENT_TYPE;
use axum::routing::get;

use crate::config::Config;

/// One line on what this is, for the page and its link previews.
const DESCRIPTION: &str = "Your Simkl watchlist, history and ratings, in Claude.";

const ICON: &[u8] = include_bytes!("../assets/icon.png");
const OG_IMAGE: &[u8] = include_bytes!("../../../docs/screenshots/title.png");

/// `templates/landing.html`.
#[derive(Template, WebTemplate, Clone)]
#[template(path = "landing.html")]
struct LandingPage {
    /// The public origin, such as `https://simkl.xwx.cat`.
    origin: String,
    description: &'static str,
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
    };
    Router::new()
        .route("/", get(move || async move { page.clone() }))
        .route("/robots.txt", get(|| async { ROBOTS }))
        .route(
            "/icon.png",
            get(|| async { ([(CONTENT_TYPE, "image/png")], ICON) }),
        )
        .route(
            "/og.png",
            get(|| async { ([(CONTENT_TYPE, "image/png")], OG_IMAGE) }),
        )
}

#[cfg(test)]
#[path = "landing_test.rs"]
mod tests;
