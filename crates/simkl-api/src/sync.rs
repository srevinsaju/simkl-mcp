//! Per-user library snapshots kept fresh with `/sync/activities`: only the
//! buckets whose timestamps moved are re-read from `/sync/all-items`.
//!
//! This is a cache, not durable state; losing it costs one full read.

#[cfg(test)]
#[path = "sync_test.rs"]
mod tests;
