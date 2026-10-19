//! The HTTP client: app identification, bearer tokens, rate limiting and
//! mapping Simkl responses into [`crate::error::Error`].
//!
//! Public catalog calls (titles by Simkl ID, episode lists) go out without an
//! `Authorization` header; everything else requires a user access token.

#[cfg(test)]
#[path = "client_test.rs"]
mod tests;
