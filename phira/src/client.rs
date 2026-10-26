//! Http client for Phira API.

mod auth;
mod model;
pub use model::*;

use std::{borrow::Cow, collections::HashMap, fmt, marker::PhantomData, sync::Arc};

use crate::{get_data, get_data_mut, save_data};
use anyhow::{anyhow, bail, Context, Result};
use arc_swap::{ArcSwap, ArcSwapOption};
use auth::{send_with_refresh, Session};
use once_cell::sync::Lazy;
use prpr::scene::SimpleRecord;
use prpr_l10n::LANG_IDENTS;
use reqwest::{header, ClientBuilder, Method, RequestBuilder, Response, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{debug, warn};

pub static CLIENT_TOKEN: Lazy<ArcSwap<Option<String>>> = Lazy::new(|| ArcSwap::from_pointee(None));

static CLIENT: Lazy<ArcSwap<reqwest::Client>> = Lazy::new(|| ArcSwap::from_pointee(basic_client_builder().build().unwrap()));
static SESSION: Lazy<ArcSwapOption<Session>> = Lazy::new(ArcSwapOption::empty);
static SESSION_UPDATE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub struct Client;

// pub const API_URL: &str = "http://localhost:2924";
pub const API_URL: &str = "https://phira.5wyxi.com";

pub fn basic_client_builder() -> ClientBuilder {
    let policy = reqwest::redirect::Policy::custom(|attempt| {
        if let Some(_cid) = attempt.url().as_str().strip_prefix("anys://") {
            attempt.stop()
        } else {
            attempt.follow()
        }
    });
    let mut builder = reqwest::ClientBuilder::new().redirect(policy);
    if get_data().accept_invalid_cert {
        builder = builder.danger_accept_invalid_certs(true);
    }
    builder
}

fn client_locale() -> String {
    get_data().language.clone().unwrap_or(LANG_IDENTS[0].to_string())
}

/// Distribution flavor of this build, read from the optional `flavor` asset.
///
/// Store-specific builds ship this file; open builds don't, so they report
/// `"none"` — the same convention used by the update check.
pub async fn client_flavor() -> String {
    match macroquad::prelude::load_file("flavor").await.map(String::from_utf8) {
        Ok(Ok(flavor)) => flavor.trim().to_owned(),
        _ => "none".to_owned(),
    }
}

fn build_client() -> Result<Arc<reqwest::Client>> {
    let mut headers = header::HeaderMap::new();
    headers.append(header::ACCEPT_LANGUAGE, header::HeaderValue::from_str(&client_locale())?);
    Ok(basic_client_builder().default_headers(headers).build()?.into())
}

pub fn set_access_token_sync(access_token: Option<&str>) -> Result<()> {
    let _guard = SESSION_UPDATE_LOCK.lock().unwrap();
    let client = build_client()?;
    if let Some(token) = access_token {
        header::HeaderValue::from_str(&format!("Bearer {token}"))?;
    }
    let tokens = get_data().tokens.as_ref().filter(|tokens| Some(tokens.0.as_str()) == access_token);
    let current = SESSION.load_full();
    if current.as_ref().map(|session| session.tokens.load_full()).as_deref() != tokens {
        SESSION.store(tokens.cloned().map(Session::new).map(Arc::new));
    }
    CLIENT.store(client);
    CLIENT_TOKEN.store(access_token.map(str::to_owned).into());
    Ok(())
}

pub fn clear_session() {
    let _guard = SESSION_UPDATE_LOCK.lock().unwrap();
    SESSION.store(None);
    CLIENT_TOKEN.store(None.into());
    get_data_mut().me = None;
    get_data_mut().tokens = None;
    if let Err(err) = save_data() {
        warn!(?err, "failed to persist logout");
    }
}

/// Only a rejected refresh credential means that the local session has expired.
pub struct SessionExpired(Arc<Session>);

impl SessionExpired {
    pub fn is_current(&self) -> bool {
        is_current_session(&self.0)
    }
}

impl fmt::Debug for SessionExpired {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SessionExpired")
    }
}

impl fmt::Display for SessionExpired {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("refresh token is no longer valid")
    }
}

