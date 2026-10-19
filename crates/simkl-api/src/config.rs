//! [`ClientConfig`], passed to [`Client::new`](crate::Client::new), with the
//! defaults Simkl documents.

use std::num::NonZeroU32;
use std::time::Duration;

use url::Url;

/// Who is calling Simkl. Every request carries these.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub client_id: String,
    pub app_name: String,
    pub app_version: String,
    pub api_url: Url,
    pub data_url: Url,
    pub limits: Limits,
    pub retries: Retries,
}

impl ClientConfig {
    pub fn new(
        client_id: impl Into<String>,
        app_name: impl Into<String>,
        app_version: impl Into<String>,
    ) -> Self {
        Self {
            client_id: client_id.into(),
            app_name: app_name.into(),
            app_version: app_version.into(),
            api_url: Url::parse("https://api.simkl.com").unwrap(),
            data_url: Url::parse("https://data.simkl.in").unwrap(),
            limits: Limits::default(),
            retries: Retries::default(),
        }
    }
}

/// Per-user rate limits, and how long their state is kept.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub reads_per_second: NonZeroU32,
    pub writes_per_second: NonZeroU32,
    /// Forget a user's write lock once they have been idle this long.
    pub idle_user_ttl: Duration,
    /// Drop stale rate-limit state once this many users are tracked.
    pub prune_at_users: usize,
}

impl Default for Limits {
    /// Simkl's documented caps: 10 GET/s and 1 POST/s
    /// ([rate limits](https://api.simkl.org/resources/rate-limits)).
    fn default() -> Self {
        Self {
            reads_per_second: NonZeroU32::new(10).unwrap(),
            writes_per_second: NonZeroU32::new(1).unwrap(),
            idle_user_ttl: Duration::from_secs(10 * 60),
            prune_at_users: 10_000,
        }
    }
}

/// How often and how long to wait before retrying. Simkl asks clients not to
/// back off exponentially for either.
#[derive(Debug, Clone, Copy)]
pub struct Retries {
    pub attempts: usize,
    /// After a per-second rate limit (`429 rate_limit`).
    pub burst_delay: Duration,
    /// After the per-user write lock (`400 rate_limit`).
    pub write_lock_delay: Duration,
}

impl Default for Retries {
    fn default() -> Self {
        Self {
            attempts: 3,
            burst_delay: Duration::from_millis(1100),
            write_lock_delay: Duration::from_secs(3),
        }
    }
}
