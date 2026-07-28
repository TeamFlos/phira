//! Banned-word detection.
//!
//! [`CensorManager`] fetches the word list on first use (or when the cache is
//! older than a week), builds a `daachorse` double-array Aho-Corasick automaton,
//! and caches the *serialized* automaton (zstd-compressed) to disk. Caching the
//! built automaton rather than the raw words turns a ~300ms startup rebuild
//! (55k patterns) into a ~10ms deserialize — important for mobile cold starts.

#![allow(dead_code)]

#[rustfmt::skip]
#[cfg(closed)]
mod inner;

use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use daachorse::{CharwiseDoubleArrayAhoCorasick, CharwiseDoubleArrayAhoCorasickBuilder, MatchKind};
use serde::Deserialize;
use tokio::sync::OnceCell;

const REFRESH_INTERVAL: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const ZSTD_LEVEL: i32 = 19;
const WORD_SEPARATOR: char = '|';
const CACHE_MAGIC: &[u8; 4] = b"CSAC";
/// Bump whenever the daachorse version or our serialization changes, so an
/// incompatible cache triggers a rebuild instead of an unchecked deserialize (UB).
const CACHE_VERSION: u8 = 1;

type Matcher = CharwiseDoubleArrayAhoCorasick<u32>;

static INSTANCE: OnceCell<CensorManager> = OnceCell::const_new();

/// Endpoint response; `data` is base64 of the `|`-separated word list.
#[derive(Deserialize)]
struct WordsResponse {
    code: i32,
    #[allow(dead_code)]
    msg: String,
    data: String,
}

pub struct CensorManager {
    ac: Matcher,
}

impl CensorManager {
    pub fn cache_path() -> Result<PathBuf> {
        Ok(format!("{}/censor.bin", crate::dir::cache()?).into())
    }

    /// Initialize (or return) the singleton with a specific cache path.
    /// If already initialized, the passed path is ignored (first caller wins).
    pub async fn init(url: String) -> Result<&'static CensorManager> {
        // get_or_try_init leaves the cell empty on error, so a failed init retries.
        INSTANCE.get_or_try_init(|| async move { Self::new(url).await }).await
    }

    /// Construct with a specific cache file. Uses a fresh cache (<1 week) directly,
    /// otherwise fetches and rebuilds; on fetch failure falls back to a stale cache.
    pub async fn new(url: String) -> Result<Self> {
        let cache_path = Self::cache_path()?;
        tracing::debug!("initializing censor manager");
        let ac = load_or_fetch(&cache_path, &url).await?;
        tracing::info!("censor manager ready ({} KB)", ac.heap_bytes() / 1024);
        Ok(Self { ac })
    }

    /// Check if the given text contains any banned words. Case-insensitive.
    pub fn check(&self, text: &str) -> Result<()> {
        let hay = text.to_ascii_lowercase();
        let hit = self.ac.leftmost_find_iter(&hay).next();
        if hit.is_some() {
            bail!("{}", crate::ttl!("contains-banned-words"));
        }
        Ok(())
    }

    /// Approximate heap size of the automaton, in bytes.
    pub fn heap_bytes(&self) -> usize {
        self.ac.heap_bytes()
    }
}

/// Stale if the file is missing or its mtime is older than the refresh interval.
fn is_stale(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return true;
    };
    let Ok(modified) = meta.modified() else {
        return true;
    };
    SystemTime::now()
        .duration_since(modified)
        .map(|age| age > REFRESH_INTERVAL)
        .unwrap_or(false)
}

async fn load_or_fetch(path: &Path, url: &str) -> Result<Matcher> {
    if !is_stale(path) {
        tracing::debug!("censor cache is fresh, loading from {}", path.display());
        match load_cache(path) {
            Ok(ac) => return Ok(ac),
            Err(e) => tracing::warn!("failed to load censor cache: {e:#}, rebuilding"),
        }
    } else {
        tracing::debug!("censor cache missing or stale, refetching");
    }
    fetch_build_cache(path, url).await
}

async fn fetch_build_cache(path: &Path, url: &str) -> Result<Matcher> {
    match fetch_words(url).await {
        Ok(words) => {
            let ac = build_matcher(&words)?;
            // A cache write failure only slows the next cold start; not fatal.
            if let Err(e) = write_cache(path, &ac) {
                tracing::warn!("failed to write censor cache: {e:#}");
            } else {
                tracing::info!("cached censor automaton to {}", path.display());
            }
            Ok(ac)
        }
        Err(e) => {
            if let Ok(ac) = load_cache(path) {
                tracing::warn!("failed to fetch words: {e:#}, falling back to existing cache");
                return Ok(ac);
            }
            Err(e).context("failed to fetch word list and no usable cache available")
        }
    }
}