impl std::error::Error for SessionExpired {}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ErrorCode(Cow<'static, str>);

macro_rules! error_code {
    ($($name:ident => $status:expr),* $(,)?) => {
        $(
            pub const $name: ErrorCode = ErrorCode(Cow::Borrowed(stringify!($name)));
        )*
    };
}

#[allow(dead_code)]
impl ErrorCode {
    error_code! {
        INVALID_INPUT => StatusCode::BAD_REQUEST,
        UNAUTHENTICATED => StatusCode::UNAUTHORIZED,
        EXPIRED => StatusCode::UNAUTHORIZED,
        PERMISSION_DENIED => StatusCode::FORBIDDEN,
        USER_BANNED => StatusCode::FORBIDDEN,
        PENDING_DELETE_REQUEST => StatusCode::FORBIDDEN,
        RATE_LIMITED => StatusCode::TOO_MANY_REQUESTS,
        NOT_FOUND => StatusCode::NOT_FOUND,
        CONFLICT => StatusCode::CONFLICT,
        NOT_MODIFIED => StatusCode::NOT_MODIFIED,
        NOT_IMPLEMENTED => StatusCode::NOT_IMPLEMENTED,
        STORAGE_UNAVAILABLE => StatusCode::SERVICE_UNAVAILABLE,
        INTERNAL_SERVER_ERROR => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ErrorCode({})", self.0)
    }
}

impl std::error::Error for ErrorCode {}

/// Send an API request, refreshing an expired access token before one retry.
/// Keep the status intact for callers handling 304, 404 or endpoint-specific errors.
pub async fn send_raw(request: RequestBuilder) -> Result<Response> {
    let session = SESSION.load_full();
    send_with_refresh(request, API_URL, async move |token| {
        let Some(session) = session else { return Ok(None) };
        if !session.owns_token(&token) || !is_current_session(&session) {
            return Ok(None);
        }
        refresh_session(&session, &token, false).await?;
        if !is_current_session(&session) {
            return Ok(None);
        }
        Ok(Some(session.tokens.load().0.clone()))
    })
    .await
}

pub async fn recv_raw(request: RequestBuilder) -> Result<Response> {
    check_response(send_raw(request).await?).await
}

async fn check_response(response: Response) -> Result<Response> {
    if !response.status().is_success() {
        let unauthorized = response.status() == StatusCode::UNAUTHORIZED;
        let status = response.status().as_str().to_owned();
        let text = response.text().await.context("failed to receive text")?;
        if let Ok(what) = serde_json::from_str::<serde_json::Value>(&text) {
            let detail = what.get("error").and_then(|it| it.as_str()).unwrap_or("unknown error");
            let mut err = anyhow!("request failed (HTTP {status}): {detail}");
            if let Some(code) = what.get("code").and_then(|it| it.as_str()) {
                err = err.context(ErrorCode(Cow::Owned(code.to_owned())));
            } else if unauthorized {
                err = err.context(ErrorCode::UNAUTHENTICATED);
            }
            return Err(err);
        }
        let err = anyhow!("request failed ({status}): {text}");
        return Err(if unauthorized { err.context(ErrorCode::UNAUTHENTICATED) } else { err });
    }
    Ok(response)
}

fn is_current_session(session: &Arc<Session>) -> bool {
    SESSION.load_full().is_some_and(|current| Arc::ptr_eq(&current, session))
}

fn classify_refresh_error(err: anyhow::Error, session: &Arc<Session>) -> anyhow::Error {
    if is_current_session(session)
        && matches!(err.downcast_ref::<ErrorCode>(), Some(code) if *code == ErrorCode::EXPIRED || *code == ErrorCode::UNAUTHENTICATED)
    {
        err.context(SessionExpired(Arc::clone(session)))
    } else {
        err
    }
}

async fn refresh_session(session: &Arc<Session>, rejected_token: &str, cancel_delete_request: bool) -> Result<()> {
    session
        .refresh(rejected_token, async |tokens| {
            if !is_current_session(session) {
                bail!("session changed while refreshing token");
            }
            let resp = Client::request_login(LoginParams::RefreshToken {
                token: &tokens.1,
                cancel_delete_request,
            })
            .await
            .map_err(|err| classify_refresh_error(err, session))?;
            // A logout or a new login during the request must not restore the old session.
            let _guard = SESSION_UPDATE_LOCK.lock().unwrap();
            if !is_current_session(session) {
                bail!("session changed while refreshing token");
            }
            header::HeaderValue::from_str(&format!("Bearer {}", resp.token))?;
            let tokens = (resp.token, resp.refresh_token);
            session.store_tokens(Arc::new(tokens.clone()));
            CLIENT_TOKEN.store(Some(tokens.0.clone()).into());
            get_data_mut().tokens = Some(tokens.clone());
            // The server may rotate refresh credentials. Keep the new pair in memory
            // even if saving to disk fails, so the current session remains usable.
            if let Err(err) = save_data() {
                warn!(?err, "failed to persist refreshed tokens");
            }
            Ok(tokens)
        })
        .await?;
    Ok(())
}

#[derive(Serialize)]
#[serde(untagged, rename_all_fields = "camelCase")]
pub enum LoginParams<'a> {
    Password {
        email: &'a str,
        password: &'a str,
        cancel_delete_request: bool,
    },
    RefreshToken {
        #[serde(rename = "refreshToken")]
        token: &'a str,
        cancel_delete_request: bool,
    },
}

