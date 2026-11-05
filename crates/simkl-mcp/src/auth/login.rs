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

use askama::Template;
use askama_web::WebTemplate;
use axum::Form;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, RawQuery, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
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

/// What the sign-in routes share.
#[derive(Clone)]
pub struct Login {
    server: Arc<AuthorizationServer<Store>>,
    upstream: Upstream,
    vault: Arc<Vault>,
    pending: Arc<PendingSignIns<SignIn>>,
    /// Whether the public URL is `https`, so the cookie can be `Secure`.
    secure: bool,
    /// How long a sign-in may take, consent page to Simkl callback.
    sign_in_ttl: Duration,
}

impl Login {
    pub fn new(
        server: Arc<AuthorizationServer<Store>>,
        upstream: Upstream,
        vault: Arc<Vault>,
        secure: bool,
        sign_in_ttl: Duration,
    ) -> Login {
        Login {
            server,
            upstream,
            vault,
            pending: Arc::new(PendingSignIns::new(sign_in_ttl)),
            secure,
            sign_in_ttl,
        }
    }

    /// The cookie that ties a pending sign-in to its browser. Over `https`
    /// it carries the `__Host-` prefix, so the browser only accepts it
    /// `Secure`, host-only and for `/`. It is an identifier rather than a
    /// setting: renaming it only orphans sign-ins in flight.
    fn cookie_name(&self) -> &'static str {
        match self.secure {
            true => "__Host-simkl_mcp_signin",
            false => "simkl_mcp_signin",
        }
    }

    fn binding(&self, jar: &CookieJar) -> Option<String> {
        jar.get(self.cookie_name())
            .map(|cookie| cookie.value().to_owned())
            .filter(|value| !value.is_empty())
    }

    fn binding_cookie(&self, binding: String) -> Cookie<'static> {
        let max_age = cookie::time::Duration::try_from(self.sign_in_ttl).unwrap();
        Cookie::build((self.cookie_name(), binding))
            .path("/")
            .http_only(true)
            .secure(self.secure)
            .same_site(SameSite::Lax)
            .max_age(max_age)
            .build()
    }

    /// Clears the cookie once the sign-in is over.
    fn forget(&self, jar: CookieJar) -> CookieJar {
        jar.remove(Cookie::build(self.cookie_name()).path("/"))
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

/// `templates/consent.html`: asks the user to allow a client.
#[derive(Template, WebTemplate)]
#[template(path = "consent.html")]
struct ConsentPage {
    client: String,
    redirect_host: String,
    write: bool,
    key: String,
}

/// `templates/message.html`: a sign-in that cannot go on.
#[derive(Template, WebTemplate)]
#[template(path = "message.html")]
struct MessagePage {
    title: &'static str,
    text: String,
}

/// `GET /authorize`: validate the client's request and ask the user.
pub async fn authorize(
    State(login): State<Login>,
    jar: CookieJar,
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
            return failed(
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
    let page = ConsentPage {
        client: client_name.unwrap_or_else(|| validated.client_id.as_str().to_owned()),
        redirect_host: url::Url::parse(&validated.redirect_uri)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
            .unwrap_or_else(|| validated.redirect_uri.clone()),
        write: validated.scope.contains(WRITE_SCOPE),
        key: random_token(),
    };
    let binding = login.binding(&jar).unwrap_or_else(random_token);
    login
        .pending
        .put(
            page.key.clone(),
            binding.clone(),
            SignIn::Consent(validated),
        )
        .await;
    (jar.add(login.binding_cookie(binding)), page).into_response()
}

#[derive(Deserialize)]
pub struct ConsentForm {
    key: String,
    action: String,
}

/// `POST /authorize`: the consent page's answer.
pub async fn consent(
    State(login): State<Login>,
    jar: CookieJar,
    Form(form): Form<ConsentForm>,
) -> Response {
    let binding = login.binding(&jar);
    let Some(SignIn::Consent(request)) = login.pending.take(&form.key, binding.as_deref()).await
    else {
        return expired();
    };
    if form.action != "allow" {
        return (login.forget(jar), see_other(&request.denied().location())).into_response();
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
    jar: CookieJar,
    state: Result<Query<CallbackState>, QueryRejection>,
    callback: Result<Query<Callback>, QueryRejection>,
) -> Response {
    let binding = login.binding(&jar);
    let jar = login.forget(jar);
    let (Ok(Query(CallbackState { state })), Ok(Query(callback))) = (state, callback) else {
        return (jar, expired()).into_response();
    };
    let Some(SignIn::Simkl { request, simkl }) =
        login.pending.take(&state, binding.as_deref()).await
    else {
        return (jar, expired()).into_response();
    };
    let response = finish(&login, request, simkl, callback).await;
    (jar, response).into_response()
}

/// Exchanges Simkl's code, keeps the tokens, and issues our code.
async fn finish(
    login: &Login,
    request: ValidatedAuthorizationRequest,
    simkl: PendingSignIn,
    callback: Callback,
) -> Response {
    let grant = match login.upstream.exchange(simkl, callback).await {
        Ok(grant) => grant,
        Err(UpstreamError::Denied) => return see_other(&request.denied().location()),
        Err(error) => {
            tracing::warn!(%error, "Simkl sign-in failed");
            return failed(
                StatusCode::BAD_GATEWAY,
                "Signing in with Simkl failed. Start again from your app.",
            );
        }
    };
    // Our token's scope is the client's request, so Simkl must have granted
    // as much, or write tools would fail behind a token that allows them.
    if request.scope.contains(WRITE_SCOPE) && grant.scope != SimklScope::ReadWrite {
        return failed(
            StatusCode::BAD_GATEWAY,
            "Simkl did not grant write access. Start again from your app.",
        );
    }
    let connection = match login.vault.connect(grant).await {
        Ok(connection) => connection,
        Err(error) => {
            tracing::error!(%error, "keeping Simkl tokens failed");
            return failed(
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
        AuthorizationError::Direct(error) => failed(StatusCode::BAD_REQUEST, &error.to_string()),
    }
}

fn expired() -> Response {
    failed(
        StatusCode::BAD_REQUEST,
        "This sign-in has expired. Start again from your app.",
    )
}

fn failed(status: StatusCode, text: &str) -> Response {
    let page = MessagePage {
        title: "sign-in failed",
        text: text.to_owned(),
    };
    (status, page).into_response()
}

fn see_other(location: &str) -> Response {
    Redirect::to(location).into_response()
}

fn random_token() -> String {
    URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>())
}
