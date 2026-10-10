//! The HTTP client: endpoints as types, Simkl's request rules, rate limiting,
//! and mapping responses into [`Error`].
//!
//! Every endpoint implements [`Endpoint`], and its [`Access`] marker decides
//! how it is sent:
//!
//! | Access | Host | Token | Limits |
//! |---|---|---|---|
//! | [`Catalog`] | api.simkl.com | never | none (edge-cached) |
//! | [`Data`] | data.simkl.in | never | none (edge-cached) |
//! | [`Read`] | api.simkl.com | required | user's GET limit |
//! | [`Write`] | api.simkl.com | required | user's POST limit, one write at a time |
//! | [`Redirect`] | api.simkl.com | required | none; the answer is the redirect |
//!
//! [`Client::call`] takes only token-free endpoints; [`UserClient::call`]
//! takes any. Reads never carry a body, which Simkl would treat as a write.
//!
//! This module is private: only this crate declares endpoints.

use std::hash::{BuildHasher, RandomState};
use std::sync::Arc;
use std::time::Duration;

use backon::{ConstantBuilder, Retryable};
use garde::Validate;
use governor::{DefaultKeyedRateLimiter, Quota, RateLimiter};
use moka::future::Cache;
use reqwest::header::{self, HeaderMap};
use reqwest::{Method, RequestBuilder, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::config::{Config, Limits};
use crate::error::{Error, Result};

/// A user's Simkl access token. Never printed, and wiped on drop.
pub type AccessToken = SecretString;

/// A Simkl API endpoint. Implement with the `endpoint!` macro.
///
/// The value serializes to the query string for reads and to the JSON body
/// for writes; fields that only fill the path are `#[serde(skip)]`. It is
/// validated before sending.
pub trait Endpoint: Serialize + Validate<Context = ()> + Sync {
    type Access: Access;
    type Response;

    fn path(&self) -> String;

    fn parse(&self, reply: Reply) -> Result<Self::Response>;
}

/// How an endpoint is sent. See the [module docs](self).
pub trait Access: Send + Sync + 'static {
    const RULES: Rules;
}

/// Endpoints that need no user, callable through [`Client::call`].
pub trait Anonymous: Access {}