/// A freshly minted token pair returned by every login endpoint.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoginResp {
    id: i32,
    token: String,
    refresh_token: String,
}

/// Response of `POST /login/hykb`: either an immediate login or a pending choice.
#[cfg(feature = "hykb")]
#[derive(Deserialize)]
#[serde(tag = "status", rename_all = "camelCase")]
enum HykbLoginResp {
    Ok {
        #[serde(flatten)]
        login: LoginResp,
    },
    NeedChoice {
        hykb_token: String,
    },
}

/// Outcome of a HYKB login attempt surfaced to the UI.
#[cfg(feature = "hykb")]
pub enum HykbLoginOutcome {
    /// The HYKB account was already bound; the user is now logged in.
    LoggedIn,
    /// First time we see this HYKB account; the user must register or claim.
    NeedChoice { hykb_token: String },
}

impl Client {
    #[inline]
    pub fn get(path: impl AsRef<str>) -> RequestBuilder {
        Self::request(Method::GET, path)
    }

    #[inline]
    pub fn post<T: Serialize>(path: impl AsRef<str>, data: &T) -> RequestBuilder {
        Self::request(Method::POST, path).json(data)
    }

    #[inline]
    pub fn delete(path: impl AsRef<str>) -> RequestBuilder {
        Self::request(Method::DELETE, path)
    }

    pub fn request(method: Method, path: impl AsRef<str>) -> RequestBuilder {
        let request = CLIENT.load().request(method, API_URL.to_string() + path.as_ref());
        if let Some(token) = CLIENT_TOKEN.load().as_ref() {
            request.bearer_auth(token)
        } else {
            request
        }
    }

