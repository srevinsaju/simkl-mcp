//! `oauth_as::Storage` over SQLite, checked with the library's storage
//! conformance harness. `take_*` and `claim_replay_id` must be atomic, and
//! expired records are swept on a timer.
