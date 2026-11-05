//! Error type covering transport failures, Simkl status codes
//! ([errors](https://api.simkl.org/conventions/errors)), and errors Simkl
//! reports inside a `200` body such as `premium_only`.

use std::time::Duration;

use reqwest::StatusCode;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The request never got an HTTP response.
    #[error("request to Simkl failed: {0}")]
    Transport(#[from] reqwest::Error),

    /// The token is missing, expired or revoked (`401`). `code` is Simkl's
    /// reason, e.g. `invalid_token` or `user_token_required`.
    #[error("Simkl rejected the access token ({})", .code.as_deref().unwrap_or("no reason given"))]
    Unauthorized { code: Option<String> },

    /// The token lacks a scope this request needs (`403 insufficient_scope`).
    #[error("the Simkl token does not allow this; it needs write access")]
    InsufficientScope,

    /// Too many requests per second, still failing after retrying.
    #[error("rate limited by Simkl")]
    RateLimited,

    /// The user's daily request quota is spent. Retrying before
    /// `retry_after` will not help.
    #[error("daily Simkl request quota exceeded")]
    QuotaExceeded { retry_after: Option<Duration> },

    /// Another write for this user was still in progress, even after
    /// retrying.
    #[error("another Simkl sync is in progress for this user")]
    WriteLocked,

    /// Any other error status, or a `200` whose body carries an `error` key.
    #[error("Simkl returned {status}: {}", .error.as_deref().unwrap_or("no error code"))]
    Api {
        status: StatusCode,
        error: Option<String>,
        message: Option<String>,
    },

    /// The request was refused before sending: Simkl would reject, ignore
    /// or silently rewrite it.
    #[error("invalid request: {0}")]
    Invalid(#[from] garde::Report),

    /// The response did not have the shape we expected.
    #[error("unexpected response from Simkl: {0}")]
    Decode(#[from] serde_json::Error),
}

impl Error {
    /// The error code Simkl put in the body, if any (`premium_only`,
    /// `max_items`, ...).
    pub fn code(&self) -> Option<&str> {
        match self {
            Error::Api { error, .. } => error.as_deref(),
            Error::Unauthorized { code } => code.as_deref(),
            _ => None,
        }
    }
}
