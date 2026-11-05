//! Signing a user in: our `/authorize` and consent page, then Simkl, then the
//! authorization code.
//!
//! `oauth-as`'s own authorize handler only learns who the user is from a
//! synchronous look at the request headers, which cannot send a signed-out
//! user to Simkl. So `/authorize` is ours:
//!
//! 1. `GET /authorize` validates the client's request with `oauth-as` and
//!    shows a consent page naming the client and where it redirects. Without
//!    it, any registered client could get a code for someone already signed in
//!    to Simkl.
//! 2. `POST /authorize` (Allow) sends the user to Simkl, asking for write only
//!    when the client asked for it.
//! 3. `GET /auth/simkl/callback` exchanges Simkl's code, keeps the tokens in
//!    the vault, and has `oauth-as` issue our code with the new connection as
//!    its subject.
//!
//! Between steps the request waits in [`PendingSignIns`], in memory (one
//! replica) for ten minutes, tied to the browser that started it by a cookie.
//! A restart mid-sign-in means starting again.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Form;
use axum::extract::{Query, RawQuery, State};
use axum::http::header::{COOKIE, LOCATION, SET_COOKIE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use moka::future::Cache;
use oauth_as::authorization::{
    AuthorizationError, AuthorizationRequest, ValidatedAuthorizationRequest,
};
use oauth_as::client::ClientId;
use oauth_as::server::{AuthorizationServer, UserApproval};
use oauth_as::store::Storage;
use serde::Deserialize;

use crate::auth::clients::{WRITE_SCOPE, widen_registered_scopes};
use crate::auth::store::Store;
use crate::auth::upstream::{Callback, PendingSignIn, SimklScope, Upstream, UpstreamError};
use crate::auth::vault::Vault;

/// The cookie that ties a pending sign-in to its browser.
const COOKIE_NAME: &str = "simkl_mcp_signin";
/// How long a sign-in may take, consent page to Simkl callback.
const SIGN_IN_TTL: Duration = Duration::from_secs(10 * 60);

/// What the sign-in routes share.
#[derive(Clone)]
pub struct Login {
    server: Arc<AuthorizationServer<Store>>,
    upstream: Upstream,
    vault: Arc<Vault>,
    pending: Arc<PendingSignIns<SignIn>>,
    /// Whether cookies need `Secure`: the public URL is `https`.
    secure: bool,
}

impl Login {
    pub fn new(
        server: Arc<AuthorizationServer<Store>>,
        upstream: Upstream,
        vault: Arc<Vault>,
        secure: bool,
    ) -> Login {
        Login {
            server,
            upstream,
            vault,
            pending: Arc::new(PendingSignIns::new(SIGN_IN_TTL)),
            secure,
        }
    }
}

/// A sign-in between our consent page and Simkl's callback.
enum SignIn {
    /// Waiting for the user to allow the client.
    Consent(ValidatedAuthorizationRequest),
    /// Waiting for Simkl to send the user back.
    Simkl {
        request: ValidatedAuthorizationRequest,
        simkl: PendingSignIn,
    },
}

/// A pending sign-in and the browser it is bound to, taken at most once.
type Slot<T> = Arc<Mutex<Option<(String, T)>>>;

/// Sign-ins in progress, each usable once and only from the browser holding
/// its cookie. Generic over what a sign-in carries.
pub struct PendingSignIns<T> {
    entries: Cache<String, Slot<T>>,
}

impl<T: Send + Sync + 'static> PendingSignIns<T> {
    fn new(ttl: Duration) -> PendingSignIns<T> {
        PendingSignIns {
            entries: Cache::builder().time_to_live(ttl).build(),
        }
    }

    /// Keeps `sign_in` under `key` for the browser holding `binding`.
    async fn put(&self, key: String, binding: String, sign_in: T) {
        self.entries
            .insert(key, Arc::new(Mutex::new(Some((binding, sign_in)))))
            .await;
    }

    /// Takes the sign-in under `key`, but only for the browser it is bound
    /// to. Either way it is gone afterwards: a sign-in is never offered twice.
    async fn take(&self, key: &str, binding: Option<&str>) -> Option<T> {
        let entry = self.entries.remove(key).await?;
        let (bound_to, sign_in) = entry.lock().unwrap().take()?;
        (Some(bound_to.as_str()) == binding).then_some(sign_in)
    }
}

