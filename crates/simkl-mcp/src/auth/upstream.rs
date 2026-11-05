//! The Simkl side of signing in, via the `oauth2` crate: authorization code
//! with PKCE (S256), the callback's `state` and `iss` checks, refresh and
//! revoke ([AUTH V2](https://api.simkl.org/api-reference/auth-v2)).
//!
//! Simkl is a confidential client here ("Server apps & services"), so the
//! token and revoke requests carry the client secret (HTTP Basic). A refresh
//! returns the same refresh token with its 180-day window moved, and kills the
//! previous access token at once; [`Vault`](super::vault) is the only caller.

use std::time::Duration;

use jiff::{SignedDuration, Timestamp};
use oauth2::basic::BasicClient;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointNotSet, EndpointSet,
    ErrorResponseType, HttpRequest, HttpResponse, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl,
    RefreshToken, RequestTokenError, RevocationUrl, Scope, StandardErrorResponse,
    StandardRevocableToken, TokenResponse, TokenUrl,
};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use simkl_api::AccessToken;
use url::Url;

/// What Simkl puts in the callback's `iss`
/// ([discovery](https://simkl.com/.well-known/oauth-authorization-server)).
const ISSUER: &str = "https://simkl.com";
const AUTHORIZE_URL: &str = "https://simkl.com/oauth2/authorize";
const TOKEN_URL: &str = "https://api.simkl.com/oauth2/token";
const REVOKE_URL: &str = "https://api.simkl.com/oauth2/revoke";

type SimklClient =
    BasicClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointSet, EndpointSet>;

/// Our Simkl app: its credentials, Simkl's endpoints and our callback URL.
pub struct Upstream {
    oauth: SimklClient,
    http: reqwest::Client,
}

impl Upstream {
    /// `redirect_uri` must be registered with Simkl exactly as given; for a
    /// loopback address it must include the port in use.
    pub fn new(
        client_id: String,
        client_secret: SecretString,
        redirect_uri: Url,
    ) -> Result<Upstream, reqwest::Error> {
        let oauth = BasicClient::new(ClientId::new(client_id))
            .set_client_secret(ClientSecret::new(client_secret.expose_secret().to_owned()))
            .set_auth_uri(AuthUrl::new(AUTHORIZE_URL.to_owned()).unwrap())
            .set_token_uri(TokenUrl::new(TOKEN_URL.to_owned()).unwrap())
            .set_revocation_url(RevocationUrl::new(REVOKE_URL.to_owned()).unwrap())
            .set_redirect_uri(RedirectUrl::from_url(redirect_uri));
        // No redirects: a token endpoint that redirects is not one to follow
        // with a client secret.
        let http = reqwest::Client::builder()
            .user_agent(concat!(
                env!("CARGO_PKG_NAME"),
                "/",
                env!("CARGO_PKG_VERSION")
            ))
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(Upstream { oauth, http })
    }

    /// Where to send the user, and what the callback must match.
    pub fn authorize(&self, scope: SimklScope) -> (Url, PendingSignIn) {
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let (url, state) = self
            .oauth
            .authorize_url(CsrfToken::new_random)
            .add_scope(Scope::new(scope.as_str().to_owned()))
            .set_pkce_challenge(challenge)
            .url();
        (url, PendingSignIn { state, verifier })
    }

    /// Checks the callback against `pending`, then exchanges its code.
    pub async fn exchange(
        &self,
        pending: PendingSignIn,
        callback: Callback,
    ) -> Result<SimklGrant, UpstreamError> {
        let code = callback.verify(&pending.state)?;
        let response = self
            .oauth
            .exchange_code(code)
            .set_pkce_verifier(pending.verifier)
            .request_async(&|request| send(&self.http, request))
            .await
            .map_err(token_error)?;
        SimklGrant::from_response(&response, Timestamp::now())
    }

    /// A new access token for `refresh_token`. The previous access token
    /// stops working immediately.
    pub async fn refresh(&self, refresh_token: &SecretString) -> Result<SimklGrant, UpstreamError> {
        let response = self
            .oauth
            .exchange_refresh_token(&RefreshToken::new(refresh_token.expose_secret().to_owned()))
            .request_async(&|request| send(&self.http, request))
            .await
            .map_err(token_error)?;
        SimklGrant::from_response(&response, Timestamp::now())
    }

    /// Ends the grant behind `refresh_token`, access token included. Simkl
    /// answers 200 even for tokens it does not know.
    pub async fn revoke(&self, refresh_token: &SecretString) -> Result<(), UpstreamError> {
        self.oauth
            .revoke_token(StandardRevocableToken::RefreshToken(RefreshToken::new(
                refresh_token.expose_secret().to_owned(),
            )))
            .unwrap()
            .request_async(&|request| send(&self.http, request))
            .await
            .map_err(token_error)
    }
}

/// The Simkl scopes we ask for: read by default, write as a step-up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimklScope {
    Read,
    ReadWrite,
}

