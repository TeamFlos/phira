use anyhow::{ensure, Result};
use arc_swap::ArcSwap;
use chrono::{DateTime, TimeDelta, Utc};
use reqwest::{header, Request, RequestBuilder, Response, StatusCode, Url};
use std::{collections::HashSet, future::Future, sync::Arc};
use tokio::sync::Mutex;

pub(super) type TokenPair = (String, String);

pub(super) struct Credentials {
    pub pair: TokenPair,
    expires_at: Option<DateTime<Utc>>,
}

impl Credentials {
    pub fn restored(pair: TokenPair) -> Self {
        Self { pair, expires_at: None }
    }

    pub fn issued(pair: TokenPair, expires_at: Option<DateTime<Utc>>) -> Self {
        // Older servers may omit expireAt. Recheck those credentials after a short interval.
        Self {
            pair,
            expires_at: Some(expires_at.unwrap_or_else(|| Utc::now() + TimeDelta::minutes(5))),
        }
    }

    pub fn needs_refresh(&self, now: DateTime<Utc>) -> bool {
        self.expires_at.is_none_or(|expiration| expiration <= now + TimeDelta::minutes(1))
    }
}

pub(super) struct Session {
    pub credentials: ArcSwap<Credentials>,
    issued_tokens: std::sync::Mutex<HashSet<String>>,
    refresh_lock: Mutex<()>,
}

impl Session {
    pub fn new(credentials: Credentials) -> Self {
        Self {
            issued_tokens: std::sync::Mutex::new(HashSet::from([credentials.pair.0.clone()])),
            credentials: ArcSwap::from_pointee(credentials),
            refresh_lock: Mutex::new(()),
        }
    }

    fn owns_token(&self, token: &str) -> bool {
        self.issued_tokens.lock().unwrap().contains(token)
    }

    pub fn store_credentials(&self, credentials: Arc<Credentials>) {
        self.issued_tokens.lock().unwrap().insert(credentials.pair.0.clone());
        self.credentials.store(credentials);
    }

    pub async fn authorize<F, Fut>(&self, token: &str, mode: RefreshMode, refresh: F) -> Result<Arc<Credentials>>
    where
        F: FnOnce(Arc<Credentials>) -> Fut,
        Fut: Future<Output = Result<Arc<Credentials>>>,
    {
        ensure!(self.owns_token(token), "session changed before sending request");
        let credentials = self.credentials.load_full();
        let refresh_needed = match mode {
            RefreshMode::BeforeSend => credentials.needs_refresh(Utc::now()),
            RefreshMode::Rejected => credentials.pair.0 == token,
        };
        if refresh_needed {
            self.refresh(&credentials, false, refresh).await
        } else {
            Ok(credentials)
        }
    }

    /// The refresh callback publishes the new credentials before returning.
    pub async fn refresh<F, Fut>(&self, observed: &Arc<Credentials>, force: bool, refresh: F) -> Result<Arc<Credentials>>
    where
        F: FnOnce(Arc<Credentials>) -> Fut,
        Fut: Future<Output = Result<Arc<Credentials>>>,
    {
        let _guard = self.refresh_lock.lock().await;
        let credentials = self.credentials.load_full();
        // Another request may already have refreshed this session while we waited.
        if !force && !Arc::ptr_eq(&credentials, observed) {
            return Ok(credentials);
        }
        refresh(credentials).await
    }
}

pub(super) fn accepts_session(url: &Url, api_url: &str) -> bool {
    let Ok(api_url) = Url::parse(api_url) else { return false };
    let path = url.path();
    if url.origin() == api_url.origin() {
        return !matches!(path, "/login" | "/register" | "/check-update" | "/font-bold" | "/terms")
            && !path.starts_with("/login/")
            && !path.starts_with("/terms/");
    }
    url.scheme() == "https"
        && url.port().is_none()
        && (url.host_str() == Some("files.phira.cn") || (url.host_str() == Some("api.phira.cn") && path.starts_with("/files/")))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RefreshMode {
    BeforeSend,
    Rejected,
}

fn set_bearer(request: &mut Request, token: &str) -> Result<()> {
    let mut value = header::HeaderValue::from_str(&format!("Bearer {token}"))?;
    value.set_sensitive(true);
    request.headers_mut().insert(header::AUTHORIZATION, value);
    Ok(())
}

pub(super) async fn send_with_refresh<F, Fut>(request: RequestBuilder, api_url: &str, mut authorize: F) -> Result<Response>
where
    F: FnMut(String, RefreshMode) -> Fut,
    Fut: Future<Output = Result<String>>,
{
    let (client, request) = request.build_split();
    let mut request = request?;
    let token = accepts_session(request.url(), api_url)
        .then(|| {
            request
                .headers()
                .get(header::AUTHORIZATION)?
                .to_str()
                .ok()?
                .strip_prefix("Bearer ")
                .map(str::to_owned)
        })
        .flatten();
    let Some(token) = token else { return Ok(client.execute(request).await?) };
    let token = authorize(token, RefreshMode::BeforeSend).await?;
    set_bearer(&mut request, &token)?;
    let retry = request.try_clone();
    let response = client.execute(request).await?;
    if response.status() == StatusCode::UNAUTHORIZED && accepts_session(response.url(), api_url) {
        if let Some(mut retry) = retry.filter(|retry| response.url().origin() == retry.url().origin()) {
            let token = authorize(token, RefreshMode::Rejected).await?;
            set_bearer(&mut retry, &token)?;
            // Replay only once, preserving the method, URL, headers and body.
            return Ok(client.execute(retry).await?);
        }
    }
    Ok(response)
}
