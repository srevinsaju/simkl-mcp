//! The OAuth 2.1 authorization server MCP clients talk to, sitting in front of
//! Simkl's AUTH V2.
//!
//! The protocol is `oauth-as`: authorization code with PKCE, refresh rotation
//! with reuse detection, RFC 8707 audiences, metadata, Dynamic Client
//! Registration and Client ID Metadata Documents. This module supplies only
//! what that library leaves to its host: storage, signing in the user (via
//! Simkl), and keeping each grant's Simkl tokens.

mod clients;
mod login;
mod store;
mod upstream;
mod vault;