impl SimklScope {
    pub fn as_str(self) -> &'static str {
        match self {
            SimklScope::Read => "media:read",
            SimklScope::ReadWrite => "media:read media:write",
        }
    }

    /// Strict: a scope Simkl does not document is an error, not read access.
    fn parse(scopes: &[Scope]) -> Result<SimklScope, UpstreamError> {
        let mut write = false;
        for scope in scopes {
            match scope.as_str() {
                "media:read" => {}
                "media:write" => write = true,
                _ => return Err(UpstreamError::Malformed("unknown scope")),
            }
        }
        Ok(if write {
            SimklScope::ReadWrite
        } else {
            SimklScope::Read
        })
    }
}

/// A sign-in between the redirect to Simkl and its callback.
pub struct PendingSignIn {
    state: CsrfToken,
    verifier: PkceCodeVerifier,
}

/// The query of Simkl's redirect back to us. `iss` is always present
/// ([RFC 9207](https://www.rfc-editor.org/rfc/rfc9207)).
#[derive(Deserialize)]
pub struct Callback {
    iss: String,
    state: String,
    code: Option<String>,
    error: Option<String>,
}

impl Callback {
    /// The code, once the callback is known to come from Simkl (`iss`) for
    /// this sign-in (`state`), in that order.
    fn verify(self, state: &CsrfToken) -> Result<AuthorizationCode, UpstreamError> {
        if self.iss != ISSUER {
            return Err(UpstreamError::WrongIssuer);
        }
        if self.state != *state.secret() {
            return Err(UpstreamError::WrongState);
        }
        match (self.code, self.error) {
            (Some(code), None) => Ok(AuthorizationCode::new(code)),
            (None, Some(error)) if error == "access_denied" => Err(UpstreamError::Denied),
            _ => Err(UpstreamError::Malformed("callback without a code")),
        }
    }
}

/// A Simkl grant's tokens, as of the exchange or refresh that returned them.
pub struct SimklGrant {
    pub access_token: AccessToken,
    pub refresh_token: SecretString,
    pub access_expires_at: Timestamp,
    pub scope: SimklScope,
}

impl SimklGrant {
    /// Strict: Simkl always returns a refresh token, a lifetime and the scope.
    fn from_response(
        response: &impl TokenResponse,
        now: Timestamp,
    ) -> Result<SimklGrant, UpstreamError> {
        let refresh_token = response.refresh_token().ok_or(UpstreamError::Malformed(
            "token response without a refresh token",
        ))?;
        let expires_in = response.expires_in().ok_or(UpstreamError::Malformed(
            "token response without expires_in",
        ))?;
        let scopes = response
            .scopes()
            .ok_or(UpstreamError::Malformed("token response without a scope"))?;
        let expires_in = SignedDuration::try_from(expires_in)
            .map_err(|_| UpstreamError::Malformed("expires_in is out of range"))?;
        Ok(SimklGrant {
            access_token: SecretString::from(response.access_token().secret().as_str()),
            refresh_token: SecretString::from(refresh_token.secret().as_str()),
            access_expires_at: now
                .checked_add(expires_in)
                .map_err(|_| UpstreamError::Malformed("expires_in is out of range"))?,
            scope: SimklScope::parse(scopes)?,
        })
    }
}

/// Why signing in, refreshing or revoking failed. Never carries a token or a
/// response body.
#[derive(Debug, thiserror::Error)]
pub enum UpstreamError {
    #[error("the callback's issuer is not Simkl")]
    WrongIssuer,
    #[error("the callback's state does not match this sign-in")]
    WrongState,
    #[error("the user declined on Simkl")]
    Denied,
    /// `invalid_grant`: the code or refresh token is spent, expired or
    /// revoked.
    #[error("Simkl rejected the grant")]
    Rejected,
    #[error("Simkl answered {0}")]
    Refused(String),
    #[error("malformed Simkl response: {0}")]
    Malformed(&'static str),
    #[error("could not reach Simkl: {0}")]
    Transport(#[source] reqwest::Error),
}

/// Keeps `oauth2`'s parse errors out: they quote the response body, which
/// holds tokens.
fn token_error<T: ErrorResponseType + AsRef<str> + std::fmt::Display>(
    error: RequestTokenError<reqwest::Error, StandardErrorResponse<T>>,
) -> UpstreamError {
    match error {
        RequestTokenError::ServerResponse(response) => match response.error().as_ref() {
            "invalid_grant" => UpstreamError::Rejected,
            code => UpstreamError::Refused(code.to_owned()),
        },
        RequestTokenError::Request(error) => UpstreamError::Transport(error),
        RequestTokenError::Parse(..) => UpstreamError::Malformed("unparseable token response"),
        RequestTokenError::Other(_) => UpstreamError::Malformed("unexpected token response"),
    }
}

/// `oauth2`'s HTTP client over our `reqwest`.
async fn send(
    http: &reqwest::Client,
    request: HttpRequest,
) -> Result<HttpResponse, reqwest::Error> {
    let response = http.execute(request.try_into()?).await?;
    let status = response.status();
    let headers = response.headers().clone();
    let mut out = HttpResponse::new(response.bytes().await?.to_vec());
    *out.status_mut() = status;
    *out.headers_mut() = headers;
    Ok(out)
}

#[cfg(test)]
#[path = "upstream_test.rs"]
mod tests;
