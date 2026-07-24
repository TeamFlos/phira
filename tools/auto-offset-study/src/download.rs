use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::Cursor,
    path::Path,
    time::Duration,
};

use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use prpr::info::ChartInfo;

use crate::{
    api::{send_with_retries, PagedResult, RemoteChart},
    config::{Cli, API_URL},
};

pub async fn ensure_samples(cli: &Cli) -> Result<()> {
    let charts_dir = cli.root.join("charts");
    let existing = cached_chart_ids(&charts_dir)?;
    if existing.len() >= cli.samples && !cli.download {
        println!("using {} cached charts", existing.len());
        return Ok(());
    }

    let target = if cli.download { existing.len() + cli.samples } else { cli.samples };
    println!("fetching charts until cache reaches {target} samples");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(cli.request_timeout_ms))
        .build()?;
    let mut seen = existing;

    for page in 1..=cli.pages {
        if seen.len() >= target {
            break;
        }
        let request = || {
            client.get(format!("{API_URL}/chart")).query(&[
                ("page", page.to_string()),
                ("pageNum", cli.page_num.to_string()),
                ("order", cli.order.clone()),
                ("type", "-1".to_owned()),
                ("rating", "0,16".to_owned()),
            ])
        };
        let res: PagedResult<RemoteChart> = send_with_retries(request, cli.retries).await?.error_for_status()?.json().await?;

        for chart in res.results {
            if seen.len() >= target {
                break;
            }
            if seen.contains(&chart.id) {
                continue;
            }
            match download_chart(&client, &charts_dir, &chart, cli.retries).await {
                Ok(()) => {
                    seen.insert(chart.id);
                    println!("downloaded {} ({}/{target})", chart.id, seen.len());
                }
                Err(err) => eprintln!("skip {}: {err:#}", chart.id),
            }
        }
    }

    if seen.len() < cli.samples {
        bail!("only {} charts cached; increase --pages or check network", seen.len());
    }
    Ok(())
}

pub fn cached_chart_ids(charts_dir: &Path) -> Result<BTreeSet<i32>> {
    let mut ids = BTreeSet::new();
    if !charts_dir.exists() {
        return Ok(ids);
    }
    for entry in fs::read_dir(charts_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        if let Some(id) = entry.file_name().to_str().and_then(|it| it.parse::<i32>().ok()) {
            ids.insert(id);
        }
    }
    Ok(ids)
}

async fn download_chart(client: &reqwest::Client, charts_dir: &Path, chart: &RemoteChart, retries: usize) -> Result<()> {
    let tmp_dir = charts_dir.join(format!("{}.tmp", chart.id));
    let final_dir = charts_dir.join(chart.id.to_string());
    if final_dir.exists() {
        return Ok(());
    }
    if tmp_dir.exists() {
        fs::remove_dir_all(&tmp_dir)?;
    }
    fs::create_dir_all(&tmp_dir)?;

    let bytes = fetch_bytes(client, &chart.file, retries).await?;
    unzip_bytes(&bytes, &tmp_dir).context("failed to unzip chart archive")?;

    let info_path = tmp_dir.join("info.yml");
    let mut info: ChartInfo = serde_yaml::from_reader(File::open(&info_path)?)?;
    info.id = Some(chart.id);
    serde_yaml::to_writer(File::create(&info_path)?, &info)?;

    fs::rename(tmp_dir, final_dir)?;
    Ok(())
}

async fn fetch_bytes(client: &reqwest::Client, url: &str, retries: usize) -> Result<Vec<u8>> {
    let response = send_with_retries(|| client.get(url), retries).await?.error_for_status()?;
    if response.url().as_str().starts_with("anys://") {
        bail!("anys:// redirect is not supported by this study tool")
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        bytes.extend_from_slice(&chunk?);
    }
    Ok(bytes)
}

fn unzip_bytes(bytes: &[u8], out_dir: &Path) -> Result<()> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        if entry.is_dir() {
            continue;
        }
        let enclosed = entry.enclosed_name().context("invalid zip path")?;
        let out_path = out_dir.join(enclosed);
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = File::create(out_path)?;
        std::io::copy(&mut entry, &mut out)?;
    }
    Ok(())
}
