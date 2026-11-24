//! The page at `/`: what this server is and the URL to add to Claude, with
//! the head tags link previews read.
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
use axum::http::header::CONTENT_TYPE;
use axum::routing::get;

use crate::config::Config;

/// One line on what this is, for the page and its link previews.
const DESCRIPTION: &str = "Your Simkl watchlist, history and ratings, in Claude.";

const ICON: &[u8] = include_bytes!("../assets/icon.png");
const OG_IMAGE: &[u8] = include_bytes!("../assets/og.png");

/// `templates/landing.html`.
#[derive(Template, WebTemplate, Clone)]
#[template(path = "landing.html")]
struct LandingPage {
    /// The public origin, such as `https://simkl.xwx.cat`.
    origin: String,
    description: &'static str,
}

/// `/`, `/icon.png` and `/og.png`.
pub fn router(config: &Config) -> Router {
    let page = LandingPage {
        origin: config.issuer().to_owned(),
        description: DESCRIPTION,
    };
    Router::new()
        .route("/", get(move || async move { page.clone() }))
        .route(
            "/icon.png",
            get(|| async { ([(CONTENT_TYPE, "image/png")], ICON) }),
        )
        .route(
            "/og.png",
            get(|| async { ([(CONTENT_TYPE, "image/png")], OG_IMAGE) }),
        )
}