/// `GET /authorize`: validate the client's request and ask the user.
pub async fn authorize(
    State(login): State<Login>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Response {
    let pairs: Vec<(String, String)> =
        url::form_urlencoded::parse(query.unwrap_or_default().as_bytes())
            .into_owned()
            .collect();
    let request = AuthorizationRequest::from_pairs(pairs);
    if let Some(client_id) = &request.client_id {
        let client_id = ClientId::new(client_id.as_ref());
        if let Err(error) = widen_registered_scopes(login.server.store(), &client_id).await {
            tracing::error!(%error, "widening a client's scopes failed");
            return message(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Something went wrong. Try again.",
            );
        }
    }
    let validated = match login.server.validate_authorization_request(&request).await {
        Ok(validated) => validated,
        Err(error) => return authorization_error(error),
    };
    let client_name = match login.server.store().get_client(&validated.client_id).await {
        Ok(Some(client)) => client.name.clone(),
        _ => None,
    };
    let key = random_token();
    let page = consent_page(&validated, client_name.as_deref(), &key);
    let binding = browser_binding(&headers).unwrap_or_else(random_token);
    login
        .pending
        .put(key, binding.clone(), SignIn::Consent(validated))
        .await;
    let mut response = Html(page).into_response();
    response
        .headers_mut()
        .insert(SET_COOKIE, binding_cookie(&binding, login.secure));
    response
}

#[derive(Deserialize)]
pub struct ConsentForm {
    key: String,
    action: String,
}

/// `POST /authorize`: the consent page's answer.
pub async fn consent(
    State(login): State<Login>,
    headers: HeaderMap,
    Form(form): Form<ConsentForm>,
) -> Response {
    let binding = browser_binding(&headers);
    let Some(SignIn::Consent(request)) = login.pending.take(&form.key, binding.as_deref()).await
    else {
        return expired();
    };
    if form.action != "allow" {
        return see_other(&request.denied().location());
    }
    let scope = match request.scope.contains(WRITE_SCOPE) {
        true => SimklScope::ReadWrite,
        false => SimklScope::Read,
    };
    let (simkl_url, simkl) = login.upstream.authorize(scope);
    let binding = binding.expect("take succeeded, so the browser presented its binding");
    login
        .pending
        .put(
            simkl.state().to_owned(),
            binding,
            SignIn::Simkl { request, simkl },
        )
        .await;
    see_other(simkl_url.as_str())
}

#[derive(Deserialize)]
pub struct CallbackState {
    state: String,
}

/// `GET /auth/simkl/callback`: Simkl sent the user back.
pub async fn callback(
    State(login): State<Login>,
    headers: HeaderMap,
    Query(CallbackState { state }): Query<CallbackState>,
    Query(callback): Query<Callback>,
) -> Response {
    let Some(SignIn::Simkl { request, simkl }) = login
        .pending
        .take(&state, browser_binding(&headers).as_deref())
        .await
    else {
        return expired();
    };
    let grant = match login.upstream.exchange(simkl, callback).await {
        Ok(grant) => grant,
        Err(UpstreamError::Denied) => return see_other(&request.denied().location()),
        Err(error) => {
            tracing::warn!(%error, "Simkl sign-in failed");
            return message(
                StatusCode::BAD_GATEWAY,
                "Signing in with Simkl failed. Start again from your app.",
            );
        }
    };
    // Our token's scope is the client's request, so Simkl must have granted
    // as much, or write tools would fail behind a token that allows them.
    if request.scope.contains(WRITE_SCOPE) && grant.scope != SimklScope::ReadWrite {
        return message(
            StatusCode::BAD_GATEWAY,
            "Simkl did not grant write access. Start again from your app.",
        );
    }
    let connection = match login.vault.connect(grant).await {
        Ok(connection) => connection,
        Err(error) => {
            tracing::error!(%error, "keeping Simkl tokens failed");
            return message(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Something went wrong. Start again from your app.",
            );
        }
    };
    let approval = UserApproval::granted(&request, connection.as_str());
    match login.server.issue_authorization_code(approval).await {
        Ok(code) => see_other(&code.location(&request.redirect_uri)),
        Err(error) => authorization_error(error),
    }
}

fn authorization_error(error: AuthorizationError) -> Response {
    match error {
        AuthorizationError::Redirect(redirect) => see_other(&redirect.location()),
        AuthorizationError::Direct(error) => message(StatusCode::BAD_REQUEST, &error.to_string()),
    }
}

fn consent_page(
    request: &ValidatedAuthorizationRequest,
    client_name: Option<&str>,
    key: &str,
) -> String {
    let redirect_host = url::Url::parse(&request.redirect_uri)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_else(|| request.redirect_uri.clone());
    let client = client_name.unwrap_or(request.client_id.as_str());
    let access = match request.scope.contains(WRITE_SCOPE) {
        true => "read and change your Simkl watchlist, history and ratings",
        false => "read your Simkl watchlist, history and ratings",
    };
    format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width\"><title>Connect to Simkl</title></head>\
         <body><h1>Connect {client} to Simkl</h1>\
         <p><strong>{client}</strong> wants to {access}.</p>\
         <p>After you allow it, Simkl asks you to sign in, and you are sent back to <code>{host}</code>.</p>\
         <form method=\"post\" action=\"/authorize\">\
         <input type=\"hidden\" name=\"key\" value=\"{key}\">\
         <button type=\"submit\" name=\"action\" value=\"allow\">Allow</button> \
         <button type=\"submit\" name=\"action\" value=\"deny\">Deny</button>\
         </form></body></html>",
        client = escape(client),
        access = access,
        host = escape(&redirect_host),
        key = escape(key),
    )
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn expired() -> Response {
    message(
        StatusCode::BAD_REQUEST,
        "This sign-in has expired. Start again from your app.",
    )
}

fn message(status: StatusCode, text: &str) -> Response {
    (
        status,
        Html(format!("<!DOCTYPE html><p>{}</p>", escape(text))),
    )
        .into_response()
}

fn see_other(location: &str) -> Response {
    match HeaderValue::from_str(location) {
        Ok(location) => (StatusCode::SEE_OTHER, [(LOCATION, location)]).into_response(),
        Err(_) => message(StatusCode::BAD_REQUEST, "Bad redirect."),
    }
}

fn random_token() -> String {
    URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>())
}

fn browser_binding(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|cookies| cookies.split(';'))
        .find_map(|cookie| cookie.trim().strip_prefix(COOKIE_NAME)?.strip_prefix('='))
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn binding_cookie(binding: &str, secure: bool) -> HeaderValue {
    let secure = if secure { "; Secure" } else { "" };
    HeaderValue::from_str(&format!(
        "{COOKIE_NAME}={binding}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{secure}",
        SIGN_IN_TTL.as_secs()
    ))
    .unwrap()
}