    pub fn clear_cache<T: Object + 'static>(id: i32) -> Result<bool> {
        let map = obtain_map_cache::<T>();
        let mut guard = map.lock().unwrap();
        let Some(actual_map) = guard.downcast_mut::<ObjectMap<T>>() else {
            unreachable!()
        };
        Ok(actual_map.pop(&id).is_some())
    }

    pub async fn load<T: Object + 'static>(id: i32) -> Result<Arc<T>> {
        {
            let map = obtain_map_cache::<T>();
            let mut guard = map.lock().unwrap();
            let Some(actual_map) = guard.downcast_mut::<ObjectMap<T>>() else {
                unreachable!()
            };
            if let Some(value) = actual_map.get(&id) {
                return Ok(Arc::clone(value));
            }
            drop(guard);
            drop(map);
        }
        Self::fetch(id).await
    }

    pub async fn fetch<T: Object + 'static>(id: i32) -> Result<Arc<T>> {
        Self::fetch_opt(id).await?.ok_or_else(|| anyhow!("entry not found"))
    }

    pub async fn fetch_opt<T: Object + 'static>(id: i32) -> Result<Option<Arc<T>>> {
        let value = Client::fetch_inner::<T>(id).await?;
        let Some(value) = value else { return Ok(None) };
        let value = Arc::new(value);
        let map = obtain_map_cache::<T>();
        let mut guard = map.lock().unwrap();
        let Some(actual_map) = guard.downcast_mut::<ObjectMap<T>>() else {
            unreachable!()
        };
        actual_map.put(id, Arc::clone(&value));
        Ok(Some(value))
    }

    async fn fetch_inner<T: Object>(id: i32) -> Result<Option<T>> {
        let resp = send_raw(Self::get(format!("/{}/{id}", T::QUERY_PATH))).await?;
        if resp.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !resp.status().is_success() {
            let status = resp.status().as_str().to_owned();
            let text = resp.text().await.context("failed to receive text")?;
            if let Ok(what) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(detail) = what["error"].as_str() {
                    bail!("request failed ({status}): {detail}");
                }
            }
            bail!("request failed ({status}): {text}");
        }
        Ok(Some(resp.json().await?))
    }

    pub fn query<T: Object>() -> QueryBuilder<T> {
        QueryBuilder {
            queries: HashMap::new(),
            page: None,
            suffix: "",
            _phantom: PhantomData,
        }
    }

    pub async fn register(email: &str, username: &str, password: &str) -> Result<()> {
        recv_raw(Self::post(
            "/register",
            &json!({
                "email": email,
                "name": username,
                "password": password,
            }),
        ))
        .await?;
        Ok(())
    }

    pub async fn login(params: LoginParams<'_>) -> Result<()> {
        if let LoginParams::RefreshToken {
            token,
            cancel_delete_request,
        } = params
        {
            let session = SESSION.load_full().context("not logged in")?;
            let tokens = session.tokens.load_full();
            if tokens.1 != token {
                bail!("session changed before refreshing token");
            }
            return refresh_session(&session, &tokens.0, cancel_delete_request).await;
        }
        let resp = Self::request_login(params).await?;
        Self::store_login(resp.id, resp.token, resp.refresh_token).await
    }

    pub async fn refresh_tokens(cancel_delete_request: bool) -> Result<()> {
        let session = SESSION.load_full().context("not logged in")?;
        let tokens = session.tokens.load_full();
        refresh_session(&session, &tokens.0, cancel_delete_request).await
    }

    async fn request_login(params: LoginParams<'_>) -> Result<LoginResp> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct FullLoginParams<'a> {
            #[serde(flatten)]
            inner: LoginParams<'a>,
            #[serde(rename = "clientVersion")]
            client_version: &'static str,
            #[serde(rename = "clientFlavor")]
            client_flavor: String,
        }

        // Login itself must bypass automatic refresh to avoid recursion.
        let resp = check_response(
            Self::post(
                "/login",
                &FullLoginParams {
                    inner: params,
                    client_version: env!("CARGO_PKG_VERSION"),
                    client_flavor: client_flavor().await,
                },
            )
            .send()
            .await?,
        )
        .await?
        .json()
        .await?;

        Ok(resp)
    }

    /// Persist a freshly minted token pair and wire it into the HTTP client.
    /// Shared by the password and HYKB login entry points. `_id` is
    /// kept in the signature so callers can pass the account id even though it
    /// is no longer needed here (the native anti-addiction bridge that used it
    /// is gone).
    async fn store_login(_id: i32, token: String, refresh_token: String) -> Result<()> {
        let _guard = SESSION_UPDATE_LOCK.lock().unwrap();
        let client = build_client()?;
        header::HeaderValue::from_str(&format!("Bearer {token}"))?;
        let tokens = (token, refresh_token);
        get_data_mut().tokens = Some(tokens.clone());
        // Every explicit login starts a new session, even for the same account.
        CLIENT.store(client);
        CLIENT_TOKEN.store(Some(tokens.0.clone()).into());
        SESSION.store(Some(Arc::new(Session::new(tokens))));
        save_data()?;
        Ok(())
    }

    #[cfg(feature = "hykb")]
    /// Entry point for HYKB (好游快爆) login. The verified `(uid, access_token)`
    /// come from the native SDK. Either logs the user straight in (account already
    /// bound) or returns a short-lived `hykb_token` for the register/claim step.
    pub async fn login_hykb(uid: i64, access_token: &str) -> Result<HykbLoginOutcome> {
        let resp: HykbLoginResp = recv_raw(Self::post(
            "/login/hykb",
            &json!({
                "hykbUid": uid,
                "accessToken": access_token,
                "clientVersion": env!("CARGO_PKG_VERSION"),
                "clientFlavor": client_flavor().await,
            }),
        ))
        .await?
        .json()
        .await?;
        match resp {
            HykbLoginResp::Ok { login } => {
                Self::store_login(login.id, login.token, login.refresh_token).await?;
                Ok(HykbLoginOutcome::LoggedIn)
            }
            HykbLoginResp::NeedChoice { hykb_token } => Ok(HykbLoginOutcome::NeedChoice { hykb_token }),
        }
    }

    #[cfg(feature = "hykb")]
    /// New player: create a fresh Phira account bound to the pending HYKB identity,
    /// using the username chosen by the player.
    pub async fn login_hykb_register(hykb_token: &str, username: &str) -> Result<()> {
        let resp: LoginResp = recv_raw(Self::post(
            "/login/hykb/register",
            &json!({
                "hykbToken": hykb_token,
                "nick": username,
                "clientVersion": env!("CARGO_PKG_VERSION"),
                "clientFlavor": client_flavor().await,
            }),
        ))
        .await?
        .json()
        .await?;
        Self::store_login(resp.id, resp.token, resp.refresh_token).await?;
        Ok(())
    }

    #[cfg(feature = "hykb")]
    /// Legacy migration: bind the pending HYKB identity to an existing email account
    /// after verifying its email + password.
    pub async fn login_hykb_claim(hykb_token: &str, email: &str, password: &str) -> Result<()> {
        let resp: LoginResp = recv_raw(Self::post(
            "/login/hykb/claim",
            &json!({
                "hykbToken": hykb_token,
                "email": email,
                "password": password,
                "clientVersion": env!("CARGO_PKG_VERSION"),
                "clientFlavor": client_flavor().await,
            }),
        ))
        .await?
        .json()
        .await?;
        Self::store_login(resp.id, resp.token, resp.refresh_token).await?;
        Ok(())
    }

    #[cfg(feature = "hykb")]
    /// Bind a HYKB account to the currently logged-in account.
    pub async fn bind_hykb(uid: i64, access_token: &str) -> Result<()> {
        recv_raw(Self::post(
            "/me/bind-hykb",
            &json!({
                "hykbUid": uid,
                "accessToken": access_token,
            }),
        ))
        .await?;
        Ok(())
    }

    #[cfg(feature = "hykb")]
    /// Unbind the HYKB account from the current account.
    pub async fn unbind_hykb() -> Result<()> {
        recv_raw(Self::post("/me/unbind-hykb", &())).await?;
        Ok(())
    }

    #[cfg(feature = "hykb")]
    /// Request transferring the current HYKB-only account onto an existing email
    /// account. Sends a confirmation email to `email`; the move happens only once
    /// the user clicks the link. Returns Ok even when the email is unregistered
    /// (the server intentionally does not reveal whether it exists).
    pub async fn transfer_request(email: &str) -> Result<()> {
        recv_raw(Self::post("/me/transfer-request", &json!({ "email": email }))).await?;
        Ok(())
    }

    /// Mint a one-time ticket that logs a browser into the current account. The
    /// client's own tokens never leave the app; the web redeems the ticket for
    /// an independent session.
    pub async fn create_web_ticket() -> Result<String> {
        #[derive(Deserialize)]
        struct Resp {
            ticket: String,
        }
        let resp: Resp = recv_raw(Self::post("/me/web-ticket", &())).await?.json().await?;
        Ok(resp.ticket)
    }

    pub async fn get_me() -> Result<User> {
        // Accounts not bound to a HYKB account are valid: anti-addiction is
        // covered by a native HYKB login performed at sign-in (used for the
        // SDK's enforcement, not bound to the account), and the player may
        // bind HYKB later from the profile page.
        Ok(recv_raw(Self::get("/me")).await?.json().await?)
    }

    pub async fn authenticated_token() -> Result<String> {
        Self::get_me().await?;
        CLIENT_TOKEN.load().as_ref().clone().context("not logged in")
    }

    pub async fn best_record(id: i32) -> Result<SimpleRecord> {
        Ok(recv_raw(Self::get(format!("/record/best/{id}"))).await?.json().await?)
    }

    pub async fn upload_file(name: &str, bytes: Vec<u8>) -> Result<String> {
        #[derive(Deserialize)]
        struct Resp {
            id: String,
        }
        let resp: Resp = recv_raw(Self::request(Method::POST, format!("/upload/{name}")).body(bytes))
            .await?
            .json()
            .await?;
        Ok(resp.id)
    }

    /// Returns `Some(modified)` (the `Last-Modified` header) if the terms have
    /// been updated since `modified`, or `None` if unchanged. Uses HEAD so the
    /// ~9 KB body is never downloaded — change detection relies solely on the
    /// `Last-Modified` header.
    pub async fn fetch_terms(modified: Option<&str>) -> Result<Option<String>> {
        let mut req = Self::request(Method::HEAD, format!("/terms/{}.txt", client_locale()));
        if let Some(modified) = modified {
            req = req.header(header::IF_MODIFIED_SINCE, header::HeaderValue::from_str(modified)?);
        }
        let resp = send_raw(req).await?;
        if resp.status() == StatusCode::NOT_MODIFIED {
            return Ok(None);
        }
        if !resp.status().is_success() {
            bail!("failed to fetch terms: {:?}", resp.status());
        }
        let new_modified = resp
            .headers()
            .get(header::LAST_MODIFIED)
            .and_then(|it| it.to_str().ok())
            .map(str::to_owned)
            .ok_or_else(|| anyhow!("invalid last-modified header"))?;
        debug!("{new_modified} {modified:?}");
        if Some(new_modified.as_str()) == modified {
            // That mother fucker qiniu does not return NOT_MODIFIED
            return Ok(None);
        }
        Ok(Some(new_modified))
    }
}

