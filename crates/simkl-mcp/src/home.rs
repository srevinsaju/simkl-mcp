//! The homepage at `/`: what simkl-mcp is and how to connect, with the
//! README's screenshots and the tags search engines and link previews read.
//! The screenshots are embedded from `docs/screenshots`, so they are the same
//! files the README shows.

use askama::Template;
use askama_web::WebTemplate;
use axum::Router;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::response::IntoResponse;
use axum::routing::get;

use crate::config::Config;

const LIBRARY_SCREENSHOT: &[u8] = include_bytes!("../../../docs/screenshots/library.webp");
const TITLE_SCREENSHOT: &[u8] = include_bytes!("../../../docs/screenshots/title.png");

/// `templates/home.html`.
#[derive(Clone, Template, WebTemplate)]
#[template(path = "home.html")]
struct HomePage {
    /// The public URL without a trailing slash: the canonical page address.
    base: String,
    /// What users add as a connector.
    connector_url: String,
}

/// `/`, `/robots.txt` and the screenshots.
pub fn router(config: &Config) -> Router {
    let page = HomePage {
        base: config.issuer().to_owned(),
        connector_url: config.resource(),
    };
    let robots = format!(
        "User-agent: *\nAllow: /$\nAllow: /screenshots/\nDisallow: /\n\nSitemap: {}/sitemap.xml\n",
        page.base
    );
    let sitemap = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\
         <url><loc>{}/</loc></url></urlset>\n",
        page.base
    );
    Router::new()
        .route("/", get(move || async move { page.clone() }))
        .route("/robots.txt", get(move || async move { robots.clone() }))
        .route(
            "/sitemap.xml",
            get(move || async move { ([(CONTENT_TYPE, "application/xml")], sitemap.clone()) }),
        )
        .route(
            "/screenshots/library.webp",
            get(|| async { image("image/webp", LIBRARY_SCREENSHOT) }),
        )
        .route(
            "/screenshots/title.png",
            get(|| async { image("image/png", TITLE_SCREENSHOT) }),
        )
}

fn image(content_type: &'static str, bytes: &'static [u8]) -> impl IntoResponse {
    (
        [
            (CONTENT_TYPE, content_type),
            (CACHE_CONTROL, "public, max-age=86400"),
        ],
        bytes,
    )
}
