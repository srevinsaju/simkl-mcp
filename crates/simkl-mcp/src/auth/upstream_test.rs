use oauth2::CsrfToken;
use rstest::rstest;

use super::{Callback, UpstreamError};

const STATE: &str = "state-of-this-sign-in";

fn callback(iss: &str, state: &str, code: Option<&str>, error: Option<&str>) -> Callback {
    Callback {
        iss: iss.to_owned(),
        state: state.to_owned(),
        code: code.map(str::to_owned),
        error: error.map(str::to_owned),
    }
}

#[rstest]
#[case::code(callback("https://simkl.com", STATE, Some("abc"), None), Ok("abc"))]
#[case::wrong_issuer(
    callback("https://evil.example", STATE, Some("abc"), None),
    Err("WrongIssuer")
)]
#[case::wrong_issuer_before_state(
    callback("https://evil.example", "other", Some("abc"), None),
    Err("WrongIssuer")
)]
#[case::wrong_state(
    callback("https://simkl.com", "other", Some("abc"), None),
    Err("WrongState")
)]
#[case::denied(
    callback("https://simkl.com", STATE, None, Some("access_denied")),
    Err("Denied")
)]
fn verifies_issuer_then_state_before_the_code(
    #[case] callback: Callback,
    #[case] expected: Result<&str, &str>,
) {
    let verified = callback.verify(&CsrfToken::new(STATE.to_owned()));
    let verified = verified
        .as_ref()
        .map(|code| code.secret().as_str())
        .map_err(|error| match error {
            UpstreamError::WrongIssuer => "WrongIssuer",
            UpstreamError::WrongState => "WrongState",
            UpstreamError::Denied => "Denied",
            _ => "other",
        });
    assert_eq!(verified, expected);
}
