//! MCP tools, one per user intent rather than one per Simkl endpoint.
//!
//! Every tool declares an `outputSchema`, returns `structuredContent` plus a
//! compact text rendering, and sets read-only/destructive/idempotent hints.
//! Tools are listed in a fixed order.

mod catalog;
mod library;
mod write;