macro_rules! access {
    ($($(#[$doc:meta])* $name:ident $({$anonymous:ident})? = $rules:expr;)+) => {$(
        $(#[$doc])*
        #[expect(dead_code, reason = "endpoints arrive in the next PR")]
        #[derive(Debug)]
        pub enum $name {}
        impl Access for $name {
            const RULES: Rules = $rules;
        }
        $(impl $anonymous for $name {})?
    )+};
}

access! {
    /// Public catalog data on api.simkl.com.
    Catalog {Anonymous} = Rules { host: Host::Api, token: false, limit: None, follow_redirects: true };
    /// Static files on data.simkl.in.
    Data {Anonymous} = Rules { host: Host::Data, token: false, limit: None, follow_redirects: true };
    /// The user's data, read.
    Read = Rules { host: Host::Api, token: true, limit: Some(Limit::Read), follow_redirects: true };
    /// The user's data, written.
    Write = Rules { host: Host::Api, token: true, limit: Some(Limit::Write), follow_redirects: true };
    /// `/redirect`, whose answer is the `Location` it redirects to.
    Redirect = Rules { host: Host::Api, token: true, limit: None, follow_redirects: false };
}

#[derive(Debug, Clone, Copy)]
pub struct Rules {
    host: Host,
    token: bool,
    limit: Option<Limit>,
    follow_redirects: bool,
}

#[expect(dead_code, reason = "endpoints arrive in the next PR")]
#[derive(Debug, Clone, Copy)]
enum Host {
    Api,
    Data,
}

#[expect(dead_code, reason = "endpoints arrive in the next PR")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Limit {
    Read,
    Write,
}

/// Client for Simkl's public data. Cheap to clone.
#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

struct Inner {
    config: Config,
    http: reqwest::Client,
    /// For `/redirect`, whose answer is the redirect itself.
    no_redirect: reqwest::Client,
    users: UserLimits,
}

/// Per-user rate limits and write serialisation, keyed by a hash of the token
/// so raw tokens are not kept as map keys.
struct UserLimits {
    hasher: RandomState,
    reads: DefaultKeyedRateLimiter<u64>,
    writes: DefaultKeyedRateLimiter<u64>,
    write_locks: Cache<u64, Arc<Mutex<()>>>,
    prune_at_users: usize,
}

impl UserLimits {
    fn new(limits: Limits) -> Self {
        Self {
            hasher: RandomState::new(),
            reads: RateLimiter::keyed(Quota::per_second(limits.reads_per_second)),
            writes: RateLimiter::keyed(Quota::per_second(limits.writes_per_second)),
            write_locks: Cache::builder().time_to_idle(limits.idle_user_ttl).build(),
            prune_at_users: limits.prune_at_users,
        }
    }

    fn key(&self, token: &AccessToken) -> u64 {
        self.hasher.hash_one(token.expose_secret())
    }

    async fn until_ready(&self, limit: Limit, key: u64) {
        let limiter = match limit {
            Limit::Read => &self.reads,
            Limit::Write => &self.writes,
        };
        if limiter.len() > self.prune_at_users {
            limiter.retain_recent();
        }
        limiter.until_key_ready(&key).await;
    }
}

impl Client {
    pub fn new(config: Config) -> Result<Self> {
        let user_agent = format!("{}/{}", config.app_name, config.app_version);
        let builder = || reqwest::Client::builder().user_agent(&user_agent);
        let http = builder().build()?;
        let no_redirect = builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let users = UserLimits::new(config.limits);
        Ok(Self {
            inner: Arc::new(Inner {
                config,
                http,
                no_redirect,
                users,
            }),
        })
    }

    pub fn config(&self) -> &Config {
        &self.inner.config
    }

    /// Act as the user who owns `token`.
    pub fn user(&self, token: AccessToken) -> UserClient {
        UserClient {
            key: self.inner.users.key(&token),
            client: self.clone(),
            token,
        }
    }

    /// Call an endpoint that needs no user.
    pub async fn call<E>(&self, endpoint: E) -> Result<E::Response>
    where
        E: Endpoint,
        E::Access: Anonymous,
    {
        self.execute(&endpoint, None).await
    }

    /// The one path every request takes.
    async fn execute<E: Endpoint>(
        &self,
        endpoint: &E,
        user: Option<&UserClient>,
    ) -> Result<E::Response> {
        endpoint.validate()?;
        let rules = E::Access::RULES;
        let Inner {
            config,
            http,
            no_redirect,
            users,
        } = &*self.inner;

        let mut _write_guard = None;
        if let (Some(user), Some(limit)) = (user, rules.limit) {
            if limit == Limit::Write {
                let lock = users
                    .write_locks
                    .get_with(user.key, async { Arc::default() })
                    .await;
                _write_guard = Some(lock.lock_owned().await);
            }
            users.until_ready(limit, user.key).await;
        }

        let base = match rules.host {
            Host::Api => &config.api_url,
            Host::Data => &config.data_url,
        };
        let url = base
            .join(&endpoint.path())
            .expect("endpoint paths are valid relative URLs");
        let http = if rules.follow_redirects {
            http
        } else {
            no_redirect
        };
        let writes = rules.limit == Some(Limit::Write);
        let method = if writes { Method::POST } else { Method::GET };
        let mut request = http.request(method, url).query(&[
            ("client_id", config.client_id.as_str()),
            ("app-name", config.app_name.as_str()),
            ("app-version", config.app_version.as_str()),
        ]);
        request = if writes {
            request.json(endpoint)
        } else {
            request.query(endpoint)
        };
        if let (true, Some(user)) = (rules.token, user) {
            request = request.bearer_auth(user.token.expose_secret());
        }

        let reply = self.send(request).await?;
        endpoint.parse(reply)
    }

    /// Send, retrying per-second rate limits and the per-user write lock.
    async fn send(&self, request: RequestBuilder) -> Result<Reply> {
        let retries = self.inner.config.retries;
        let attempt = || async {
            let request = request.try_clone().expect("requests have buffered bodies");
            Reply::read(request.send().await.map_err(Error::from)?).await
        };
        let backoff = ConstantBuilder::new()
            .with_delay(retries.burst_delay)
            .with_max_times(retries.attempts);
        let reply = attempt
            .retry(backoff)
            .when(|failure| matches!(failure, Failure::Burst | Failure::WriteLocked))
            .adjust(|failure, delay| match failure {
                Failure::WriteLocked => delay.map(|_| retries.write_lock_delay),
                _ => delay,
            })
            .notify(|failure, delay| {
                tracing::debug!(?failure, ?delay, "retrying Simkl request");
            })
            .await?;
        Ok(reply)
    }
}

/// A [`Client`] acting for one user. Cheap to clone.
#[derive(Clone)]
pub struct UserClient {
    client: Client,
    token: AccessToken,
    key: u64,
}

impl UserClient {
    /// Call any endpoint as this user. Public endpoints still go out
    /// without the token.
    pub async fn call<E: Endpoint>(&self, endpoint: E) -> Result<E::Response> {
        self.client.execute(&endpoint, Some(self)).await
    }
}

/// A successful response, or the redirect `/redirect` answers with.
#[derive(Debug)]
pub struct Reply {
    headers: HeaderMap,
    body: serde_json::Value,
}

impl Reply {
    async fn read(response: reqwest::Response) -> Result<Self, Failure> {
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response.bytes().await.map_err(Error::from)?;
        Self::classify(status, headers, &bytes)
    }

    /// Simkl's [error conventions](https://api.simkl.org/conventions/errors)
    /// reuse statuses: a write lock is a `400`, three different limits share
    /// `429`, and some errors arrive in a `200` body.
    fn classify(status: StatusCode, headers: HeaderMap, bytes: &[u8]) -> Result<Self, Failure> {
        if status.is_redirection() {
            let body = serde_json::Value::Null;
            return Ok(Self { headers, body });
        }
        if !status.is_success() {
            // Error bodies are not always JSON; the status still decides.
            let body: ErrorBody = serde_json::from_slice(bytes).unwrap_or_default();
            return Err(Failure::from_error(status, &headers, body));
        }
        let body: serde_json::Value = match bytes.is_empty() {
            true => serde_json::Value::Null,
            false => serde_json::from_slice(bytes).map_err(Error::from)?,
        };
        if let Ok(ErrorBody {
            error: Some(error),
            message,
            ..
        }) = ErrorBody::deserialize(&body)
        {
            let error = Some(error);
            return Err(Error::Api {
                status,
                error,
                message,
            }
            .into());
        }
        Ok(Self { headers, body })
    }

    pub fn json<T: DeserializeOwned>(self) -> Result<T> {
        Ok(serde_json::from_value(self.body)?)
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name)?.to_str().ok()
    }
}

