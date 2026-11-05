//! [`ClientConfig`], passed to [`Client::new`](crate::Client::new), with the
//! defaults Simkl documents. It deserializes too, so a host can load it from
//! its own configuration: only `client_id`, `app_name` and `app_version` are
//! required, and durations take jiff's friendly form, such as `1100ms`.

use std::num::NonZeroU32;
use std::time::Duration;

use serde::Deserialize;
use url::Url;

/// Who is calling Simkl. Every request carries these.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientConfig {
    pub client_id: String,
    pub app_name: String,
    pub app_version: String,
    #[serde(default = "default_api_url")]
    pub api_url: Url,
    #[serde(default = "default_data_url")]
    pub data_url: Url,
    #[serde(default)]
    pub limits: Limits,
    #[serde(default)]
    pub retries: Retries,
}

fn default_api_url() -> Url {
    Url::parse("https://api.simkl.com").unwrap()
}

fn default_data_url() -> Url {
    Url::parse("https://data.simkl.in").unwrap()
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
            api_url: default_api_url(),
            data_url: default_data_url(),
            limits: Limits::default(),
            retries: Retries::default(),
        }
    }
}

/// Per-user rate limits, and how long their state is kept.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    pub reads_per_second: NonZeroU32,
    pub writes_per_second: NonZeroU32,
    /// Forget a user's rate-limit state and write lock once they have been
    /// idle this long.
    #[serde(with = "jiff::fmt::serde::unsigned_duration::required")]
    pub idle_user_ttl: Duration,
}

impl Default for Limits {
    /// Simkl's documented caps: 10 GET/s and 1 POST/s
    /// ([rate limits](https://api.simkl.org/resources/rate-limits)).
    fn default() -> Self {
        Self {
            reads_per_second: NonZeroU32::new(10).unwrap(),
            writes_per_second: NonZeroU32::new(1).unwrap(),
            idle_user_ttl: Duration::from_secs(10 * 60),
        }
    }
}

/// How often and how long to wait before retrying. Simkl asks clients not to
/// back off exponentially for either.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Retries {
    pub attempts: usize,
    /// After a per-second rate limit (`429 rate_limit`).
    #[serde(with = "jiff::fmt::serde::unsigned_duration::required")]
    pub burst_delay: Duration,
    /// After the per-user write lock (`400 rate_limit`).
    #[serde(with = "jiff::fmt::serde::unsigned_duration::required")]
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
