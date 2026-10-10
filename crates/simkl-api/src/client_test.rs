use std::time::Duration;

use reqwest::StatusCode;
use reqwest::header::{HeaderMap, HeaderValue, RETRY_AFTER};
use rstest::rstest;

use super::{Failure, Reply};
use crate::error::Error;

/// Simkl's [error conventions](https://api.simkl.org/conventions/errors)
/// decide what is retried, what triggers step-up, and what is final.
#[rstest]
#[case::write_lock(400, None, r#"{"error":"rate_limit"}"#,
    |f: &Failure| matches!(f, Failure::WriteLocked))]
#[case::burst(429, None, r#"{"error":"rate_limit"}"#,
    |f: &Failure| matches!(f, Failure::Burst))]
#[case::user_quota(429, Some("3600"), r#"{"error":"user_limit_exceeded"}"#,
    |f: &Failure| matches!(f, Failure::Final(Error::QuotaExceeded { retry_after: Some(d) }) if *d == Duration::from_secs(3600)))]
#[case::app_quota(429, None, r#"{"error":"app_limit_exceeded"}"#,
    |f: &Failure| matches!(f, Failure::Final(Error::QuotaExceeded { retry_after: None })))]
#[case::client_id_throttled(412, None, r#"{"error":"client_id_failed"}"#,
    |f: &Failure| matches!(f, Failure::Final(Error::Api { error: Some(e), .. }) if e == "client_id_failed"))]
#[case::insufficient_scope(403, None, r#"{"error":"insufficient_scope"}"#,
    |f: &Failure| matches!(f, Failure::Final(Error::InsufficientScope)))]
#[case::unauthorized(401, None, r#"{"error":"invalid_token"}"#,
    |f: &Failure| matches!(f, Failure::Final(Error::Unauthorized { code: Some(c) }) if c == "invalid_token"))]
#[case::error_in_ok_body(200, None, r#"{"error":"premium_only","message":"VIP only"}"#,
    |f: &Failure| matches!(f, Failure::Final(Error::Api { status: StatusCode::OK, error: Some(e), .. }) if e == "premium_only"))]
fn classifies_simkl_errors(
    #[case] status: u16,
    #[case] retry_after: Option<&str>,
    #[case] body: &str,
    #[case] expected: fn(&Failure) -> bool,
) {
    let mut headers = HeaderMap::new();
    if let Some(seconds) = retry_after {
        headers.insert(RETRY_AFTER, HeaderValue::from_str(seconds).unwrap());
    }
    let status = StatusCode::from_u16(status).unwrap();
    let failure = Reply::classify(status, headers, body.as_bytes()).unwrap_err();
    assert!(expected(&failure), "got {failure:?}");
}
