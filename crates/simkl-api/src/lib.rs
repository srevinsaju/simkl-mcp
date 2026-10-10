//! Typed client for the [Simkl API](https://api.simkl.org), AUTH V2 only.
//!
//! This crate knows nothing about MCP. It owns Simkl's request rules so that
//! callers cannot get them wrong:
//!
//! - every request carries `client_id`, `app-name`, `app-version` and a
//!   `User-Agent` ([headers](https://api.simkl.org/conventions/headers));
//! - per-user rate limits of 10 GET/s and 1 POST/s, with writes serialised per
//!   user and parallelism only on edge-cached endpoints
//!   ([rate limits](https://api.simkl.org/resources/rate-limits));
//! - library reads gated on `/sync/activities`, never polling `/sync/all-items`
//!   ([sync](https://api.simkl.org/guides/sync));
//! - `200` responses carrying an `error` key surface as errors.

#[macro_use]
mod macros;

pub mod account;
mod client;
pub mod data;
mod error;
pub mod library;
pub mod media;
mod sync;

#[cfg(test)]
mod mock_simkl;
