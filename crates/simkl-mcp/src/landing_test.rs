use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::header::CONTENT_TYPE;
use axum::http::{Request, StatusCode};
use rstest::rstest;
use serde_json::json;
use tower::ServiceExt;

use crate::auth::Auth;
use crate::config::Config;
use crate::http;

/// The whole server, as `serve` builds it, over an in-memory database: the
/// landing routes must merge with `/mcp` and the OAuth routes without a
/// clash, which axum only reports when the router is built.
async fn app() -> Router {
    let config: Config = serde_json::from_value(json!({
        "public_url": "https://simkl.example",
        "database": "sqlite::memory:",
        "client": {"client_id": "test", "app_name": "simkl-mcp", "app_version": "0.0.0"},
    }))
    .unwrap();
    let auth = Auth::open(
        &config.public_url,
        &config.resource(),
        &config.database,
        "test".to_owned(),
        "secret".to_owned().into(),
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
            .parse()
            .unwrap(),
        config.auth,
    )
    .await
    .unwrap();
    let simkl = simkl_api::Client::new(config.client.clone()).unwrap();
    http::router(&config, Arc::new(auth), simkl).unwrap()
}

async fn get(path: &str) -> (StatusCode, String, Vec<u8>) {
    let request = Request::get(path).body(Body::empty()).unwrap();
    let response = app().await.oneshot(request).await.unwrap();
    let status = response.status();
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .map(|value| value.to_str().unwrap().to_owned())
        .unwrap_or_default();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, content_type, body.to_vec())
}

/// The tags xwx.cat's card build reads, and the URL users copy.
#[tokio::test]
async fn landing_page_has_card_tags_and_mcp_url() {
    let (status, content_type, body) = get("/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(content_type.starts_with("text/html"), "{content_type}");
    let html = String::from_utf8(body).unwrap();
    for needle in [
        "<title>Simkl MCP",
        r#"<link rel="icon" type="image/png" href="/icon.png">"#,
        r#"<meta property="og:title""#,
        r#"<meta property="og:description""#,
        r#"<meta property="og:image" content="https://simkl.example/og.png">"#,
        r#"<meta property="og:url" content="https://simkl.example/">"#,
        "https://simkl.example/mcp</code>",
    ] {
        assert!(html.contains(needle), "landing page lacks {needle}");
    }
}

#[rstest]
#[case::icon("/icon.png")]
#[case::link_preview("/og.png")]
#[tokio::test]
async fn serves_png(#[case] path: &str) {
    let (status, content_type, body) = get(path).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type, "image/png");
    assert!(body.starts_with(b"\x89PNG\r\n\x1a\n"));
}

#[tokio::test]
async fn robots_hides_everything_but_the_landing_page() {
    let (status, _, body) = get("/robots.txt").await;
    assert_eq!(status, StatusCode::OK);
    let robots = String::from_utf8(body).unwrap();
    assert!(robots.contains("Allow: /$\n"));
    assert!(robots.contains("Disallow: /\n"));
}

/// `/mcp` still answers with its bearer challenge, not the landing page.
#[tokio::test]
async fn mcp_still_needs_a_token() {
    let (status, _, _) = get("/mcp").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
