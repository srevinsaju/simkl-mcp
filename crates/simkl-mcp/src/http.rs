//! The axum router for `serve`: `/mcp`, the OAuth endpoints, well-known
//! metadata, and `/health`.
//!
//! Scope is enforced here, before rmcp sees the request: a `tools/call` for a
//! write tool (known from the `Mcp-Name` header) with a token lacking
//! `media:write` gets `403 insufficient_scope`, which makes the client step up.
