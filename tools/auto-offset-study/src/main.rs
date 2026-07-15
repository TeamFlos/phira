use anyhow::{bail, Context, Result};
use clap::Parser;
use futures_util::{stream, StreamExt};
use plotters::prelude::*;
use prpr::{
    core::{BpmList, Chart, NoteKind, Triple},
    fs::{fs_from_file, load_info, FileSystem},
    info::{ChartFormat, ChartInfo},
    parse::{parse_pec, parse_phigros, parse_rpe},
};
use prpr_auto_offset::{AlignConfig, AutoOffsetNoteKind, NoteEvent, NoteGaussian, PreprocessedNoteGaussian, Signal, SuperFlux};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, HashMap},
    fs::{self, File},
    io::{Cursor, Write},
    path::{Path, PathBuf},
    time::Duration,
};

const API_URL: &str = "https://phira.5wyxi.com";
const DEFAULT_ROOT: &str = "data/auto-offset-study";

#[derive(Parser)]
#[command(name = "prpr-auto-offset-study")]
#[command(about = "Download chart samples and study auto-offset energy/correlation relationships")]
struct Cli {
    /// Working directory for cached charts, CSV and plot.
    #[arg(long, default_value = DEFAULT_ROOT)]
    root: PathBuf,

    /// Minimum target number of downloaded/analyzed chart samples.
    #[arg(short, long, default_value_t = 300)]
    samples: usize,

    /// Fetch additional charts even if the cache already has enough samples.
    #[arg(long)]
    download: bool,

    /// Number of remote list pages to scan while looking for downloadable charts.
    #[arg(long, default_value_t = 20)]
    pages: u64,

    /// Number of charts requested per remote page.
    #[arg(long, default_value_t = 30)]
    page_num: u64,

    /// Chart ordering passed to the Phira API.
    #[arg(long, default_value = "-updated")]
    order: String,

    /// Search range in seconds for offset estimation.
    #[arg(long, default_value_t = 0.30)]
    range: f64,

    /// Sampling interval in seconds for the correlation grid.
    #[arg(long, default_value_t = 0.005)]
    interval: f64,

    /// Gaussian blur sigma in seconds for the note signal.
    #[arg(long, default_value_t = 0.02)]
    blur_sigma: f64,

    /// Recompute rows even if results.csv already contains a chart id.
    #[arg(long)]
    recompute: bool,

    /// Number of cached charts to analyze concurrently.
    #[arg(long, default_value_t = default_jobs())]
    jobs: usize,

    /// Allow results.csv to be overwritten with fewer rows than it currently contains.
    #[arg(long)]
    allow_shrink: bool,

    /// Per-request timeout in milliseconds.
    #[arg(long, default_value_t = 8000)]
    request_timeout_ms: u64,

    /// Number of attempts per HTTP request.
    #[arg(long, default_value_t = 10)]
    retries: usize,
}

