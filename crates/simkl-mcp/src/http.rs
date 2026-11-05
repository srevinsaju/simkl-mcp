//! The axum router for `serve`: `/mcp`, the OAuth endpoints, the protected
//! resource metadata (RFC 9728), and `/health`.
//!
//! Every `/mcp` request needs a bearer token our authorization server issued
//! for the `/mcp` resource. Its subject is a vault connection, whose Simkl
//! token becomes the request's [`UserClient`](simkl_api::UserClient), which
//! the tools read (see [`crate::tools::User`]).
//!
//! Scope is enforced here, before rmcp sees the request: a `tools/call` for a
//! write tool with a token lacking `media:write` gets `403
//! insufficient_scope`, which makes the client step up. The tool name comes
//! from the JSON-RPC body, since only clients on protocol 2026-07-28 or later
//! send `Mcp-Name`.

use std::collections::HashSet;
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, Bytes, to_bytes};
use axum::extract::{Request, State};
use axum::http::header::{AUTHORIZATION, WWW_AUTHENTICATE};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::{Next, from_fn_with_state};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::get;
use oauth_as::resource_metadata::{ProtectedResourceConfig, ProtectedResourceMetadata};
use oauth_as::scope::ScopeSet;
use rmcp::transport::streamable_http_server::session::never::NeverSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use serde::Deserialize;
use serde_json::Value;

use crate::auth::{Auth, ConnectionId, READ_SCOPE, VaultError, WRITE_SCOPE};
use crate::config::Config;
use crate::server::SimklServer;
use crate::tools;

/// The `mcp` section of the server's configuration.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct McpConfig {
    /// A JSON-RPC request larger than this is refused before it is parsed.
    pub max_request_bytes: usize,
}

impl Default for McpConfig {
    fn default() -> McpConfig {
        McpConfig {
            max_request_bytes: 1 << 20,
        }
    }
}

/// What the `/mcp` middleware needs.
#[derive(Clone)]
struct Mcp {
    auth: Arc<Auth>,
    simkl: simkl_api::Client,
    resource: Arc<str>,
    /// Tools whose `readOnlyHint` is not true.
    write_tools: Arc<HashSet<String>>,
    /// Sent in every challenge, so clients can find the authorization server.
    metadata_url: Arc<str>,
    max_request_bytes: usize,
}

/// The whole server.
pub fn router(
    config: &Config,
    auth: Arc<Auth>,
    simkl: simkl_api::Client,
) -> anyhow::Result<Router> {
    let resource = config.resource();
    let mut metadata = ProtectedResourceConfig::new(resource.clone(), config.issuer());
    metadata.scopes_supported = Some(vec![READ_SCOPE.to_owned(), WRITE_SCOPE.to_owned()]);
    metadata.resource_name = Some("Simkl".to_owned());
    let metadata = ProtectedResourceMetadata::from_config(&metadata);
    let metadata_path = metadata.well_known_path();

    let server = SimklServer::new();
    let mcp = Mcp {
        auth: auth.clone(),
        simkl,
        resource: resource.into(),
        write_tools: Arc::new(tools::write_tool_names()),
        metadata_url: format!("{}{metadata_path}", config.issuer()).into(),
        max_request_bytes: config.mcp.max_request_bytes,
    };
    let host = config
        .public_url
        .host_str()
        .unwrap_or("localhost")
        .to_owned();
    let service = StreamableHttpService::new(
        move || Ok(server.clone()),
        Arc::new(NeverSessionManager::default()),
        StreamableHttpServerConfig::default()
            .with_legacy_session_mode(false)
            .with_json_response(true)
            .with_allowed_hosts([host, "localhost".to_owned(), "127.0.0.1".to_owned()]),
    );
    let mcp_routes = Router::new()
        .nest_service("/mcp", service)
        .layer(from_fn_with_state(mcp, authenticate));

    Ok(Router::new()
        .route("/health", get(|| async { "ok" }))
        .route(
            &metadata_path,
            get(move || async move { Json(metadata.clone()) }),
        )
        .merge(mcp_routes)
        .merge(auth.router()?))
}

