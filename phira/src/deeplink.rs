prpr_l10n::tl_file!("import" itl);

use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use macroquad::prelude::*;
use prpr::{
    ext::{semi_black, RectExt},
    task::Task,
    ui::{DRectButton, Ui},
};
use std::{
    io::{Seek, SeekFrom, Write},
    sync::{Arc, LazyLock, Mutex},
};
use tempfile::tempfile;
use url::Url;

use crate::client::{basic_client_builder, API_URL};

/// Upper bound for deeplink downloads, protecting against untrusted sources
/// serving arbitrarily large files.
pub const MAX_DEEPLINK_DOWNLOAD: u64 = 100 << 20;

static PENDING_DEEPLINK: Mutex<Option<String>> = Mutex::new(None);

/// Stores a deeplink payload injected by the platform layer (Android JNI,
/// desktop argv). Last one wins.
pub fn set_deeplink(input: impl Into<String>) {
    *PENDING_DEEPLINK.lock().unwrap() = Some(input.into());
}

/// Takes the pending deeplink, if any. Consumed by `MainScene::update`.
pub fn take_deeplink() -> Option<String> {
    PENDING_DEEPLINK.lock().unwrap().take()
}

static OFFICIAL_HOST: LazyLock<String> = LazyLock::new(|| {
    Url::parse(API_URL)
        .ok()
        .and_then(|url| url.host_str().map(ToOwned::to_owned))
        .unwrap_or_else(|| API_URL.trim_start_matches("https://").trim_end_matches('/').to_owned())
});

/// The host charts files are expected to come from to count as official.
pub fn official_host() -> &'static str {
    &OFFICIAL_HOST
}

/// Web wrapper host (`https://phira.moe/dlink/...?src=<real url>`), kept in
/// sync with the Android intent-filter on the same path.
const DLINK_HOST: &str = "phira.moe";
const DLINK_PATH: &str = "/dlink";

#[derive(Clone)]
pub struct DeepLinkTarget {
    pub url: Url,
    pub official: bool,
}

fn is_official(url: &Url) -> bool {
    url.host_str() == Some(&OFFICIAL_HOST) && url.port().is_none()
}

/// Whether the URL is a transport wrapper (`phira://...` or
/// `https://phira.moe/dlink/...`) whose real target sits in the `src` query
/// parameter.
fn is_wrapper(url: &Url) -> bool {
    url.scheme() == "phira" || (matches!(url.scheme(), "http" | "https") && url.host_str() == Some(DLINK_HOST) && url.path().starts_with(DLINK_PATH))
}

/// Resolves a raw `http(s)` chart file URL, a `phira://import?src=<url>` or an
/// `https://phira.moe/dlink/import?src=<url>` wrapper, into the final download
/// target. The indirection is unwrapped exactly once and must lead to
/// `http(s)`; the official-source flag is judged on the *final* URL, so a
/// wrapper pointing anywhere but the official host still warns.
pub fn parse_deeplink(input: &str) -> Result<DeepLinkTarget> {
    let mut url = Url::parse(input.trim())?;
    if is_wrapper(&url) {
        let src = url
            .query_pairs()
            .find(|(key, _)| key == "src")
            .map(|(_, value)| value.into_owned())
            .context("missing src")?;
        url = Url::parse(&src).context("invalid src")?;
        if is_wrapper(&url) {
            bail!("nested wrapper");
        }
    }
    if !matches!(url.scheme(), "http" | "https") {
        bail!("unsupported scheme");
    }
    let official = is_official(&url);
    Ok(DeepLinkTarget { url, official })
}

/// Overlay state for an in-progress deeplink download, modeled after
/// `SongScene`'s `Downloading`. Dropping this struct cancels the transfer.
pub struct DeepLinkDownload {
    prog: Arc<Mutex<Option<f32>>>,
    loading_last: f32,
    cancel_btn: DRectButton,
    task: Task<Result<std::fs::File>>,
}

pub fn start_deeplink_download(target: DeepLinkTarget) -> Result<DeepLinkDownload> {
    let prog = Arc::new(Mutex::new(None));
    let prog_wk = Arc::downgrade(&prog);
    Ok(DeepLinkDownload {
        prog,
        loading_last: 0.,
        cancel_btn: DRectButton::new(),
        task: Task::new(async move {
            let mut file = tempfile()?;
            let Some(prog) = prog_wk.upgrade() else {
                bail!("cancelled");
            };
            let client = basic_client_builder().build()?;
            let res = client
                .get(target.url)
                .send()
                .await
                .context("failed to send request")?
                .error_for_status()?;
            let size = res.content_length();
            let mut stream = res.bytes_stream();
            let mut count: u64 = 0;
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.context("failed to read response body")?;
                count += chunk.len() as u64;
                if count > MAX_DEEPLINK_DOWNLOAD {
                    bail!(itl!("deeplink-too-large"));
                }
                file.write_all(&chunk)?;
                if let Some(size) = size {
                    *prog.lock().unwrap() = Some(count.min(size) as f32 / size as f32);
                }
                if prog_wk.strong_count() == 1 {
                    // cancelled by the user
                    bail!("cancelled");
                }
            }
            file.seek(SeekFrom::Start(0))?;
            Ok(file)
        }),
    })
}

impl DeepLinkDownload {
    /// Returns `true` when the user tapped the cancel button.
    pub fn touch(&mut self, touch: &Touch, t: f32) -> bool {
        self.cancel_btn.touch(touch, t)
    }

    pub fn render(&mut self, ui: &mut Ui, t: f32) {
        ui.fill_rect(ui.screen_rect(), semi_black(0.6));
        ui.loading(0., -0.06, t, WHITE, (*self.prog.lock().unwrap(), &mut self.loading_last));
        ui.text(itl!("deeplink-downloading")).pos(0., 0.02).anchor(0.5, 0.).size(0.6).draw();
        let r = ui.text(ttl!("cancel")).pos(0., 0.12).anchor(0.5, 0.).size(0.7).measure().feather(0.02);
        self.cancel_btn.render_text(ui, r, t, ttl!("cancel"), 0.6, true);
    }

    pub fn take_result(&mut self) -> Option<Result<std::fs::File>> {
        self.task.take()
    }
}