/// Shape of Simkl's error bodies. Every field is optional because not every
/// error has a body.
#[derive(Debug, Default, Deserialize)]
struct ErrorBody {
    error: Option<String>,
    message: Option<String>,
    error_description: Option<String>,
}

/// Why a request failed, and whether retrying can help.
#[derive(Debug, derive_more::From)]
enum Failure {
    /// `429 rate_limit`: over the per-second limit.
    #[from(ignore)]
    Burst,
    /// `400 rate_limit`: another write for this user is in progress.
    #[from(ignore)]
    WriteLocked,
    Final(Error),
}

impl Failure {
    fn from_error(status: StatusCode, headers: &HeaderMap, body: ErrorBody) -> Self {
        let code = body.error.as_deref().unwrap_or_default();
        let is = |expected: &str| code.eq_ignore_ascii_case(expected);
        match status {
            StatusCode::UNAUTHORIZED => Error::Unauthorized { code: body.error }.into(),
            StatusCode::FORBIDDEN if is("insufficient_scope") => Error::InsufficientScope.into(),
            StatusCode::BAD_REQUEST if is("rate_limit") => Failure::WriteLocked,
            StatusCode::TOO_MANY_REQUESTS
                if is("user_limit_exceeded") || is("app_limit_exceeded") =>
            {
                // Only a daily-quota 429 carries Retry-After, in seconds.
                let retry_after = headers
                    .get(header::RETRY_AFTER)
                    .and_then(|v| v.to_str().ok()?.parse().ok())
                    .map(Duration::from_secs);
                Error::QuotaExceeded { retry_after }.into()
            }
            StatusCode::TOO_MANY_REQUESTS => Failure::Burst,
            // Includes `412 client_id_failed`: the app is throttled, which
            // retrying does not fix.
            _ => Error::Api {
                status,
                error: body.error,
                message: body.message.or(body.error_description),
            }
            .into(),
        }
    }
}

impl From<Failure> for Error {
    fn from(failure: Failure) -> Self {
        match failure {
            Failure::Burst => Error::RateLimited,
            Failure::WriteLocked => Error::WriteLocked,
            Failure::Final(error) => error,
        }
    }
}

#[cfg(test)]
#[path = "client_test.rs"]
mod tests;