/// Authenticates a `/mcp` request, enforces write scope, and attaches the
/// user's Simkl client.
async fn authenticate(State(mcp): State<Mcp>, request: Request, next: Next) -> Response {
    let Some(token) = bearer(&request) else {
        return mcp.unauthorized(None);
    };
    let issued = match mcp.auth.server().introspect(&token).await {
        Ok(Some(issued)) => issued,
        Ok(None) => return mcp.unauthorized(Some("invalid_token")),
        Err(error) => {
            tracing::error!(%error, "introspecting a token failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let audience_matches = issued
        .resource
        .iter()
        .any(|resource| **resource == *mcp.resource);
    let (Some(subject), true) = (issued.subject.as_deref(), audience_matches) else {
        return mcp.unauthorized(Some("invalid_token"));
    };

    let (parts, body) = request.into_parts();
    let Ok(body) = to_bytes(body, mcp.max_request_bytes).await else {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    };
    if needs_step_up(&issued.scope, &body, &mcp.write_tools) {
        return mcp.insufficient_scope();
    }

    let access_token = match mcp
        .auth
        .vault()
        .access_token(&ConnectionId::from_subject(subject))
        .await
    {
        Ok(access_token) => access_token,
        Err(VaultError::Disconnected) => return mcp.unauthorized(Some("invalid_token")),
        Err(error) => {
            tracing::error!(%error, "reading the Simkl token failed");
            return StatusCode::BAD_GATEWAY.into_response();
        }
    };
    let mut request = Request::from_parts(parts, Body::from(body));
    request
        .extensions_mut()
        .insert(mcp.simkl.user(access_token));
    next.run(request).await
}

fn bearer(request: &Request) -> Option<String> {
    let value = request.headers().get(AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    scheme
        .eq_ignore_ascii_case("bearer")
        .then(|| token.trim().to_owned())
}

/// Whether a token with `scope` must step up to `media:write` before this
/// JSON-RPC body runs.
fn needs_step_up(scope: &ScopeSet, body: &Bytes, write_tools: &HashSet<String>) -> bool {
    !scope.contains(WRITE_SCOPE) && calls_write_tool(body, write_tools)
}

/// Whether a JSON-RPC body (one message or a batch) calls a write tool.
fn calls_write_tool(body: &Bytes, write_tools: &HashSet<String>) -> bool {
    let Ok(body) = serde_json::from_slice::<Value>(body) else {
        return false;
    };
    let messages = match &body {
        Value::Array(batch) => batch.iter().collect(),
        message => vec![message],
    };
    messages.into_iter().any(|message| {
        message["method"] == "tools/call"
            && message["params"]["name"]
                .as_str()
                .is_some_and(|name| write_tools.contains(name))
    })
}

impl Mcp {
    /// `401`, pointing at the protected resource metadata (RFC 9728 §5.1).
    fn unauthorized(&self, error: Option<&str>) -> Response {
        let error = error
            .map(|error| format!(", error=\"{error}\""))
            .unwrap_or_default();
        self.challenge(
            StatusCode::UNAUTHORIZED,
            format!("Bearer resource_metadata=\"{}\"{error}", self.metadata_url),
        )
    }

    /// `403 insufficient_scope`, naming the scope to re-authorize with
    /// (RFC 6750 §3.1).
    fn insufficient_scope(&self) -> Response {
        self.challenge(
            StatusCode::FORBIDDEN,
            format!(
                "Bearer error=\"insufficient_scope\", scope=\"{READ_SCOPE} {WRITE_SCOPE}\", \
                 resource_metadata=\"{}\"",
                self.metadata_url
            ),
        )
    }

    fn challenge(&self, status: StatusCode, challenge: String) -> Response {
        let mut response = status.into_response();
        response
            .headers_mut()
            .insert(WWW_AUTHENTICATE, HeaderValue::from_str(&challenge).unwrap());
        response
    }
}

#[cfg(test)]
#[path = "http_test.rs"]
mod tests;
