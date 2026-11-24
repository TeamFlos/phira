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

use crate::client::{basic_client_builder, Chart, Ptr, API_URL};

/// Upper bound for deeplink downloads from untrusted sources.
pub const MAX_DEEPLINK_DOWNLOAD: u64 = 100 << 20;

static PENDING_DEEPLINK: Mutex<Option<String>> = Mutex::new(None);

pub fn set_deeplink(input: impl Into<String>) {
    *PENDING_DEEPLINK.lock().unwrap() = Some(input.into());
}

pub fn take_deeplink() -> Option<String> {
    PENDING_DEEPLINK.lock().unwrap().take()
}

static OFFICIAL_HOST: LazyLock<String> = LazyLock::new(|| {
    Url::parse(API_URL)
        .ok()
        .and_then(|url| url.host_str().map(ToOwned::to_owned))
        .unwrap_or_else(|| API_URL.trim_start_matches("https://").trim_end_matches('/').to_owned())
});

pub fn official_host() -> &'static str {
    &OFFICIAL_HOST
}

const DLINK_HOST: &str = "phira.moe";
const DLINK_PATH: &str = "/dlink/";

#[derive(Clone)]
pub struct DeepLinkTarget {
    pub url: Url,
    pub official: bool,
}

fn is_official(url: &Url) -> bool {
    url.host_str() == Some(&OFFICIAL_HOST) && url.port().is_none()
}

/// The `<action>` segment of an `https://phira.moe/dlink/<action>` wrapper.
fn dlink_action(url: &Url) -> Option<&str> {
    if url.host_str() != Some(DLINK_HOST) {
        return None;
    }
    url.path().strip_prefix(DLINK_PATH)
}

fn query_param(url: &Url, key: &str) -> Option<String> {
    url.query_pairs().into_owned().find(|(k, _)| k == key).map(|(_, v)| v)
}

fn src_target(url: &Url) -> Result<Url> {
    let src = query_param(url, "src").context("missing src")?;
    let target = Url::parse(&src).context("invalid src")?;
    if dlink_action(&target).is_some() {
        bail!("nested wrapper");
    }
    Ok(target)
}

fn chart_id(url: &Url) -> Result<i32> {
    query_param(url, "id").context("missing id")?.parse().context("invalid id")
}

/// What a deeplink asks the app to do.
#[derive(Clone)]
pub enum DeepLink {
    /// Download a chart file and import it. `official` is judged on the final
    /// download URL, after unwrapping any wrapper.
    Import(DeepLinkTarget),
    /// Open the details page of the chart with this id.
    Chart(i32),
}

/// Accepted forms:
/// - `phira://chart?id=<id>` or `https://phira.moe/dlink/chart?id=<id>`
/// - `phira://import?src=<url>` or `https://phira.moe/dlink/import?src=<url>`
pub fn parse_deeplink(input: &str) -> Result<DeepLink> {
    let url = Url::parse(input.trim())?;
    let action = if url.scheme() == "phira" {
        url.host_str().context("missing action")?
    } else if let Some(action) = dlink_action(&url) {
        action
    } else {
        bail!("unsupported scheme");
    };
    match action {
        "import" => Ok(DeepLink::Import(download_target(src_target(&url)?)?)),
        "chart" => Ok(DeepLink::Chart(chart_id(&url)?)),
        _ => bail!("unknown action"),
    }
}

fn download_target(url: Url) -> Result<DeepLinkTarget> {
    if !matches!(url.scheme(), "http" | "https") {
        bail!("unsupported scheme");
    }
    Ok(DeepLinkTarget {
        official: is_official(&url),
        url,
    })
}

/// Overlay for an in-progress deeplink download. Dropping it cancels the transfer.
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

/// Overlay while a chart deeplink fetches the chart's details. Dropping it
/// cancels the fetch.
pub struct DeepLinkChartOpening {
    cancel_btn: DRectButton,
    task: Task<Result<Arc<Chart>>>,
}

pub fn start_chart_opening(id: i32) -> DeepLinkChartOpening {
    DeepLinkChartOpening {
        cancel_btn: DRectButton::new(),
        task: Task::new(async move { Ptr::<Chart>::new(id).fetch().await }),
    }
}

impl DeepLinkChartOpening {
    /// Returns `true` when the user tapped the cancel button.
    pub fn touch(&mut self, touch: &Touch, t: f32) -> bool {
        self.cancel_btn.touch(touch, t)
    }

    pub fn render(&mut self, ui: &mut Ui, t: f32) {
        ui.fill_rect(ui.screen_rect(), semi_black(0.6));
        ui.loading(0., -0.06, t, WHITE, ());
        ui.text(itl!("deeplink-opening")).pos(0., 0.02).anchor(0.5, 0.).size(0.6).draw();
        let r = ui.text(ttl!("cancel")).pos(0., 0.12).anchor(0.5, 0.).size(0.7).measure().feather(0.02);
        self.cancel_btn.render_text(ui, r, t, ttl!("cancel"), 0.6, true);
    }

    pub fn take_result(&mut self) -> Option<Result<Arc<Chart>>> {
        self.task.take()
    }
}