fn default_jobs() -> usize {
    std::thread::available_parallelism().map_or(4, usize::from).max(1)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteChart {
    id: i32,
    file: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublicChartMetadata {
    id: i32,
    reviewed: Option<bool>,
    stable: Option<bool>,
    ranked: Option<bool>,
    stable_request: Option<bool>,
    rating: Option<f32>,
}

#[derive(Debug, Deserialize)]
struct PagedResult<T> {
    results: Vec<T>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RpeTimingChart {
    #[serde(rename = "META")]
    meta: RpeTimingMeta,
    #[serde(rename = "BPMList")]
    bpm_list: Vec<RpeTimingBpm>,
    judge_line_list: Vec<RpeTimingLine>,
}

#[derive(Deserialize)]
struct RpeTimingMeta {
    offset: f32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RpeTimingBpm {
    start_time: Triple,
    bpm: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RpeTimingLine {
    notes: Option<Vec<RpeTimingNote>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RpeTimingNote {
    start_time: Triple,
    #[serde(rename = "type")]
    kind: u8,
    is_fake: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StudyRow {
    chart_id: i32,
    chart_name: String,
    notes: usize,
    duration_sec: f64,
    search_center_sec: f64,
    suggested_offset_sec: f64,
    lag_sec: f64,
    raw_peak: f64,
    note_energy: f64,
    audio_energy: f64,
    normalized_peak: f64,
    uncorrected_raw_peak: f64,
    uncorrected_normalized_peak: f64,
    fitted_log_raw_peak: f64,
    log_raw_residual: f64,
    empirical_peak_ratio: f64,
    uncorrected_log_raw_residual: f64,
    uncorrected_empirical_peak_ratio: f64,
    chart_reviewed: Option<bool>,
    chart_stable: Option<bool>,
    chart_ranked: Option<bool>,
    chart_stable_request: Option<bool>,
    player_rating: Option<f32>,
    player_rating_score: Option<f32>,
    reliable: bool,
    slide_ratio: Option<f64>,
    preprocessed_suggested_offset_sec: Option<f64>,
    preprocessed_lag_sec: Option<f64>,
    preprocessed_raw_peak: Option<f64>,
    preprocessed_normalized_peak: Option<f64>,
    preprocessed_uncorrected_raw_peak: Option<f64>,
    preprocessed_uncorrected_normalized_peak: Option<f64>,
}

fn csv_escape(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

impl StudyRow {
    fn header() -> &'static str {
        "chart_id,chart_name,notes,duration_sec,search_center_sec,suggested_offset_sec,lag_sec,raw_peak,note_energy,audio_energy,normalized_peak,uncorrected_raw_peak,uncorrected_normalized_peak,fitted_log_raw_peak,log_raw_residual,empirical_peak_ratio,uncorrected_log_raw_residual,uncorrected_empirical_peak_ratio,chart_reviewed,chart_stable,chart_ranked,chart_stable_request,player_rating,player_rating_score,reliable,slide_ratio,preprocessed_suggested_offset_sec,preprocessed_lag_sec,preprocessed_raw_peak,preprocessed_normalized_peak,preprocessed_uncorrected_raw_peak,preprocessed_uncorrected_normalized_peak"
    }

    fn to_csv(&self) -> String {
        format!(
            "{},{},{},{:.6},{:.6},{:.6},{:.6},{:.9},{:.9},{:.9},{:.9},{:.9},{:.9},{:.9},{:.9},{:.9},{:.9},{:.9},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            self.chart_id,
            csv_escape(&self.chart_name),
            self.notes,
            self.duration_sec,
            self.search_center_sec,
            self.suggested_offset_sec,
            self.lag_sec,
            self.raw_peak,
            self.note_energy,
            self.audio_energy,
            self.normalized_peak,
            self.uncorrected_raw_peak,
            self.uncorrected_normalized_peak,
            self.fitted_log_raw_peak,
            self.log_raw_residual,
            self.empirical_peak_ratio,
            self.uncorrected_log_raw_residual,
            self.uncorrected_empirical_peak_ratio,
            csv_optional_bool(self.chart_reviewed),
            csv_optional_bool(self.chart_stable),
            csv_optional_bool(self.chart_ranked),
            csv_optional_bool(self.chart_stable_request),
            csv_optional_f32(self.player_rating),
            csv_optional_f32(self.player_rating_score),
            self.reliable,
            csv_optional_f64(self.slide_ratio),
            csv_optional_f64(self.preprocessed_suggested_offset_sec),
            csv_optional_f64(self.preprocessed_lag_sec),
            csv_optional_f64(self.preprocessed_raw_peak),
            csv_optional_f64(self.preprocessed_normalized_peak),
            csv_optional_f64(self.preprocessed_uncorrected_raw_peak),
            csv_optional_f64(self.preprocessed_uncorrected_normalized_peak)
        )
    }

    fn apply_metadata(&mut self, metadata: &PublicChartMetadata) {
        self.chart_reviewed = metadata.reviewed;
        self.chart_stable = metadata.stable;
        self.chart_ranked = metadata.ranked;
        self.chart_stable_request = metadata.stable_request;
        self.player_rating = metadata.rating;
        self.player_rating_score = metadata.rating.map(|rating| rating * 5.0);
    }

    fn apply_preprocessed_score(&mut self, score: PreprocessedScore) {
        self.preprocessed_suggested_offset_sec = Some(score.suggested_offset_sec);
        self.preprocessed_lag_sec = Some(score.lag_sec);
        self.preprocessed_raw_peak = Some(score.raw_peak);
        self.preprocessed_normalized_peak = Some(score.normalized_peak);
        self.preprocessed_uncorrected_raw_peak = Some(score.uncorrected_raw_peak);
        self.preprocessed_uncorrected_normalized_peak = Some(score.uncorrected_normalized_peak);
    }
}

#[derive(Debug, Clone, Copy)]
struct PreprocessedScore {
    suggested_offset_sec: f64,
    lag_sec: f64,
    raw_peak: f64,
    normalized_peak: f64,
    uncorrected_raw_peak: f64,
    uncorrected_normalized_peak: f64,
}

fn csv_optional_bool(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "true",
        Some(false) => "false",
        None => "",
    }
}

fn csv_optional_f32(value: Option<f32>) -> String {
    value.map_or_else(String::new, |value| format!("{value:.6}"))
}

fn csv_optional_f64(value: Option<f64>) -> String {
    value.map_or_else(String::new, |value| format!("{value:.9}"))
}

#[derive(Debug, Clone, Copy)]
struct FittedPlane {
    intercept: f64,
    note_coef: f64,
    audio_coef: f64,
    r2: f64,
    rmse: f64,
}

impl FittedPlane {
    fn predict_log_raw(&self, note_energy: f64, audio_energy: f64) -> f64 {
        self.intercept + self.note_coef * note_energy.max(1e-12).log10() + self.audio_coef * audio_energy.max(1e-12).log10()
    }
}

#[derive(Debug, Clone, Copy)]
enum PlotMode {
    Corrected,
    Uncorrected,
}

impl PlotMode {
    fn title(self) -> &'static str {
        match self {
            Self::Corrected => "Corrected lag",
            Self::Uncorrected => "Uncorrected lag=0",
        }
    }

    fn raw_peak(self, row: &StudyRow) -> f64 {
        match self {
            Self::Corrected => row.raw_peak,
            Self::Uncorrected => row.uncorrected_raw_peak,
        }
    }

    fn normalized_peak(self, row: &StudyRow) -> f64 {
        match self {
            Self::Corrected => row.normalized_peak,
            Self::Uncorrected => row.uncorrected_normalized_peak,
        }
    }

    fn log_residual(self, row: &StudyRow) -> f64 {
        match self {
            Self::Corrected => row.log_raw_residual,
            Self::Uncorrected => row.uncorrected_log_raw_residual,
        }
    }

    fn empirical_ratio(self, row: &StudyRow) -> f64 {
        match self {
            Self::Corrected => row.empirical_peak_ratio,
            Self::Uncorrected => row.uncorrected_empirical_peak_ratio,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    fs::create_dir_all(cli.root.join("charts"))?;

    ensure_samples(&cli).await?;
    let mut rows = analyze_samples(&cli).await?;
    enrich_public_chart_metadata(&cli, &mut rows).await;
    backfill_slide_ratios(&cli, &mut rows).await;
    backfill_preprocessed_scores(&cli, &mut rows).await;
    let plane = fit_log_peak_plane(&rows)?;
    apply_plane_scores(&mut rows, plane);
    let color_abs = residual_color_abs(&rows);

    write_csv(&cli.root.join("results.csv"), &rows, cli.allow_shrink)?;
    draw_plot(&cli.root.join("peak-energy-corrected.png"), &rows, PlotMode::Corrected, color_abs, plane)?;
    draw_plot(&cli.root.join("peak-energy-uncorrected.png"), &rows, PlotMode::Uncorrected, color_abs, plane)?;
    write_plotly_html(&cli.root.join("peak-energy-corrected.html"), &rows, PlotMode::Corrected, color_abs, plane)?;
    write_plotly_html(&cli.root.join("peak-energy-uncorrected.html"), &rows, PlotMode::Uncorrected, color_abs, plane)?;
    write_offset_score_relation_html(&cli.root.join("offset-score-relation.html"), &rows)?;
    write_theoretical_norm_relation_html(&cli.root.join("theoretical-normalized-score-relation.html"), &rows)?;
    write_score_distribution_html(&cli.root.join("score-distribution.html"), &rows)?;
    write_theoretical_norm_score_distribution_html(&cli.root.join("theoretical-normalized-score-distribution.html"), &rows)?;
    write_preprocessed_theoretical_norm_score_distribution_html(
        &cli.root.join("preprocessed-theoretical-normalized-score-distribution.html"),
        &rows,
    )?;
    write_theoretical_normalized_html(&cli.root.join("theoretical-normalized-correlation.html"), &rows)?;
    write_theoretical_normalized_3d_html(&cli.root.join("theoretical-normalized-3d-corrected.html"), &rows, PlotMode::Corrected)?;
    write_theoretical_normalized_3d_html(&cli.root.join("theoretical-normalized-3d-uncorrected.html"), &rows, PlotMode::Uncorrected)?;

    println!("rows: {}", rows.len());
    println!(
        "fit: log_raw = {:.6} + {:.6}*log_note + {:.6}*log_audio (r2={:.4}, rmse={:.4})",
        plane.intercept, plane.note_coef, plane.audio_coef, plane.r2, plane.rmse
    );
    println!("csv: {}", cli.root.join("results.csv").display());
    println!("corrected plot: {}", cli.root.join("peak-energy-corrected.png").display());
    println!("uncorrected plot: {}", cli.root.join("peak-energy-uncorrected.png").display());
    println!("corrected html: {}", cli.root.join("peak-energy-corrected.html").display());
    println!("uncorrected html: {}", cli.root.join("peak-energy-uncorrected.html").display());
    println!("offset score html: {}", cli.root.join("offset-score-relation.html").display());
    println!("theoretical norm score relation html: {}", cli.root.join("theoretical-normalized-score-relation.html").display());
    println!("score distribution html: {}", cli.root.join("score-distribution.html").display());
    println!("theoretical norm score distribution html: {}", cli.root.join("theoretical-normalized-score-distribution.html").display());
    println!(
        "preprocessed theoretical norm score distribution html: {}",
        cli.root.join("preprocessed-theoretical-normalized-score-distribution.html").display()
    );
    println!("theoretical normalized html: {}", cli.root.join("theoretical-normalized-correlation.html").display());
    println!("theoretical normalized 3d corrected html: {}", cli.root.join("theoretical-normalized-3d-corrected.html").display());
    println!("theoretical normalized 3d uncorrected html: {}", cli.root.join("theoretical-normalized-3d-uncorrected.html").display());
    Ok(())
}

async fn ensure_samples(cli: &Cli) -> Result<()> {
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

fn cached_chart_ids(charts_dir: &Path) -> Result<BTreeSet<i32>> {
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

async fn send_with_retries(request: impl Fn() -> reqwest::RequestBuilder, retries: usize) -> Result<reqwest::Response, reqwest::Error> {
    let attempts = retries.max(1);
    let mut last_error = None;
    for attempt in 1..=attempts {
        match request().send().await {
            Ok(response) => {
                if !should_retry_status(response.status()) || attempt == attempts {
                    return Ok(response);
                }
                tokio::time::sleep(Duration::from_millis(150 * attempt as u64)).await;
            }
            Err(err) => {
                last_error = Some(err);
                if attempt < attempts {
                    tokio::time::sleep(Duration::from_millis(150 * attempt as u64)).await;
                }
            }
        }
    }
    Err(last_error.expect("at least one request attempt"))
}

fn should_retry_status(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
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

async fn analyze_samples(cli: &Cli) -> Result<Vec<StudyRow>> {
    let charts_dir = cli.root.join("charts");
    let existing_rows = if cli.recompute {
        Vec::new()
    } else {
        read_existing_csv(&cli.root.join("results.csv"))?
    };
    let done: BTreeSet<i32> = existing_rows.iter().map(|row| row.chart_id).collect();
    let mut rows = existing_rows;

    let target_rows = rows.len().max(cli.samples);
    let remaining = target_rows.saturating_sub(rows.len());
    let jobs = cli.jobs.max(1);
    let ids: Vec<i32> = cached_chart_ids(&charts_dir)?
        .into_iter()
        .filter(|id| !done.contains(id))
        .take(remaining)
        .collect();

    if !ids.is_empty() {
        println!("analyzing {} cached charts with {jobs} jobs", ids.len());
    }

    let mut analyzed = stream::iter(ids.into_iter().map(|id| {
        let dir = charts_dir.join(id.to_string());
        async move { (id, analyze_chart(id, &dir, cli).await) }
    }))
    .buffer_unordered(jobs);

    while let Some((id, result)) = analyzed.next().await {
        match result {
            Ok(row) => {
                println!(
                    "analyzed {id}: raw={:.3} norm={:.4} raw0={:.3} norm0={:.4}",
                    row.raw_peak, row.normalized_peak, row.uncorrected_raw_peak, row.uncorrected_normalized_peak
                );
                rows.push(row);
            }
            Err(err) => eprintln!("skip analysis {id}: {err:#}"),
        }
    }

    rows.sort_by_key(|row| row.chart_id);
    Ok(rows)
}

fn read_existing_csv(path: &Path) -> Result<Vec<StudyRow>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(path)?;
    let mut rows = Vec::new();
    for line in text.lines().skip(1) {
        let cols = split_csv_line(line);
        if cols.len() != 19 && cols.len() != 25 && cols.len() != 26 && cols.len() != 32 {
            continue;
        }
        let has_metadata = cols.len() >= 25;
        rows.push(StudyRow {
            chart_id: cols[0].parse()?,
            chart_name: cols[1].clone(),
            notes: cols[2].parse()?,
            duration_sec: cols[3].parse()?,
            search_center_sec: cols[4].parse()?,
            suggested_offset_sec: cols[5].parse()?,
            lag_sec: cols[6].parse()?,
            raw_peak: cols[7].parse()?,
            note_energy: cols[8].parse()?,
            audio_energy: cols[9].parse()?,
            normalized_peak: cols[10].parse()?,
            uncorrected_raw_peak: cols[11].parse()?,
            uncorrected_normalized_peak: cols[12].parse()?,
            fitted_log_raw_peak: cols[13].parse()?,
            log_raw_residual: cols[14].parse()?,
            empirical_peak_ratio: cols[15].parse()?,
            uncorrected_log_raw_residual: cols[16].parse()?,
            uncorrected_empirical_peak_ratio: cols[17].parse()?,
            chart_reviewed: if has_metadata { parse_optional_bool(cols.get(18)) } else { None },
            chart_stable: if has_metadata { parse_optional_bool(cols.get(19)) } else { None },
            chart_ranked: if has_metadata { parse_optional_bool(cols.get(20)) } else { None },
            chart_stable_request: if has_metadata { parse_optional_bool(cols.get(21)) } else { None },
            player_rating: if has_metadata { parse_optional_f32(cols.get(22))? } else { None },
            player_rating_score: if has_metadata { parse_optional_f32(cols.get(23))? } else { None },
            reliable: if has_metadata { cols[24].parse()? } else { cols[18].parse()? },
            slide_ratio: if cols.len() >= 26 { parse_optional_f64(cols.get(25))? } else { None },
            preprocessed_suggested_offset_sec: if cols.len() >= 32 { parse_optional_f64(cols.get(26))? } else { None },
            preprocessed_lag_sec: if cols.len() >= 32 { parse_optional_f64(cols.get(27))? } else { None },
            preprocessed_raw_peak: if cols.len() >= 32 { parse_optional_f64(cols.get(28))? } else { None },
            preprocessed_normalized_peak: if cols.len() >= 32 { parse_optional_f64(cols.get(29))? } else { None },
            preprocessed_uncorrected_raw_peak: if cols.len() >= 32 { parse_optional_f64(cols.get(30))? } else { None },
            preprocessed_uncorrected_normalized_peak: if cols.len() >= 32 { parse_optional_f64(cols.get(31))? } else { None },
        });
    }
    Ok(rows)
}

fn parse_optional_bool(value: Option<&String>) -> Option<bool> {
    value.and_then(|value| match value.as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    })
}

fn parse_optional_f32(value: Option<&String>) -> Result<Option<f32>> {
    value.map_or(Ok(None), |value| if value.is_empty() { Ok(None) } else { Ok(Some(value.parse()?)) })
}

fn parse_optional_f64(value: Option<&String>) -> Result<Option<f64>> {
    value.map_or(Ok(None), |value| if value.is_empty() { Ok(None) } else { Ok(Some(value.parse()?)) })
}

async fn backfill_slide_ratios(cli: &Cli, rows: &mut [StudyRow]) {
    let charts_dir = cli.root.join("charts");
    let mut filled = 0;
    for row in rows {
        if row.slide_ratio.is_some() {
            continue;
        }
        match load_chart_note_stats(&charts_dir.join(row.chart_id.to_string())).await {
            Ok(stats) => {
                row.slide_ratio = Some(stats.slide_ratio());
                filled += 1;
            }
            Err(err) => eprintln!("skip slide ratio {}: {err:#}", row.chart_id),
        }
    }
    if filled > 0 {
        println!("slide ratios: backfilled {filled} rows from cached charts");
    }
}

async fn backfill_preprocessed_scores(cli: &Cli, rows: &mut [StudyRow]) {
    let charts_dir = cli.root.join("charts");
    let mut filled = 0;
    let jobs = cli.jobs.max(1);
    let ids: Vec<i32> = rows
        .iter()
        .filter(|row| row.preprocessed_normalized_peak.is_none())
        .map(|row| row.chart_id)
        .collect();
    if ids.is_empty() {
        return;
    }

    println!("preprocessed scores: analyzing {} cached charts with {jobs} jobs", ids.len());
    let mut analyzed = stream::iter(ids.into_iter().map(|id| {
        let dir = charts_dir.join(id.to_string());
        async move { (id, analyze_preprocessed_chart(id, &dir, cli).await) }
    }))
    .buffer_unordered(jobs);

    let mut scores = HashMap::new();
    while let Some((id, result)) = analyzed.next().await {
        match result {
            Ok(score) => {
                scores.insert(id, score);
                filled += 1;
            }
            Err(err) => eprintln!("skip preprocessed analysis {id}: {err:#}"),
        }
    }

    for row in rows {
        if let Some(score) = scores.get(&row.chart_id) {
            row.apply_preprocessed_score(*score);
        }
    }
    if filled > 0 {
        println!("preprocessed scores: backfilled {filled} rows from cached charts");
    }
}

async fn enrich_public_chart_metadata(cli: &Cli, rows: &mut [StudyRow]) {
    let ids: Vec<i32> = rows.iter().filter(|row| row.player_rating.is_none()).map(|row| row.chart_id).collect();
    if ids.is_empty() {
        return;
    }

    let client = match reqwest::Client::builder().timeout(Duration::from_millis(cli.request_timeout_ms)).build() {
        Ok(client) => client,
        Err(err) => {
            eprintln!("skip metadata fetch: {err:#}");
            return;
        }
    };

    let mut metadata = HashMap::new();
    for chunk in ids.chunks(80) {
        let ids_str = chunk.iter().map(i32::to_string).collect::<Vec<_>>().join(",");
        let request = || client.get(format!("{API_URL}/chart/multi-get")).query(&[("ids", ids_str.clone())]);
        match send_with_retries(request, cli.retries)
            .await
            .and_then(|response| response.error_for_status())
        {
            Ok(response) => match response.json::<Vec<PublicChartMetadata>>().await {
                Ok(items) => {
                    for item in items {
                        metadata.insert(item.id, item);
                    }
                }
                Err(err) => eprintln!("skip metadata chunk {ids_str}: {err:#}"),
            },
            Err(err) => eprintln!("skip metadata chunk {ids_str}: {err:#}"),
        }
    }

    for row in rows {
        if let Some(item) = metadata.get(&row.chart_id) {
            row.apply_metadata(item);
        }
    }
    println!("metadata: fetched {} public chart records", metadata.len());
}

fn fit_log_peak_plane(rows: &[StudyRow]) -> Result<FittedPlane> {
    if rows.len() < 3 {
        bail!("need at least 3 rows to fit log peak plane")
    }

    let mut xtx = [[0.0; 3]; 3];
    let mut xtz = [0.0; 3];
    let mut zs = Vec::with_capacity(rows.len());
    for row in rows {
        let x = row.note_energy.max(1e-12).log10();
        let y = row.audio_energy.max(1e-12).log10();
        let z = row.raw_peak.max(1e-12).log10();
        let v = [1.0, x, y];
        for i in 0..3 {
            xtz[i] += v[i] * z;
            for j in 0..3 {
                xtx[i][j] += v[i] * v[j];
            }
        }
        zs.push(z);
    }

    let coef = solve_3x3(xtx, xtz).context("failed to fit log peak plane")?;
    let plane = FittedPlane {
        intercept: coef[0],
        note_coef: coef[1],
        audio_coef: coef[2],
        r2: 0.0,
        rmse: 0.0,
    };

    let mean_z = zs.iter().sum::<f64>() / zs.len() as f64;
    let mut ss_res = 0.0;
    let mut ss_tot = 0.0;
    for (row, z) in rows.iter().zip(zs) {
        let residual = z - plane.predict_log_raw(row.note_energy, row.audio_energy);
        ss_res += residual * residual;
        let centered = z - mean_z;
        ss_tot += centered * centered;
    }

    Ok(FittedPlane {
        r2: if ss_tot > 0.0 { 1.0 - ss_res / ss_tot } else { 1.0 },
        rmse: (ss_res / rows.len() as f64).sqrt(),
        ..plane
    })
}

fn solve_3x3(mut a: [[f64; 3]; 3], mut b: [f64; 3]) -> Option<[f64; 3]> {
    for col in 0..3 {
        let mut pivot = col;
        for row in col + 1..3 {
            if a[row][col].abs() > a[pivot][col].abs() {
                pivot = row;
            }
        }
        if a[pivot][col].abs() < 1e-12 {
            return None;
        }
        a.swap(col, pivot);
        b.swap(col, pivot);

        let denom = a[col][col];
        for value in a[col].iter_mut().skip(col) {
            *value /= denom;
        }
        b[col] /= denom;

        for row in 0..3 {
            if row == col {
                continue;
            }
            let factor = a[row][col];
            let pivot_row = a[col];
            for (value, &pivot_value) in a[row].iter_mut().zip(pivot_row.iter()).skip(col) {
                *value -= factor * pivot_value;
            }
            b[row] -= factor * b[col];
        }
    }
    Some(b)
}

fn apply_plane_scores(rows: &mut [StudyRow], plane: FittedPlane) {
    for row in rows {
        row.fitted_log_raw_peak = plane.predict_log_raw(row.note_energy, row.audio_energy);
        row.log_raw_residual = row.raw_peak.max(1e-12).log10() - row.fitted_log_raw_peak;
        row.empirical_peak_ratio = 10f64.powf(row.log_raw_residual);
        row.uncorrected_log_raw_residual = row.uncorrected_raw_peak.max(1e-12).log10() - row.fitted_log_raw_peak;
        row.uncorrected_empirical_peak_ratio = 10f64.powf(row.uncorrected_log_raw_residual);
    }
}

fn residual_color_abs(rows: &[StudyRow]) -> f64 {
    let mut values: Vec<f64> = rows
        .iter()
        .flat_map(|row| [row.log_raw_residual.abs(), row.uncorrected_log_raw_residual.abs()])
        .filter(|value| value.is_finite())
        .collect();
    if values.is_empty() {
        return 0.3;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let idx = ((values.len() - 1) as f64 * 0.95).round() as usize;
    values[idx].max(0.05)
}

fn split_csv_line(line: &str) -> Vec<String> {
    let mut cols = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => {
                cols.push(std::mem::take(&mut cur));
            }
            _ => cur.push(ch),
        }
    }
    cols.push(cur);
    cols
}

async fn analyze_chart(id: i32, dir: &Path, cli: &Cli) -> Result<StudyRow> {
    let mut fs = fs_from_file(dir)?;
    let info = load_info(&mut *fs).await?;
    let (chart_offset, note_stats) = load_chart_timing(&mut *fs, &info).await?;
    if note_stats.events.len() < 16 {
        bail!("too few notes")
    }

    let audio_path = dir.join(&info.music);
    let clip = prpr_avc::demux_audio(audio_path.to_str().context("invalid audio path")?)?.context("no audio stream found")?;
    let pcm: Vec<f32> = clip.frames().iter().map(|f| (f.0 + f.1) / 2.0).collect();
    let sample_rate = clip.sample_rate();
    let duration = pcm.len() as f64 / sample_rate as f64;

    let audio = SuperFlux::new(&pcm, sample_rate, 2048, 1024);
    let note = NoteGaussian::new(note_stats.times(), cli.blur_sigma);
    let preprocessed_note = PreprocessedNoteGaussian::new(note_stats.events.clone(), cli.blur_sigma);
    let search_center = chart_offset + info.offset as f64;
    let config = AlignConfig {
        search_range_sec: cli.range,
        sampling_interval_sec: cli.interval,
        search_center_sec: search_center,
    };

    let stats = estimate_energy_stats(&audio, &note, duration, &config);
    let preprocessed_stats = estimate_energy_stats(&audio, &preprocessed_note, duration, &config);
    let preprocessed_score = PreprocessedScore {
        suggested_offset_sec: preprocessed_stats.offset,
        lag_sec: preprocessed_stats.offset - search_center,
        raw_peak: preprocessed_stats.raw_peak,
        normalized_peak: preprocessed_stats.normalized_peak,
        uncorrected_raw_peak: preprocessed_stats.uncorrected_raw_peak,
        uncorrected_normalized_peak: preprocessed_stats.uncorrected_normalized_peak,
    };
    Ok(StudyRow {
        chart_id: id,
        chart_name: info.name,
        notes: note_stats.events.len(),
        duration_sec: duration,
        search_center_sec: search_center,
        suggested_offset_sec: stats.offset,
        lag_sec: stats.offset - search_center,
        raw_peak: stats.raw_peak,
        note_energy: stats.note_energy,
        audio_energy: stats.audio_energy,
        normalized_peak: stats.normalized_peak,
        uncorrected_raw_peak: stats.uncorrected_raw_peak,
        uncorrected_normalized_peak: stats.uncorrected_normalized_peak,
        fitted_log_raw_peak: 0.0,
        log_raw_residual: 0.0,
        empirical_peak_ratio: 0.0,
        uncorrected_log_raw_residual: 0.0,
        uncorrected_empirical_peak_ratio: 0.0,
        chart_reviewed: None,
        chart_stable: None,
        chart_ranked: None,
        chart_stable_request: None,
        player_rating: None,
        player_rating_score: None,
        reliable: stats.normalized_peak > 0.05,
        slide_ratio: Some(note_stats.slide_ratio()),
        preprocessed_suggested_offset_sec: Some(preprocessed_score.suggested_offset_sec),
        preprocessed_lag_sec: Some(preprocessed_score.lag_sec),
        preprocessed_raw_peak: Some(preprocessed_score.raw_peak),
        preprocessed_normalized_peak: Some(preprocessed_score.normalized_peak),
        preprocessed_uncorrected_raw_peak: Some(preprocessed_score.uncorrected_raw_peak),
        preprocessed_uncorrected_normalized_peak: Some(preprocessed_score.uncorrected_normalized_peak),
    })
}

async fn analyze_preprocessed_chart(_id: i32, dir: &Path, cli: &Cli) -> Result<PreprocessedScore> {
    let mut fs = fs_from_file(dir)?;
    let info = load_info(&mut *fs).await?;
    let (chart_offset, note_stats) = load_chart_timing(&mut *fs, &info).await?;
    if note_stats.events.len() < 16 {
        bail!("too few notes")
    }

    let audio_path = dir.join(&info.music);
    let clip = prpr_avc::demux_audio(audio_path.to_str().context("invalid audio path")?)?.context("no audio stream found")?;
    let pcm: Vec<f32> = clip.frames().iter().map(|f| (f.0 + f.1) / 2.0).collect();
    let sample_rate = clip.sample_rate();
    let duration = pcm.len() as f64 / sample_rate as f64;

    let audio = SuperFlux::new(&pcm, sample_rate, 2048, 1024);
    let note = PreprocessedNoteGaussian::new(note_stats.events, cli.blur_sigma);
    let search_center = chart_offset + info.offset as f64;
    let config = AlignConfig {
        search_range_sec: cli.range,
        sampling_interval_sec: cli.interval,
        search_center_sec: search_center,
    };
    let stats = estimate_energy_stats(&audio, &note, duration, &config);
    Ok(PreprocessedScore {
        suggested_offset_sec: stats.offset,
        lag_sec: stats.offset - search_center,
        raw_peak: stats.raw_peak,
        normalized_peak: stats.normalized_peak,
        uncorrected_raw_peak: stats.uncorrected_raw_peak,
        uncorrected_normalized_peak: stats.uncorrected_normalized_peak,
    })
}

async fn load_chart_timing(fs: &mut dyn FileSystem, info: &ChartInfo) -> Result<(f64, NoteStats)> {
    let bytes = fs.load_file(&info.chart).await?;
    let format = infer_chart_format(info, &bytes);
    if matches!(format, ChartFormat::Rpe) {
        return parse_rpe_timing(&String::from_utf8_lossy(&bytes));
    }
    let chart = load_chart(fs, info, &bytes, format).await?;
    Ok((chart.offset as f64, extract_note_stats(&chart)))
}

async fn load_chart(fs: &mut dyn FileSystem, info: &ChartInfo, bytes: &[u8], format: ChartFormat) -> Result<Chart> {
    match format {
        ChartFormat::Rpe => parse_rpe(&String::from_utf8_lossy(bytes), fs, Default::default(), info.use_rpe_170_speed.unwrap_or_default()).await,
        ChartFormat::Pgr => parse_phigros(&String::from_utf8_lossy(bytes), Default::default()),
        ChartFormat::Pec => parse_pec(&String::from_utf8_lossy(bytes), Default::default()),
        ChartFormat::Pbc => bail!("pbc charts are not supported by this study tool"),
    }
}

async fn load_chart_note_stats(dir: &Path) -> Result<NoteStats> {
    let mut fs = fs_from_file(dir)?;
    let info = load_info(&mut *fs).await?;
    let (_, stats) = load_chart_timing(&mut *fs, &info).await?;
    Ok(stats)
}

#[derive(Debug, Clone)]
struct NoteStats {
    events: Vec<NoteEvent>,
    slides: usize,
}

impl NoteStats {
    fn times(&self) -> Vec<f64> {
        self.events.iter().map(|event| event.time).collect()
    }

    fn slide_ratio(&self) -> f64 {
        if self.events.is_empty() {
            0.0
        } else {
            self.slides as f64 / self.events.len() as f64
        }
    }
}

fn parse_rpe_timing(source: &str) -> Result<(f64, NoteStats)> {
    let rpe: RpeTimingChart = serde_json::from_str(source).context("failed to parse RPE timing")?;
    let mut bpm = BpmList::new(rpe.bpm_list.into_iter().map(|it| (it.start_time.beats(), it.bpm)).collect());
    let mut events: Vec<NoteEvent> = rpe
        .judge_line_list
        .into_iter()
        .flat_map(|line| line.notes.unwrap_or_default())
        .filter(|note| note.is_fake == 0)
        .map(|note| NoteEvent::new(bpm.time(&note.start_time), rpe_note_kind(note.kind)))
        .filter(|event| event.time >= 0.0)
        .collect();
    events.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());
    let slides = events.iter().filter(|event| event.kind == AutoOffsetNoteKind::Slide).count();
    Ok((rpe.meta.offset as f64 / 1000.0, NoteStats { events, slides }))
}

fn rpe_note_kind(kind: u8) -> AutoOffsetNoteKind {
    match kind {
        2 => AutoOffsetNoteKind::Hold,
        3 => AutoOffsetNoteKind::Flick,
        4 => AutoOffsetNoteKind::Slide,
        _ => AutoOffsetNoteKind::Tap,
    }
}

fn infer_chart_format(info: &ChartInfo, bytes: &[u8]) -> ChartFormat {
    info.format.clone().unwrap_or_else(|| {
        if let Ok(text) = String::from_utf8(bytes.to_vec()) {
            if text.starts_with('{') {
                if text.contains("\"META\"") {
                    ChartFormat::Rpe
                } else {
                    ChartFormat::Pgr
                }
            } else {
                ChartFormat::Pec
            }
        } else {
            ChartFormat::Pbc
        }
    })
}

fn extract_note_stats(chart: &Chart) -> NoteStats {
    let mut events: Vec<NoteEvent> = chart
        .lines
        .iter()
        .flat_map(|line| line.notes.iter())
        .filter(|note| !note.fake)
        .map(|note| NoteEvent::new(note.time, auto_offset_note_kind(&note.kind)))
        .filter(|event| event.time >= 0.0)
        .collect();
    events.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());
    let slides = events.iter().filter(|event| event.kind == AutoOffsetNoteKind::Slide).count();
    NoteStats { events, slides }
}

fn auto_offset_note_kind(kind: &NoteKind) -> AutoOffsetNoteKind {
    match kind {
        NoteKind::Click => AutoOffsetNoteKind::Tap,
        NoteKind::Hold { .. } => AutoOffsetNoteKind::Hold,
        NoteKind::Flick => AutoOffsetNoteKind::Flick,
        NoteKind::Drag => AutoOffsetNoteKind::Slide,
    }
}

struct EnergyStats {
    offset: f64,
    raw_peak: f64,
    note_energy: f64,
    audio_energy: f64,
    normalized_peak: f64,
    uncorrected_raw_peak: f64,
    uncorrected_normalized_peak: f64,
}

fn estimate_energy_stats<A: Signal, N: Signal>(audio: &A, note: &N, duration: f64, config: &AlignConfig) -> EnergyStats {
    let t_min = config.search_center_sec - config.search_range_sec;
    let t_max = config.search_center_sec + duration + config.search_range_sec;
    let count = ((t_max - t_min) / config.sampling_interval_sec).ceil() as usize + 1;
    let ts: Vec<f64> = (0..count).map(|i| t_min + i as f64 * config.sampling_interval_sec).collect();
    let audio_samples = audio.samples(&ts);
    let note_ts: Vec<f64> = ts.iter().map(|&t| t - config.search_center_sec).collect();
    let note_samples = note.samples(&note_ts);

    let note_energy = note_samples.iter().map(|&v| (v as f64).powi(2)).sum::<f64>();
    let audio_energy = audio_samples.iter().map(|&v| (v as f64).powi(2)).sum::<f64>();
    let denom = (note_energy * audio_energy).sqrt();
    let max_lag_bins = (config.search_range_sec / config.sampling_interval_sec).ceil() as usize;
    let n = note_samples.len().min(audio_samples.len());

    let mut best_lag = max_lag_bins;
    let mut best_norm = f64::NEG_INFINITY;
    let mut best_raw = 0.0;
    let mut uncorrected_raw = 0.0;
    let mut uncorrected_norm = 0.0;
    for lag_offset in 0..=2 * max_lag_bins {
        let lag = lag_offset as isize - max_lag_bins as isize;
        let mut raw = 0.0;
        for (i, &note_value) in note_samples.iter().take(n).enumerate() {
            let j = i as isize + lag;
            if j >= 0 && j < audio_samples.len() as isize {
                raw += note_value as f64 * audio_samples[j as usize] as f64;
            }
        }
        let normalized = if denom > 0.0 { (raw / denom).clamp(0.0, 1.0) } else { 0.0 };
        if lag_offset == max_lag_bins {
            uncorrected_raw = raw;
            uncorrected_norm = normalized;
        }
        if normalized > best_norm {
            best_norm = normalized;
            best_raw = raw;
            best_lag = lag_offset;
        }
    }

    let lag_sec = (best_lag as isize - max_lag_bins as isize) as f64 * config.sampling_interval_sec;
    EnergyStats {
        offset: config.search_center_sec + lag_sec,
        raw_peak: best_raw,
        note_energy,
        audio_energy,
        normalized_peak: best_norm,
        uncorrected_raw_peak: uncorrected_raw,
        uncorrected_normalized_peak: uncorrected_norm,
    }
}

fn write_csv(path: &Path, rows: &[StudyRow], allow_shrink: bool) -> Result<()> {
    if path.exists() && !allow_shrink {
        let existing_rows = count_csv_data_rows(path)?;
        if rows.len() < existing_rows {
            bail!(
                "refusing to overwrite {} rows in {} with {} rows; rerun with --allow-shrink if this is intentional",
                existing_rows,
                path.display(),
                rows.len()
            );
        }
    }
    let mut file = File::create(path)?;
    writeln!(file, "{}", StudyRow::header())?;
    for row in rows {
        writeln!(file, "{}", row.to_csv())?;
    }
    Ok(())
}

fn count_csv_data_rows(path: &Path) -> Result<usize> {
    Ok(fs::read_to_string(path)?.lines().skip(1).filter(|line| !line.trim().is_empty()).count())
}

fn write_plotly_html(path: &Path, rows: &[StudyRow], mode: PlotMode, color_abs: f64, plane: FittedPlane) -> Result<()> {
    let x: Vec<f64> = rows.iter().map(|row| (row.note_energy.max(1e-12)).log10()).collect();
    let y: Vec<f64> = rows.iter().map(|row| (row.audio_energy.max(1e-12)).log10()).collect();
    let z: Vec<f64> = rows.iter().map(|row| mode.raw_peak(row).max(1e-12).log10()).collect();
    let color: Vec<f64> = rows.iter().map(|row| mode.log_residual(row)).collect();
    let text: Vec<String> = rows
        .iter()
        .map(|row| {
            format!(
                "#{} {}<br>notes: {}<br>status: {}<br>rating: {}<br>offset: {:.0}ms<br>lag: {:.0}ms<br>raw: {:.3}<br>norm: {:.4}<br>fit residual: {:.4}<br>raw / fitted: {:.3}",
                row.chart_id,
                row.chart_name,
                row.notes,
                chart_status_label(row),
                rating_label(row.player_rating_score),
                row.suggested_offset_sec * 1000.0,
                match mode {
                    PlotMode::Corrected => row.lag_sec * 1000.0,
                    PlotMode::Uncorrected => 0.0,
                },
                mode.raw_peak(row),
                mode.normalized_peak(row),
                mode.log_residual(row),
                mode.empirical_ratio(row)
            )
        })
        .collect();
    let title = format!(
        "Auto-offset energy study: {} (fit log_raw = {:.3} + {:.3} log_note + {:.3} log_audio, R2={:.3})",
        mode.title(),
        plane.intercept,
        plane.note_coef,
        plane.audio_coef,
        plane.r2
    );
    let html = format!(
        r#"<!doctype html>
<html>
<head>
  <meta charset="utf-8">
  <title>Auto-offset energy study</title>
  <script src="https://cdn.plot.ly/plotly-2.35.2.min.js"></script>
  <style>html, body, #plot {{ width: 100%; height: 100%; margin: 0; }}</style>
</head>
<body>
  <div id="plot"></div>
  <script>
    const trace = {{
      type: 'scatter3d',
      mode: 'markers',
      x: {x},
      y: {y},
      z: {z},
      text: {text},
      hovertemplate: '%{{text}}<br>log note energy: %{{x:.3f}}<br>log audio energy: %{{y:.3f}}<br>log raw peak: %{{z:.3f}}<extra></extra>',
      marker: {{
        size: 4,
        color: {color},
        colorscale: 'RdBu',
        reversescale: true,
        cmin: {cmin},
        cmax: {cmax},
        cmid: 0,
        colorbar: {{ title: 'log(raw / fitted)' }},
        opacity: 0.82
      }}
    }};
    const layout = {{
      title: {title},
      scene: {{
        xaxis: {{ title: 'log10(note energy)' }},
        yaxis: {{ title: 'log10(audio energy)' }},
        zaxis: {{ title: 'log10(raw peak)' }}
      }},
      margin: {{ l: 0, r: 0, b: 0, t: 42 }}
    }};
    Plotly.newPlot('plot', [trace], layout, {{ responsive: true }});
  </script>
</body>
</html>
"#,
        x = serde_json::to_string(&x)?,
        y = serde_json::to_string(&y)?,
        z = serde_json::to_string(&z)?,
        color = serde_json::to_string(&color)?,
        text = serde_json::to_string(&text)?,
        title = serde_json::to_string(&title)?,
        cmin = -color_abs,
        cmax = color_abs,
    );
    fs::write(path, html)?;
    Ok(())
}

fn write_offset_score_relation_html(path: &Path, rows: &[StudyRow]) -> Result<()> {
    let stable = offset_score_points(rows, true);
    let unstable = offset_score_points(rows, false);
    let max_lag_ms = stable.lag_ms.iter().chain(&unstable.lag_ms).copied().fold(0.0, f64::max).max(1.0);
    let title = format!("Corrected vs uncorrected fitted-plane score (stable={}, unstable={})", stable.x.len(), unstable.x.len());
    let html = format!(
        r#"<!doctype html>
<html>
<head>
  <meta charset="utf-8">
  <title>Corrected vs uncorrected score</title>
  <script src="https://cdn.plot.ly/plotly-2.35.2.min.js"></script>
  <style>html, body, #plot {{ width: 100%; height: 100%; margin: 0; }}</style>
</head>
<body>
  <div id="plot"></div>
  <script>
    const stable = {{
      type: 'scatter',
      mode: 'markers',
      name: 'stable',
      x: {stable_x},
      y: {stable_y},
      marker: {{
        size: 7,
        color: {stable_color},
        colorscale: 'Viridis',
        cmin: 0,
        cmax: {cmax},
        colorbar: {{ title: '|lag| ms' }},
        opacity: 0.78
      }},
      text: {stable_text},
      xaxis: 'x',
      yaxis: 'y',
      hovertemplate: '%{{text}}<br>uncorrected score: %{{x:.4f}}<br>corrected score: %{{y:.4f}}<br>|lag|: %{{marker.color:.0f}}ms<extra></extra>'
    }};
    const unstable = {{
      type: 'scatter',
      mode: 'markers',
      name: 'unstable',
      x: {unstable_x},
      y: {unstable_y},
      marker: {{
        size: 7,
        color: {unstable_color},
        colorscale: 'Viridis',
        cmin: 0,
        cmax: {cmax},
        showscale: false,
        opacity: 0.78
      }},
      text: {unstable_text},
      xaxis: 'x2',
      yaxis: 'y2',
      hovertemplate: '%{{text}}<br>uncorrected score: %{{x:.4f}}<br>corrected score: %{{y:.4f}}<br>|lag|: %{{marker.color:.0f}}ms<extra></extra>'
    }};
    const diagonal = {{ type: 'line', x0: -0.75, y0: -0.75, x1: 0.45, y1: 0.45, line: {{ color: '#888', width: 1, dash: 'dot' }} }};
    const layout = {{
      title: {title},
      grid: {{ rows: 1, columns: 2, pattern: 'independent' }},
      xaxis: {{ title: 'uncorrected score: log(raw / fitted)', range: [-0.75, 0.45], zeroline: true }},
      yaxis: {{ title: 'corrected score: log(raw / fitted)', range: [-0.75, 0.45], zeroline: true }},
      xaxis2: {{ title: 'uncorrected score: log(raw / fitted)', range: [-0.75, 0.45], zeroline: true }},
      yaxis2: {{ title: 'corrected score: log(raw / fitted)', range: [-0.75, 0.45], zeroline: true }},
      shapes: [diagonal, {{ ...diagonal, xref: 'x2', yref: 'y2' }}],
      annotations: [
        {{ text: 'stable / listed', x: 0.22, y: 1.06, xref: 'paper', yref: 'paper', showarrow: false, font: {{ size: 16 }} }},
        {{ text: 'unstable / not listed', x: 0.78, y: 1.06, xref: 'paper', yref: 'paper', showarrow: false, font: {{ size: 16 }} }}
      ],
      margin: {{ l: 64, r: 24, b: 56, t: 80 }},
      showlegend: false
    }};
    Plotly.newPlot('plot', [stable, unstable], layout, {{ responsive: true }});
  </script>
</body>
</html>
"#,
        stable_x = serde_json::to_string(&stable.x)?,
        stable_y = serde_json::to_string(&stable.y)?,
        stable_color = serde_json::to_string(&stable.lag_ms)?,
        stable_text = serde_json::to_string(&stable.text)?,
        unstable_x = serde_json::to_string(&unstable.x)?,
        unstable_y = serde_json::to_string(&unstable.y)?,
        unstable_color = serde_json::to_string(&unstable.lag_ms)?,
        unstable_text = serde_json::to_string(&unstable.text)?,
        title = serde_json::to_string(&title)?,
        cmax = max_lag_ms,
    );
    fs::write(path, html)?;
    Ok(())
}

struct OffsetScorePoints {
    x: Vec<f64>,
    y: Vec<f64>,
    lag_ms: Vec<f64>,
    slide_ratio: Vec<f64>,
    text: Vec<String>,
}

fn offset_score_points(rows: &[StudyRow], stable: bool) -> OffsetScorePoints {
    let mut x = Vec::new();
    let mut y = Vec::new();
    let mut lag_ms = Vec::new();
    let mut slide_ratio = Vec::new();
    let mut text = Vec::new();
    for row in rows {
        if row.chart_stable != Some(stable) {
            continue;
        }
        x.push(row.uncorrected_log_raw_residual);
        y.push(row.log_raw_residual);
        lag_ms.push(row.lag_sec.abs() * 1000.0);
        slide_ratio.push(row.slide_ratio.unwrap_or(0.0));
        text.push(format!("#{} {}<br>status: {}<br>lag: {:+.0}ms", row.chart_id, row.chart_name, chart_status_label(row), row.lag_sec * 1000.0));
    }
    OffsetScorePoints {
        x,
        y,
        lag_ms,
        slide_ratio,
        text,
    }
}

fn write_theoretical_norm_relation_html(path: &Path, rows: &[StudyRow]) -> Result<()> {
    let stable = theoretical_norm_points(rows, true);
    let unstable = theoretical_norm_points(rows, false);
    let title = format!("Corrected vs uncorrected theoretical normalized score (stable={}, unstable={})", stable.x.len(), unstable.x.len());
    let html = format!(
        r#"<!doctype html>
<html>
<head>
  <meta charset="utf-8">
  <title>Corrected vs uncorrected theoretical normalized score</title>
  <script src="https://cdn.plot.ly/plotly-2.35.2.min.js"></script>
  <style>html, body, #plot {{ width: 100%; height: 100%; margin: 0; }}</style>
</head>
<body>
  <div id="plot"></div>
  <script>
    const stable = {norm_relation_stable};
    const unstable = {norm_relation_unstable};
    const diagonal = {{ type: 'line', x0: 0, y0: 0, x1: 1, y1: 1, line: {{ color: '#888', width: 1, dash: 'dot' }} }};
    const layout = {{
      title: {title},
      grid: {{ rows: 1, columns: 2, pattern: 'independent' }},
      xaxis: {{ title: 'uncorrected normalized peak', range: [0, 1], zeroline: true }},
      yaxis: {{ title: 'corrected normalized peak', range: [0, 1], zeroline: true }},
      xaxis2: {{ title: 'uncorrected normalized peak', range: [0, 1], zeroline: true }},
      yaxis2: {{ title: 'corrected normalized peak', range: [0, 1], zeroline: true }},
      shapes: [diagonal, {{ ...diagonal, xref: 'x2', yref: 'y2' }}],
      annotations: [
        {{ text: 'stable / listed', x: 0.22, y: 1.06, xref: 'paper', yref: 'paper', showarrow: false, font: {{ size: 16 }} }},
        {{ text: 'unstable / not listed', x: 0.78, y: 1.06, xref: 'paper', yref: 'paper', showarrow: false, font: {{ size: 16 }} }}
      ],
      margin: {{ l: 64, r: 24, b: 56, t: 80 }},
      showlegend: false
    }};
    Plotly.newPlot('plot', [stable, unstable], layout, {{ responsive: true }});
  </script>
</body>
</html>
"#,
        norm_relation_stable = norm_relation_trace(&stable, "stable", "x", "y", true)?,
        norm_relation_unstable = norm_relation_trace(&unstable, "unstable", "x2", "y2", false)?,
        title = serde_json::to_string(&title)?,
    );
    fs::write(path, html)?;
    Ok(())
}

fn theoretical_norm_points(rows: &[StudyRow], stable: bool) -> OffsetScorePoints {
    let mut x = Vec::new();
    let mut y = Vec::new();
    let mut lag_ms = Vec::new();
    let mut slide_ratio = Vec::new();
    let mut text = Vec::new();
    for row in rows {
        if row.chart_stable != Some(stable) {
            continue;
        }
        x.push(row.uncorrected_normalized_peak);
        y.push(row.normalized_peak);
        lag_ms.push(row.lag_sec.abs() * 1000.0);
        let ratio = row.slide_ratio.unwrap_or(0.0);
        slide_ratio.push(ratio);
        text.push(format!(
            "#{} {}<br>status: {}<br>lag: {:+.0}ms<br>slide ratio: {:.1}%",
            row.chart_id,
            row.chart_name,
            chart_status_label(row),
            row.lag_sec * 1000.0,
            ratio * 100.0
        ));
    }
    OffsetScorePoints {
        x,
        y,
        lag_ms,
        slide_ratio,
        text,
    }
}

fn norm_relation_trace(points: &OffsetScorePoints, name: &str, xaxis: &str, yaxis: &str, showscale: bool) -> Result<String> {
    Ok(format!(
        "{{ type: 'scatter', mode: 'markers', name: '{name}', x: {x}, y: {y}, marker: {{ size: 7, color: {color}, colorscale: 'YlOrRd', cmin: 0, cmax: 1, showscale: {showscale}, colorbar: {{ title: 'slide ratio' }}, opacity: 0.78 }}, text: {text}, xaxis: '{xaxis}', yaxis: '{yaxis}', hovertemplate: '%{{text}}<br>uncorrected norm: %{{x:.4f}}<br>corrected norm: %{{y:.4f}}<extra></extra>' }}",
        x = serde_json::to_string(&points.x)?,
        y = serde_json::to_string(&points.y)?,
        color = serde_json::to_string(&points.slide_ratio)?,
        text = serde_json::to_string(&points.text)?,
        showscale = if showscale { "true" } else { "false" },
    ))
}

fn write_score_distribution_html(path: &Path, rows: &[StudyRow]) -> Result<()> {
    let stable_corrected = score_distribution(rows, true, true);
    let stable_uncorrected = score_distribution(rows, true, false);
    let unstable_corrected = score_distribution(rows, false, true);
    let unstable_uncorrected = score_distribution(rows, false, false);
    let bin_size = 0.05;
    let curve_x: Vec<f64> = (0..=240).map(|i| -0.75 + i as f64 * 0.005).collect();
    let title = format!(
        "Score distributions (stable corrected={}, stable uncorrected={}, unstable corrected={}, unstable uncorrected={})",
        stable_corrected.values.len(),
        stable_uncorrected.values.len(),
        unstable_corrected.values.len(),
        unstable_uncorrected.values.len()
    );
    let html = format!(
        r#"<!doctype html>
<html>
<head>
  <meta charset="utf-8">
  <title>Score distributions</title>
  <script src="https://cdn.plot.ly/plotly-2.35.2.min.js"></script>
  <style>html, body, #plot {{ width: 100%; height: 100%; margin: 0; }}</style>
</head>
<body>
  <div id="plot"></div>
  <script>
    const traces = [
      {stable_corrected_hist},
      {stable_corrected_curve},
      {stable_uncorrected_hist},
      {stable_uncorrected_curve},
      {unstable_corrected_hist},
      {unstable_corrected_curve},
      {unstable_uncorrected_hist},
      {unstable_uncorrected_curve}
    ];
    const layout = {{
      title: {title},
      grid: {{ rows: 2, columns: 2, pattern: 'independent' }},
      barmode: 'overlay',
      bargap: 0.05,
      xaxis: {{ title: 'corrected score', range: [-0.75, 0.45] }},
      yaxis: {{ title: 'count' }},
      xaxis2: {{ title: 'uncorrected score', range: [-0.75, 0.45] }},
      yaxis2: {{ title: 'count' }},
      xaxis3: {{ title: 'corrected score', range: [-0.75, 0.45] }},
      yaxis3: {{ title: 'count' }},
      xaxis4: {{ title: 'uncorrected score', range: [-0.75, 0.45] }},
      yaxis4: {{ title: 'count' }},
      annotations: [
        {{ text: 'stable corrected', x: 0.225, y: 1.06, xref: 'paper', yref: 'paper', showarrow: false, font: {{ size: 15 }} }},
        {{ text: 'stable uncorrected', x: 0.775, y: 1.06, xref: 'paper', yref: 'paper', showarrow: false, font: {{ size: 15 }} }},
        {{ text: 'unstable corrected', x: 0.225, y: 0.46, xref: 'paper', yref: 'paper', showarrow: false, font: {{ size: 15 }} }},
        {{ text: 'unstable uncorrected', x: 0.775, y: 0.46, xref: 'paper', yref: 'paper', showarrow: false, font: {{ size: 15 }} }}
      ],
      margin: {{ l: 64, r: 24, b: 56, t: 82 }},
      showlegend: false
    }};
    Plotly.newPlot('plot', traces, layout, {{ responsive: true }});
  </script>
</body>
</html>
"#,
        stable_corrected_hist = histogram_trace(&stable_corrected, "x", "y", "#2f6fbd", bin_size)?,
        stable_corrected_curve = normal_curve_trace(&stable_corrected, &curve_x, "x", "y", "#122b55", bin_size)?,
        stable_uncorrected_hist = histogram_trace(&stable_uncorrected, "x2", "y2", "#5a8fd8", bin_size)?,
        stable_uncorrected_curve = normal_curve_trace(&stable_uncorrected, &curve_x, "x2", "y2", "#122b55", bin_size)?,
        unstable_corrected_hist = histogram_trace(&unstable_corrected, "x3", "y3", "#b84d3f", bin_size)?,
        unstable_corrected_curve = normal_curve_trace(&unstable_corrected, &curve_x, "x3", "y3", "#5d1f18", bin_size)?,
        unstable_uncorrected_hist = histogram_trace(&unstable_uncorrected, "x4", "y4", "#d27a63", bin_size)?,
        unstable_uncorrected_curve = normal_curve_trace(&unstable_uncorrected, &curve_x, "x4", "y4", "#5d1f18", bin_size)?,
        title = serde_json::to_string(&title)?,
    );
    fs::write(path, html)?;
    Ok(())
}

fn write_theoretical_norm_score_distribution_html(path: &Path, rows: &[StudyRow]) -> Result<()> {
    let stable_corrected = theoretical_norm_distribution(rows, true, true);
    let stable_uncorrected = theoretical_norm_distribution(rows, true, false);
    let unstable_corrected = theoretical_norm_distribution(rows, false, true);
    let unstable_uncorrected = theoretical_norm_distribution(rows, false, false);
    let bin_size = 0.025;
    let curve_x: Vec<f64> = (0..=200).map(|i| i as f64 * 0.005).collect();
    let title = format!(
        "Theoretical normalized score distributions (stable corrected={}, stable uncorrected={}, unstable corrected={}, unstable uncorrected={})",
        stable_corrected.values.len(),
        stable_uncorrected.values.len(),
        unstable_corrected.values.len(),
        unstable_uncorrected.values.len()
    );
    let html = format!(
        r#"<!doctype html>
<html>
<head>
  <meta charset="utf-8">
  <title>Theoretical normalized score distributions</title>
  <script src="https://cdn.plot.ly/plotly-2.35.2.min.js"></script>
  <style>html, body, #plot {{ width: 100%; height: 100%; margin: 0; }}</style>
</head>
<body>
  <div id="plot"></div>
  <script>
    const traces = [
      {stable_corrected_hist},
      {stable_corrected_curve},
      {stable_uncorrected_hist},
      {stable_uncorrected_curve},
      {unstable_corrected_hist},
      {unstable_corrected_curve},
      {unstable_uncorrected_hist},
      {unstable_uncorrected_curve}
    ];
    const layout = {{
      title: {title},
      grid: {{ rows: 2, columns: 2, pattern: 'independent' }},
      barmode: 'overlay',
      bargap: 0.05,
      xaxis: {{ title: 'corrected normalized peak', range: [0, 1] }},
      yaxis: {{ title: 'count' }},
      xaxis2: {{ title: 'uncorrected normalized peak', range: [0, 1] }},
      yaxis2: {{ title: 'count' }},
      xaxis3: {{ title: 'corrected normalized peak', range: [0, 1] }},
      yaxis3: {{ title: 'count' }},
      xaxis4: {{ title: 'uncorrected normalized peak', range: [0, 1] }},
      yaxis4: {{ title: 'count' }},
      annotations: [
        {{ text: 'stable corrected', x: 0.225, y: 1.06, xref: 'paper', yref: 'paper', showarrow: false, font: {{ size: 15 }} }},
        {{ text: 'stable uncorrected', x: 0.775, y: 1.06, xref: 'paper', yref: 'paper', showarrow: false, font: {{ size: 15 }} }},
        {{ text: 'unstable corrected', x: 0.225, y: 0.46, xref: 'paper', yref: 'paper', showarrow: false, font: {{ size: 15 }} }},
        {{ text: 'unstable uncorrected', x: 0.775, y: 0.46, xref: 'paper', yref: 'paper', showarrow: false, font: {{ size: 15 }} }}
      ],
      margin: {{ l: 64, r: 24, b: 56, t: 82 }},
      showlegend: false
    }};
    Plotly.newPlot('plot', traces, layout, {{ responsive: true }});
  </script>
</body>
</html>
"#,
        stable_corrected_hist = slide_colored_histogram_trace_range(&stable_corrected, "x", "y", 0.0, 1.0, bin_size, true)?,
        stable_corrected_curve = normal_curve_trace(&stable_corrected, &curve_x, "x", "y", "#122b55", bin_size)?,
        stable_uncorrected_hist = slide_colored_histogram_trace_range(&stable_uncorrected, "x2", "y2", 0.0, 1.0, bin_size, false)?,
        stable_uncorrected_curve = normal_curve_trace(&stable_uncorrected, &curve_x, "x2", "y2", "#122b55", bin_size)?,
        unstable_corrected_hist = slide_colored_histogram_trace_range(&unstable_corrected, "x3", "y3", 0.0, 1.0, bin_size, false)?,
        unstable_corrected_curve = normal_curve_trace(&unstable_corrected, &curve_x, "x3", "y3", "#5d1f18", bin_size)?,
        unstable_uncorrected_hist = slide_colored_histogram_trace_range(&unstable_uncorrected, "x4", "y4", 0.0, 1.0, bin_size, false)?,
        unstable_uncorrected_curve = normal_curve_trace(&unstable_uncorrected, &curve_x, "x4", "y4", "#5d1f18", bin_size)?,
        title = serde_json::to_string(&title)?,
    );
    fs::write(path, html)?;
    Ok(())
}

fn write_preprocessed_theoretical_norm_score_distribution_html(path: &Path, rows: &[StudyRow]) -> Result<()> {
    let stable_corrected = preprocessed_theoretical_norm_distribution(rows, true, true);
    let stable_uncorrected = preprocessed_theoretical_norm_distribution(rows, true, false);
    let unstable_corrected = preprocessed_theoretical_norm_distribution(rows, false, true);
    let unstable_uncorrected = preprocessed_theoretical_norm_distribution(rows, false, false);
    let bin_size = 0.025;
    let curve_x: Vec<f64> = (0..=200).map(|i| i as f64 * 0.005).collect();
    let title = format!(
        "Preprocessed theoretical normalized score distributions (stable corrected={}, stable uncorrected={}, unstable corrected={}, unstable uncorrected={})",
        stable_corrected.values.len(),
        stable_uncorrected.values.len(),
        unstable_corrected.values.len(),
        unstable_uncorrected.values.len()
    );
    let html = format!(
        r#"<!doctype html>
<html>
<head>
  <meta charset="utf-8">
  <title>Preprocessed theoretical normalized score distributions</title>
  <script src="https://cdn.plot.ly/plotly-2.35.2.min.js"></script>
  <style>html, body, #plot {{ width: 100%; height: 100%; margin: 0; }}</style>
</head>
<body>
  <div id="plot"></div>
  <script>
    const traces = [
      {stable_corrected_hist},
      {stable_corrected_curve},
      {stable_uncorrected_hist},
      {stable_uncorrected_curve},
      {unstable_corrected_hist},
      {unstable_corrected_curve},
      {unstable_uncorrected_hist},
      {unstable_uncorrected_curve}
    ];
    const layout = {{
      title: {title},
      grid: {{ rows: 2, columns: 2, pattern: 'independent' }},
      barmode: 'overlay',
      bargap: 0.05,
      xaxis: {{ title: 'corrected preprocessed normalized peak', range: [0, 1] }},
      yaxis: {{ title: 'count' }},
      xaxis2: {{ title: 'uncorrected preprocessed normalized peak', range: [0, 1] }},
      yaxis2: {{ title: 'count' }},
      xaxis3: {{ title: 'corrected preprocessed normalized peak', range: [0, 1] }},
      yaxis3: {{ title: 'count' }},
      xaxis4: {{ title: 'uncorrected preprocessed normalized peak', range: [0, 1] }},
      yaxis4: {{ title: 'count' }},
      annotations: [
        {{ text: 'stable corrected', x: 0.225, y: 1.06, xref: 'paper', yref: 'paper', showarrow: false, font: {{ size: 15 }} }},
        {{ text: 'stable uncorrected', x: 0.775, y: 1.06, xref: 'paper', yref: 'paper', showarrow: false, font: {{ size: 15 }} }},
        {{ text: 'unstable corrected', x: 0.225, y: 0.46, xref: 'paper', yref: 'paper', showarrow: false, font: {{ size: 15 }} }},
        {{ text: 'unstable uncorrected', x: 0.775, y: 0.46, xref: 'paper', yref: 'paper', showarrow: false, font: {{ size: 15 }} }}
      ],
      margin: {{ l: 64, r: 24, b: 56, t: 82 }},
      showlegend: false
    }};
    Plotly.newPlot('plot', traces, layout, {{ responsive: true }});
  </script>
</body>
</html>
"#,
        stable_corrected_hist = slide_colored_histogram_trace_range(&stable_corrected, "x", "y", 0.0, 1.0, bin_size, true)?,
        stable_corrected_curve = normal_curve_trace(&stable_corrected, &curve_x, "x", "y", "#122b55", bin_size)?,
        stable_uncorrected_hist = slide_colored_histogram_trace_range(&stable_uncorrected, "x2", "y2", 0.0, 1.0, bin_size, false)?,
        stable_uncorrected_curve = normal_curve_trace(&stable_uncorrected, &curve_x, "x2", "y2", "#122b55", bin_size)?,
        unstable_corrected_hist = slide_colored_histogram_trace_range(&unstable_corrected, "x3", "y3", 0.0, 1.0, bin_size, false)?,
        unstable_corrected_curve = normal_curve_trace(&unstable_corrected, &curve_x, "x3", "y3", "#5d1f18", bin_size)?,
        unstable_uncorrected_hist = slide_colored_histogram_trace_range(&unstable_uncorrected, "x4", "y4", 0.0, 1.0, bin_size, false)?,
        unstable_uncorrected_curve = normal_curve_trace(&unstable_uncorrected, &curve_x, "x4", "y4", "#5d1f18", bin_size)?,
        title = serde_json::to_string(&title)?,
    );
    fs::write(path, html)?;
    Ok(())
}

struct ScoreDistribution {
    values: Vec<f64>,
    slide_ratios: Vec<f64>,
    mean: f64,
    std_dev: f64,
}

fn score_distribution(rows: &[StudyRow], stable: bool, corrected: bool) -> ScoreDistribution {
    let pairs: Vec<(f64, f64)> = rows
        .iter()
        .filter(|row| row.chart_stable == Some(stable))
        .filter_map(|row| {
            let value = if corrected {
                row.log_raw_residual
            } else {
                row.uncorrected_log_raw_residual
            };
            value.is_finite().then_some((value, row.slide_ratio.unwrap_or(0.0)))
        })
        .collect();
    distribution_from_pairs(pairs)
}

fn theoretical_norm_distribution(rows: &[StudyRow], stable: bool, corrected: bool) -> ScoreDistribution {
    let pairs: Vec<(f64, f64)> = rows
        .iter()
        .filter(|row| row.chart_stable == Some(stable))
        .filter_map(|row| {
            let value = if corrected {
                row.normalized_peak
            } else {
                row.uncorrected_normalized_peak
            };
            value.is_finite().then_some((value, row.slide_ratio.unwrap_or(0.0)))
        })
        .collect();
    distribution_from_pairs(pairs)
}

fn preprocessed_theoretical_norm_distribution(rows: &[StudyRow], stable: bool, corrected: bool) -> ScoreDistribution {
    let pairs: Vec<(f64, f64)> = rows
        .iter()
        .filter(|row| row.chart_stable == Some(stable))
        .filter_map(|row| {
            let value = if corrected {
                row.preprocessed_normalized_peak
            } else {
                row.preprocessed_uncorrected_normalized_peak
            }?;
            value.is_finite().then_some((value, row.slide_ratio.unwrap_or(0.0)))
        })
        .collect();
    distribution_from_pairs(pairs)
}

fn distribution_from_pairs(pairs: Vec<(f64, f64)>) -> ScoreDistribution {
    let (values, slide_ratios): (Vec<_>, Vec<_>) = pairs.into_iter().unzip();
    let mean = if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    };
    let variance = if values.len() <= 1 {
        0.0
    } else {
        values.iter().map(|value| (value - mean).powi(2)).sum::<f64>() / (values.len() - 1) as f64
    };
    ScoreDistribution {
        values,
        slide_ratios,
        mean,
        std_dev: variance.sqrt(),
    }
}

fn histogram_trace(dist: &ScoreDistribution, xaxis: &str, yaxis: &str, color: &str, bin_size: f64) -> Result<String> {
    histogram_trace_range(dist, xaxis, yaxis, color, -0.75, 0.45, bin_size)
}

fn histogram_trace_range(dist: &ScoreDistribution, xaxis: &str, yaxis: &str, color: &str, start: f64, end: f64, bin_size: f64) -> Result<String> {
    Ok(format!(
        "{{ type: 'histogram', x: {values}, xaxis: '{xaxis}', yaxis: '{yaxis}', autobinx: false, xbins: {{ start: {start}, end: {end}, size: {bin_size} }}, marker: {{ color: '{color}', opacity: 0.62 }}, hovertemplate: 'score bin: %{{x:.3f}}<br>count: %{{y}}<extra></extra>' }}",
        values = serde_json::to_string(&dist.values)?,
    ))
}

fn slide_colored_histogram_trace_range(
    dist: &ScoreDistribution,
    xaxis: &str,
    yaxis: &str,
    start: f64,
    end: f64,
    bin_size: f64,
    showscale: bool,
) -> Result<String> {
    let bins = ((end - start) / bin_size).ceil().max(0.0) as usize;
    let mut counts = vec![0usize; bins];
    let mut slide_sums = vec![0.0; bins];
    for (&value, &slide_ratio) in dist.values.iter().zip(&dist.slide_ratios) {
        if value < start || value > end {
            continue;
        }
        let mut index = ((value - start) / bin_size).floor() as usize;
        if index >= bins {
            index = bins.saturating_sub(1);
        }
        counts[index] += 1;
        slide_sums[index] += slide_ratio;
    }

    let x: Vec<f64> = (0..bins).map(|index| start + (index as f64 + 0.5) * bin_size).collect();
    let y: Vec<usize> = counts.clone();
    let color: Vec<f64> = counts
        .iter()
        .zip(slide_sums)
        .map(|(&count, sum)| if count == 0 { 0.0 } else { sum / count as f64 })
        .collect();
    let text: Vec<String> = counts
        .iter()
        .zip(&color)
        .map(|(&count, &avg_slide)| format!("count: {count}<br>avg slide ratio: {:.1}%", avg_slide * 100.0))
        .collect();

    Ok(format!(
        "{{ type: 'bar', x: {x}, y: {y}, width: {bin_size}, xaxis: '{xaxis}', yaxis: '{yaxis}', marker: {{ color: {color}, colorscale: 'YlOrRd', cmin: 0, cmax: 1, showscale: {showscale}, colorbar: {{ title: 'avg slide ratio' }}, opacity: 0.72 }}, text: {text}, hovertemplate: 'score bin center: %{{x:.3f}}<br>%{{text}}<extra></extra>' }}",
        x = serde_json::to_string(&x)?,
        y = serde_json::to_string(&y)?,
        color = serde_json::to_string(&color)?,
        text = serde_json::to_string(&text)?,
        showscale = if showscale { "true" } else { "false" },
    ))
}

fn normal_curve_trace(dist: &ScoreDistribution, xs: &[f64], xaxis: &str, yaxis: &str, color: &str, bin_size: f64) -> Result<String> {
    let ys: Vec<f64> = xs
        .iter()
        .map(|&x| normal_count_density(x, dist.mean, dist.std_dev, dist.values.len(), bin_size))
        .collect();
    Ok(format!(
        "{{ type: 'scatter', mode: 'lines', x: {xs}, y: {ys}, xaxis: '{xaxis}', yaxis: '{yaxis}', line: {{ color: '{color}', width: 2 }}, hovertemplate: 'normal fit<br>score: %{{x:.3f}}<br>expected count: %{{y:.2f}}<extra></extra>' }}",
        xs = serde_json::to_string(xs)?,
        ys = serde_json::to_string(&ys)?,
    ))
}

fn normal_count_density(x: f64, mean: f64, std_dev: f64, count: usize, bin_size: f64) -> f64 {
    if count == 0 || std_dev <= 1e-9 {
        return 0.0;
    }
    let z = (x - mean) / std_dev;
    let pdf = (-0.5 * z * z).exp() / (std_dev * (2.0 * std::f64::consts::PI).sqrt());
    pdf * count as f64 * bin_size
}

fn write_theoretical_normalized_html(path: &Path, rows: &[StudyRow]) -> Result<()> {
    let points = theoretical_points(rows);
    let note_fit = linear_fit(&points.log_note, &points.corrected_log_norm);
    let audio_fit = linear_fit(&points.log_audio, &points.corrected_log_norm);
    let un_note_fit = linear_fit(&points.log_note, &points.uncorrected_log_norm);
    let un_audio_fit = linear_fit(&points.log_audio, &points.uncorrected_log_norm);
    let note_line_x = range_line(&points.log_note);
    let audio_line_x = range_line(&points.log_audio);
    let max_lag_ms = points.lag_ms.iter().copied().fold(0.0, f64::max).max(1.0);
    let title = format!(
        "Theory 0.5/0.5 normalized peak residual checks (n={}, corrected slopes: note={:.3}, audio={:.3}; uncorrected slopes: note={:.3}, audio={:.3})",
        points.log_note.len(),
        note_fit.slope,
        audio_fit.slope,
        un_note_fit.slope,
        un_audio_fit.slope
    );
    let html = format!(
        r#"<!doctype html>
<html>
<head>
  <meta charset="utf-8">
  <title>Theoretical normalized correlation</title>
  <script src="https://cdn.plot.ly/plotly-2.35.2.min.js"></script>
  <style>html, body, #plot {{ width: 100%; height: 100%; margin: 0; }}</style>
</head>
<body>
  <div id="plot"></div>
  <script>
    const correctedNote = {scatter_note_corrected};
    const correctedAudio = {scatter_audio_corrected};
    const uncorrectedNote = {scatter_note_uncorrected};
    const uncorrectedAudio = {scatter_audio_uncorrected};
    const traces = [
      correctedNote,
      {line_note_corrected},
      correctedAudio,
      {line_audio_corrected},
      uncorrectedNote,
      {line_note_uncorrected},
      uncorrectedAudio,
      {line_audio_uncorrected}
    ];
    const layout = {{
      title: {title},
      grid: {{ rows: 2, columns: 2, pattern: 'independent' }},
      xaxis: {{ title: 'log10(note energy)' }},
      yaxis: {{ title: 'log10(corrected normalized peak)' }},
      xaxis2: {{ title: 'log10(audio energy)' }},
      yaxis2: {{ title: 'log10(corrected normalized peak)' }},
      xaxis3: {{ title: 'log10(note energy)' }},
      yaxis3: {{ title: 'log10(uncorrected normalized peak)' }},
      xaxis4: {{ title: 'log10(audio energy)' }},
      yaxis4: {{ title: 'log10(uncorrected normalized peak)' }},
      annotations: [
        {{ text: 'normalized = raw / sqrt(note_energy * audio_energy)', x: 0.5, y: 1.08, xref: 'paper', yref: 'paper', showarrow: false, font: {{ size: 14 }} }},
        {{ text: 'corrected vs note energy', x: 0.225, y: 1.0, xref: 'paper', yref: 'paper', showarrow: false, font: {{ size: 15 }} }},
        {{ text: 'corrected vs audio energy', x: 0.775, y: 1.0, xref: 'paper', yref: 'paper', showarrow: false, font: {{ size: 15 }} }},
        {{ text: 'uncorrected vs note energy', x: 0.225, y: 0.46, xref: 'paper', yref: 'paper', showarrow: false, font: {{ size: 15 }} }},
        {{ text: 'uncorrected vs audio energy', x: 0.775, y: 0.46, xref: 'paper', yref: 'paper', showarrow: false, font: {{ size: 15 }} }}
      ],
      margin: {{ l: 64, r: 24, b: 56, t: 94 }},
      legend: {{ orientation: 'h', y: -0.16 }}
    }};
    Plotly.newPlot('plot', traces, layout, {{ responsive: true }});
  </script>
</body>
</html>
"#,
        scatter_note_corrected = normalized_scatter_trace(
            "corrected",
            &points.log_note,
            &points.corrected_log_norm,
            &points.lag_ms,
            &points.text,
            "x",
            "y",
            true,
            max_lag_ms
        )?,
        scatter_note_uncorrected = normalized_scatter_trace(
            "uncorrected",
            &points.log_note,
            &points.uncorrected_log_norm,
            &points.lag_ms,
            &points.text,
            "x3",
            "y3",
            false,
            max_lag_ms
        )?,
        scatter_audio_corrected = normalized_scatter_trace(
            "corrected",
            &points.log_audio,
            &points.corrected_log_norm,
            &points.lag_ms,
            &points.text,
            "x2",
            "y2",
            false,
            max_lag_ms
        )?,
        scatter_audio_uncorrected = normalized_scatter_trace(
            "uncorrected",
            &points.log_audio,
            &points.uncorrected_log_norm,
            &points.lag_ms,
            &points.text,
            "x4",
            "y4",
            false,
            max_lag_ms
        )?,
        line_note_corrected = trend_line_trace("corrected trend", &note_line_x, note_fit, "x", "y", "#1f5fb8")?,
        line_audio_corrected = trend_line_trace("corrected trend", &audio_line_x, audio_fit, "x2", "y2", "#1f5fb8")?,
        line_note_uncorrected = trend_line_trace("uncorrected trend", &note_line_x, un_note_fit, "x3", "y3", "#b84d3f")?,
        line_audio_uncorrected = trend_line_trace("uncorrected trend", &audio_line_x, un_audio_fit, "x4", "y4", "#b84d3f")?,
        title = serde_json::to_string(&title)?,
    );
    fs::write(path, html)?;
    Ok(())
}

fn write_theoretical_normalized_3d_html(path: &Path, rows: &[StudyRow], mode: PlotMode) -> Result<()> {
    let filtered: Vec<&StudyRow> = rows.iter().filter(|row| mode.normalized_peak(row) > 0.0).collect();
    let x: Vec<f64> = filtered.iter().map(|row| row.note_energy.max(1e-12).log10()).collect();
    let y: Vec<f64> = filtered.iter().map(|row| row.audio_energy.max(1e-12).log10()).collect();
    let z: Vec<f64> = filtered.iter().map(|row| mode.normalized_peak(row).log10()).collect();
    let lag_ms: Vec<f64> = filtered.iter().map(|row| row.lag_sec.abs() * 1000.0).collect();
    let max_lag_ms = lag_ms.iter().copied().fold(0.0, f64::max).max(1.0);
    let text: Vec<String> = filtered
        .iter()
        .map(|row| {
            format!(
                "#{} {}<br>status: {}<br>lag: {:+.0}ms<br>normalized: {:.4}",
                row.chart_id,
                row.chart_name,
                chart_status_label(row),
                row.lag_sec * 1000.0,
                mode.normalized_peak(row)
            )
        })
        .collect();
    let title = format!("Theory 0.5/0.5 normalized peak 3D: {} (n={})", mode.title(), filtered.len());
    let html = format!(
        r#"<!doctype html>
<html>
<head>
  <meta charset="utf-8">
  <title>Theoretical normalized peak 3D</title>
  <script src="https://cdn.plot.ly/plotly-2.35.2.min.js"></script>
  <style>html, body, #plot {{ width: 100%; height: 100%; margin: 0; }}</style>
</head>
<body>
  <div id="plot"></div>
  <script>
    const trace = {{
      type: 'scatter3d',
      mode: 'markers',
      x: {x},
      y: {y},
      z: {z},
      text: {text},
      hovertemplate: '%{{text}}<br>log note energy: %{{x:.3f}}<br>log audio energy: %{{y:.3f}}<br>log normalized peak: %{{z:.4f}}<br>|lag|: %{{marker.color:.0f}}ms<extra></extra>',
      marker: {{
        size: 4,
        color: {lag_ms},
        colorscale: 'Viridis',
        cmin: 0,
        cmax: {max_lag_ms},
        colorbar: {{ title: '|lag| ms' }},
        opacity: 0.82
      }}
    }};
    const layout = {{
      title: {title},
      scene: {{
        xaxis: {{ title: 'log10(note energy)' }},
        yaxis: {{ title: 'log10(audio energy)' }},
        zaxis: {{ title: 'log10(normalized peak)' }}
      }},
      margin: {{ l: 0, r: 0, b: 0, t: 42 }}
    }};
    Plotly.newPlot('plot', [trace], layout, {{ responsive: true }});
  </script>
</body>
</html>
"#,
        x = serde_json::to_string(&x)?,
        y = serde_json::to_string(&y)?,
        z = serde_json::to_string(&z)?,
        text = serde_json::to_string(&text)?,
        lag_ms = serde_json::to_string(&lag_ms)?,
        max_lag_ms = max_lag_ms,
        title = serde_json::to_string(&title)?,
    );
    fs::write(path, html)?;
    Ok(())
}

struct TheoreticalPoints {
    log_note: Vec<f64>,
    log_audio: Vec<f64>,
    corrected_log_norm: Vec<f64>,
    uncorrected_log_norm: Vec<f64>,
    lag_ms: Vec<f64>,
    text: Vec<String>,
}

fn theoretical_points(rows: &[StudyRow]) -> TheoreticalPoints {
    let mut points = TheoreticalPoints {
        log_note: Vec::new(),
        log_audio: Vec::new(),
        corrected_log_norm: Vec::new(),
        uncorrected_log_norm: Vec::new(),
        lag_ms: Vec::new(),
        text: Vec::new(),
    };
    for row in rows {
        if row.normalized_peak <= 0.0 || row.uncorrected_normalized_peak <= 0.0 {
            continue;
        }
        points.log_note.push(row.note_energy.max(1e-12).log10());
        points.log_audio.push(row.audio_energy.max(1e-12).log10());
        points.corrected_log_norm.push(row.normalized_peak.log10());
        points.uncorrected_log_norm.push(row.uncorrected_normalized_peak.log10());
        points.lag_ms.push(row.lag_sec.abs() * 1000.0);
        points.text.push(format!(
            "#{} {}<br>status: {}<br>lag: {:+.0}ms<br>norm: {:.4}<br>norm0: {:.4}",
            row.chart_id,
            row.chart_name,
            chart_status_label(row),
            row.lag_sec * 1000.0,
            row.normalized_peak,
            row.uncorrected_normalized_peak
        ));
    }
    points
}

#[derive(Clone, Copy)]
struct LinearFit {
    intercept: f64,
    slope: f64,
}

fn linear_fit(xs: &[f64], ys: &[f64]) -> LinearFit {
    if xs.len() < 2 || xs.len() != ys.len() {
        return LinearFit { intercept: 0.0, slope: 0.0 };
    }
    let mean_x = xs.iter().sum::<f64>() / xs.len() as f64;
    let mean_y = ys.iter().sum::<f64>() / ys.len() as f64;
    let cov = xs.iter().zip(ys).map(|(x, y)| (x - mean_x) * (y - mean_y)).sum::<f64>();
    let var = xs.iter().map(|x| (x - mean_x).powi(2)).sum::<f64>();
    let slope = if var > 0.0 { cov / var } else { 0.0 };
    LinearFit {
        intercept: mean_y - slope * mean_x,
        slope,
    }
}

fn range_line(values: &[f64]) -> Vec<f64> {
    let (min, max) = min_max(values);
    vec![min, max]
}

#[allow(clippy::too_many_arguments)]
fn normalized_scatter_trace(
    name: &str,
    x: &[f64],
    y: &[f64],
    lag_ms: &[f64],
    text: &[String],
    xaxis: &str,
    yaxis: &str,
    showscale: bool,
    max_lag_ms: f64,
) -> Result<String> {
    Ok(format!(
        "{{ type: 'scatter', mode: 'markers', name: '{name}', x: {x}, y: {y}, text: {text}, xaxis: '{xaxis}', yaxis: '{yaxis}', marker: {{ size: 6, color: {color}, colorscale: 'Viridis', cmin: 0, cmax: {max_lag_ms}, showscale: {showscale}, colorbar: {{ title: '|lag| ms' }}, opacity: 0.72 }}, hovertemplate: '%{{text}}<br>x: %{{x:.3f}}<br>log normalized: %{{y:.4f}}<br>|lag|: %{{marker.color:.0f}}ms<extra></extra>' }}",
        x = serde_json::to_string(x)?,
        y = serde_json::to_string(y)?,
        text = serde_json::to_string(text)?,
        color = serde_json::to_string(lag_ms)?,
        showscale = if showscale { "true" } else { "false" },
    ))
}

fn trend_line_trace(name: &str, xs: &[f64], fit: LinearFit, xaxis: &str, yaxis: &str, color: &str) -> Result<String> {
    let ys: Vec<f64> = xs.iter().map(|x| fit.intercept + fit.slope * x).collect();
    let label = format!("{name} slope={:.4}", fit.slope);
    Ok(format!(
        "{{ type: 'scatter', mode: 'lines', name: {label}, x: {xs}, y: {ys}, xaxis: '{xaxis}', yaxis: '{yaxis}', line: {{ color: '{color}', width: 2 }}, hovertemplate: {label_extra} }}",
        label = serde_json::to_string(&label)?,
        label_extra = serde_json::to_string(&format!("{label}<extra></extra>"))?,
        xs = serde_json::to_string(xs)?,
        ys = serde_json::to_string(&ys)?,
    ))
}

fn chart_status_label(row: &StudyRow) -> &'static str {
    match (row.chart_stable, row.chart_ranked, row.chart_reviewed, row.chart_stable_request) {
        (Some(true), Some(true), _, _) => "ranked",
        (Some(true), Some(false), _, _) => "special",
        (Some(false), _, Some(true), Some(true)) => "stable-request",
        (Some(false), _, Some(true), _) => "unstable-reviewed",
        (Some(false), _, Some(false), _) => "unreviewed",
        _ => "unknown",
    }
}

fn rating_label(value: Option<f32>) -> String {
    value.map_or_else(|| "NaN".to_owned(), |value| format!("{value:.2} / 5.00"))
}

fn draw_plot(path: &Path, rows: &[StudyRow], mode: PlotMode, color_abs: f64, plane: FittedPlane) -> Result<()> {
    let root = BitMapBackend::new(path, (1200, 900)).into_drawing_area();
    root.fill(&RGBColor(250, 250, 248))?;

    let mut chart = ChartBuilder::on(&root)
        .caption(
            format!("Auto-offset energy study: {} residual colors (R2={:.3}, rmse={:.3})", mode.title(), plane.r2, plane.rmse),
            ("sans-serif", 28).into_font(),
        )
        .margin(28)
        .x_label_area_size(48)
        .y_label_area_size(58)
        .build_cartesian_2d(-0.75f64..0.9f64, -0.75f64..0.9f64)?;

    chart
        .configure_mesh()
        .x_desc("3D projection: log10(note energy) + log10(audio energy)")
        .y_desc("3D projection: log10(raw peak)")
        .light_line_style(RGBColor(225, 225, 225))
        .draw()?;

    if rows.is_empty() {
        root.present()?;
        return Ok(());
    }

    let xs: Vec<f64> = rows.iter().map(|r| (r.note_energy.max(1e-12)).log10()).collect();
    let ys: Vec<f64> = rows.iter().map(|r| (r.audio_energy.max(1e-12)).log10()).collect();
    let zs: Vec<f64> = rows.iter().map(|r| mode.raw_peak(r).max(1e-12).log10()).collect();
    let (xmin, xmax) = min_max(&xs);
    let (ymin, ymax) = min_max(&ys);
    let (zmin, zmax) = min_max(&zs);

    chart.draw_series(rows.iter().zip(&xs).zip(ys.iter().zip(&zs)).map(|((row, &x), (&y, &z))| {
        let px = norm(x, xmin, xmax) - 0.5;
        let py = norm(y, ymin, ymax) - 0.5;
        let pz = norm(z, zmin, zmax) - 0.5;
        let sx = px + py * 0.38;
        let sy = pz - py * 0.30;
        Circle::new((sx, sy), 4, ShapeStyle::from(&diverging_heat(mode.log_residual(row), color_abs)).filled())
    }))?;

    draw_color_legend(&root, color_abs)?;
    root.present()?;
    Ok(())
}

fn min_max(values: &[f64]) -> (f64, f64) {
    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if (max - min).abs() < 1e-9 {
        (min, min + 1.0)
    } else {
        (min, max)
    }
}

fn norm(value: f64, min: f64, max: f64) -> f64 {
    ((value - min) / (max - min)).clamp(0.0, 1.0)
}

fn diverging_heat(value: f64, max_abs: f64) -> RGBColor {
    let t = (value / max_abs.max(1e-9)).clamp(-1.0, 1.0);
    if t >= 0.0 {
        mix_color(RGBColor(245, 245, 242), RGBColor(190, 45, 38), t)
    } else {
        mix_color(RGBColor(245, 245, 242), RGBColor(45, 93, 171), -t)
    }
}

fn mix_color(a: RGBColor, b: RGBColor, t: f64) -> RGBColor {
    let mix = |x: u8, y: u8| (x as f64 + (y as f64 - x as f64) * t).round() as u8;
    RGBColor(mix(a.0, b.0), mix(a.1, b.1), mix(a.2, b.2))
}

fn draw_color_legend(root: &DrawingArea<BitMapBackend<'_>, plotters::coord::Shift>, color_abs: f64) -> Result<()> {
    let x0 = 1030;
    let y0 = 140;
    let h = 300;
    for i in 0..h {
        let t = 1.0 - i as f64 / (h - 1) as f64;
        let value = color_abs * (2.0 * t - 1.0);
        root.draw(&Rectangle::new([(x0, y0 + i), (x0 + 24, y0 + i + 1)], diverging_heat(value, color_abs).filled()))?;
    }
    root.draw(&Text::new("log raw", (x0 - 18, y0 - 28), ("sans-serif", 18).into_font()))?;
    root.draw(&Text::new("/ fitted", (x0 - 18, y0 - 8), ("sans-serif", 18).into_font()))?;
    root.draw(&Text::new(format!("+{color_abs:.2}"), (x0 + 34, y0 + 6), ("sans-serif", 16).into_font()))?;
    root.draw(&Text::new("0", (x0 + 34, y0 + h / 2 + 5), ("sans-serif", 16).into_font()))?;
    root.draw(&Text::new(format!("-{color_abs:.2}"), (x0 + 34, y0 + h - 4), ("sans-serif", 16).into_font()))?;
    Ok(())
}