#[must_use]
pub struct QueryBuilder<T> {
    queries: HashMap<Cow<'static, str>, Cow<'static, str>>,
    page: Option<u64>,
    suffix: &'static str,
    _phantom: PhantomData<T>,
}

impl<T: Object> QueryBuilder<T> {
    pub fn query(mut self, key: impl Into<Cow<'static, str>>, value: impl Into<Cow<'static, str>>) -> Self {
        self.queries.insert(key.into(), value.into());
        self
    }

    #[inline]
    pub fn order(self, order: impl Into<Cow<'static, str>>) -> Self {
        self.query("order", order)
    }

    #[inline]
    pub fn tags(self, tags: impl Into<Cow<'static, str>>) -> Self {
        self.query("tags", tags)
    }

    #[inline]
    pub fn search(self, search: impl Into<Cow<'static, str>>) -> Self {
        self.query("search", search)
    }

    #[inline]
    pub fn page_num(self, page_num: u64) -> Self {
        self.query("pageNum", page_num.to_string())
    }

    #[inline]
    pub fn suffix(mut self, suffix: &'static str) -> Self {
        self.suffix = suffix;
        self
    }

    pub fn page(mut self, page: u64) -> Self {
        self.page = Some(page);
        self
    }

    pub async fn send(mut self) -> Result<(Vec<T>, u64)> {
        self.queries.insert("page".into(), (self.page.unwrap_or(0) + 1).to_string().into());
        #[derive(Deserialize)]
        struct PagedResult<T> {
            count: u64,
            results: Vec<T>,
        }
        let res: PagedResult<T> = recv_raw(Client::get(format!("/{}{}", T::QUERY_PATH, self.suffix)).query(&self.queries))
            .await?
            .json()
            .await?;
        Ok((res.results, res.count))
    }
}
