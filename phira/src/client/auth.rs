use anyhow::Result;
use arc_swap::ArcSwap;
use reqwest::{header, RequestBuilder, Response, StatusCode, Url};
use std::{collections::HashSet, future::Future, sync::Arc};
use tokio::sync::Mutex;

pub(super) type TokenPair = (String, String);

pub(super) struct Session {
    pub tokens: ArcSwap<TokenPair>,
    issued_tokens: std::sync::Mutex<HashSet<String>>,
    refresh_lock: Mutex<()>,
}

impl Session {
    pub fn new(tokens: TokenPair) -> Self {
        Self {
            issued_tokens: std::sync::Mutex::new(HashSet::from([tokens.0.clone()])),
            tokens: ArcSwap::from_pointee(tokens),
            refresh_lock: Mutex::new(()),
        }
    }

    pub fn owns_token(&self, token: &str) -> bool {
        self.issued_tokens.lock().unwrap().contains(token)
    }

    pub fn store_tokens(&self, tokens: Arc<TokenPair>) {
        self.issued_tokens.lock().unwrap().insert(tokens.0.clone());
        self.tokens.store(tokens);
    }

    pub async fn refresh<F, Fut>(&self, rejected_token: &str, refresh: F) -> Result<Arc<TokenPair>>
    where
        F: FnOnce(Arc<TokenPair>) -> Fut,
        Fut: Future<Output = Result<TokenPair>>,
    {
        let _guard = self.refresh_lock.lock().await;
        let tokens = self.tokens.load_full();
        // Another request may already have refreshed this session while we waited.
        if tokens.0 != rejected_token {
            return Ok(tokens);
        }
        let tokens = Arc::new(refresh(tokens).await?);
        self.store_tokens(Arc::clone(&tokens));
        Ok(tokens)
    }
}

pub(super) fn can_refresh(url: &Url, api_url: &str) -> bool {
    let Ok(api_url) = Url::parse(api_url) else { return false };
    url.origin() == api_url.origin() && url.path() != "/login" && !url.path().starts_with("/login/") && url.path() != "/register"
}

pub(super) async fn send_with_refresh<F, Fut>(request: RequestBuilder, api_url: &str, refresh: F) -> Result<Response>
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = Result<Option<String>>>,
{
    let (client, request) = request.build_split();
    let request = request?;
    let token = can_refresh(request.url(), api_url)
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
    let retry = token.as_ref().and_then(|_| request.try_clone());
    let response = client.execute(request).await?;
    if response.status() == StatusCode::UNAUTHORIZED {
        if let (Some(token), Some(mut retry)) = (token, retry) {
            if let Some(token) = refresh(token).await? {
                let mut value = header::HeaderValue::from_str(&format!("Bearer {token}"))?;
                value.set_sensitive(true);
                retry.headers_mut().insert(header::AUTHORIZATION, value);
                // Replay only once, preserving the method, URL, headers and body.
                return Ok(client.execute(retry).await?);
            }
        }
    }
    Ok(response)
}