async fn fetch_words(url: &str) -> Result<Vec<String>> {
    let resp: WordsResponse = reqwest::get(url)
        .await
        .context("request to word list endpoint failed")?
        .error_for_status()
        .context("word list endpoint returned error status")?
        .json()
        .await
        .context("failed to decode word list response as JSON")?;

    if resp.code != 200 {
        bail!("word list endpoint returned business code {}", resp.code);
    }

    let decoded = STANDARD
        .decode(resp.data.as_bytes())
        .context("failed to base64-decode word list payload")?;
    let text = String::from_utf8(decoded).context("word list payload is not valid UTF-8")?;
    let words = parse_words(&text);
    anyhow::ensure!(!words.is_empty(), "fetched word list is empty");
    tracing::info!("fetched {} censor words", words.len());
    Ok(words)
}

/// Split on `|` into a trimmed, deduplicated, ASCII-lowercased word list.
fn parse_words(text: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    text.split(WORD_SEPARATOR)
        .map(str::trim)
        .filter(|w| !w.is_empty())
        .map(str::to_ascii_lowercase)
        .filter(|w| seen.insert(w.clone()))
        .collect()
}

fn build_matcher<I, P>(words: I) -> Result<Matcher>
where
    I: IntoIterator<Item = P>,
    P: AsRef<str>,
{
    CharwiseDoubleArrayAhoCorasickBuilder::new()
        .match_kind(MatchKind::LeftmostLongest)
        .build(words)
        .map_err(|e| anyhow::anyhow!("failed to build daachorse automaton: {e}"))
}

fn write_cache(path: &Path, ac: &Matcher) -> Result<()> {
    let serialized = ac.serialize();
    let compressed = zstd::encode_all(serialized.as_slice(), ZSTD_LEVEL).context("failed to zstd-compress automaton")?;

    let mut buf = Vec::with_capacity(compressed.len() + CACHE_MAGIC.len() + 1);
    buf.extend_from_slice(CACHE_MAGIC);
    buf.push(CACHE_VERSION);
    buf.extend_from_slice(&compressed);

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).with_context(|| format!("failed to create cache dir {}", parent.display()))?;
        }
    }
    std::fs::write(path, &buf).with_context(|| format!("failed to write cache file {}", path.display()))?;
    Ok(())
}

fn load_cache(path: &Path) -> Result<Matcher> {
    let raw = std::fs::read(path).with_context(|| format!("failed to read cache file {}", path.display()))?;

    let header_len = CACHE_MAGIC.len() + 1;
    anyhow::ensure!(raw.len() > header_len, "cache file is truncated");
    anyhow::ensure!(&raw[..CACHE_MAGIC.len()] == CACHE_MAGIC, "cache magic mismatch");
    anyhow::ensure!(raw[CACHE_MAGIC.len()] == CACHE_VERSION, "cache version mismatch (found {}, expected {CACHE_VERSION})", raw[CACHE_MAGIC.len()]);

    let serialized = zstd::decode_all(&raw[header_len..]).context("failed to zstd-decompress cache")?;
    // SAFETY: magic and version were validated above, so these bytes were written
    // by this program under the current format.
    let (ac, rest) = unsafe { CharwiseDoubleArrayAhoCorasick::deserialize_unchecked(&serialized) };
    anyhow::ensure!(rest.is_empty(), "cache has trailing bytes after automaton");
    tracing::debug!("loaded censor automaton from cache ({} states)", ac.num_states());
    Ok(ac)
}

pub fn check_text(text: &str) -> Result<()> {
    check_texts([text])
}

/// Synchronously check texts against the banned-word list for *local* edits
/// (e.g. renaming a collection offline), returning `Err` on the first one that
/// contains a banned word. No-op (always `Ok`) unless the `aa` feature is
/// enabled.
///
/// If the automaton hasn't finished loading yet (a brief window right after
/// startup, before [`preload`] completes) the text is allowed through: local
/// data isn't safety-critical on its own, and anything later uploaded is still
/// caught by server-side moderation.
pub fn check_texts<'a>(texts: impl IntoIterator<Item = &'a str>) -> Result<()> {
    if !cfg!(feature = "hykb") {
        return Ok(());
    }
    let Some(manager) = INSTANCE.get() else {
        tracing::warn!("censor manager not ready yet, skipping local check");
        return Ok(());
    };
    for text in texts {
        manager.check(text)?;
    }
    Ok(())
}

#[cfg(closed)]
pub use inner::preload;

#[cfg(not(closed))]
pub async fn preload() {}
