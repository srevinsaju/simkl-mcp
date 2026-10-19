use std::time::Duration;

use reqwest::StatusCode;
use reqwest::header::{HeaderMap, HeaderValue, RETRY_AFTER};

use super::{Failure, Reply};
use crate::error::Error;

/// Simkl's [error conventions](https://api.simkl.org/conventions/errors)
/// decide what is retried, what triggers step-up, and what is final.
#[test]
fn classifies_simkl_errors() {
    let classify = |status: u16, retry_after: Option<&str>, body: &str| {
        let mut headers = HeaderMap::new();
        if let Some(seconds) = retry_after {
            headers.insert(RETRY_AFTER, HeaderValue::from_str(seconds).unwrap());
        }
        let status = StatusCode::from_u16(status).unwrap();
        Reply::classify(status, headers, body.as_bytes()).unwrap_err()
    };

    assert!(matches!(
        classify(400, None, r#"{"error":"rate_limit"}"#),
        Failure::WriteLocked
    ));
    assert!(matches!(
        classify(429, None, r#"{"error":"rate_limit"}"#),
        Failure::Burst
    ));
    assert!(matches!(
        classify(429, Some("3600"), r#"{"error":"user_limit_exceeded"}"#),
        Failure::Final(Error::QuotaExceeded { retry_after: Some(d) }) if d == Duration::from_secs(3600)
    ));
    assert!(matches!(
        classify(429, None, r#"{"error":"app_limit_exceeded"}"#),
        Failure::Final(Error::QuotaExceeded { retry_after: None })
    ));
    assert!(matches!(
        classify(412, None, r#"{"error":"client_id_failed"}"#),
        Failure::Final(Error::Api { error: Some(e), .. }) if e == "client_id_failed"
    ));
    assert!(matches!(
        classify(403, None, r#"{"error":"insufficient_scope"}"#),
        Failure::Final(Error::InsufficientScope)
    ));
    assert!(matches!(
        classify(401, None, r#"{"error":"invalid_token"}"#),
        Failure::Final(Error::Unauthorized { code: Some(c) }) if c == "invalid_token"
    ));
    assert!(matches!(
        classify(200, None, r#"{"error":"premium_only","message":"VIP only"}"#),
        Failure::Final(Error::Api { status: StatusCode::OK, error: Some(e), .. }) if e == "premium_only"
    ));
}
