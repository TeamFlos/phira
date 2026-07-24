use anyhow::{bail, Context, Result};
use clap::Parser;
use futures_util::{stream, StreamExt};
use prpr::{
    core::{BpmList, Chart, NoteKind, Triple},
    fs::{fs_from_file, load_info, FileSystem},
    info::{ChartFormat, ChartInfo},
    parse::{parse_pec, parse_phigros, parse_rpe},
};
use prpr_auto_offset::{estimate_with, AlignConfig, AutoOffsetNoteKind, NoteEvent, PreprocessedNoteGaussian, SuperFlux};
use serde::Deserialize;
use std::{
    collections::{BTreeSet, HashMap},
    fs::{self, File},
    io::{Cursor, Write},
    path::{Path, PathBuf},
    time::Duration,
};

const API_URL: &str = "https://phira.5wyxi.com";
const DEFAULT_ROOT: &str = "data/auto-offset-study";
const REPORT_FILE: &str = "study-report.html";

#[derive(Parser)]
#[command(name = "prpr-auto-offset-study")]
#[command(about = "Download chart samples and study preprocessed auto-offset scores")]
struct Cli {
    #[arg(long, default_value = DEFAULT_ROOT)]
    root: PathBuf,
    #[arg(short, long, default_value_t = 300)]
    samples: usize,
    #[arg(long)]
    download: bool,
    #[arg(long, default_value_t = 20)]
    pages: u64,
    #[arg(long, default_value_t = 30)]
    page_num: u64,
    #[arg(long, default_value = "-updated")]
    order: String,
    #[arg(long, default_value_t = 0.30)]
    range: f64,
    #[arg(long, default_value_t = 0.005)]
    interval: f64,
    #[arg(long, default_value_t = 0.02)]
    blur_sigma: f64,
    #[arg(long)]
    recompute: bool,
    #[arg(long, default_value_t = default_jobs())]
    jobs: usize,
    #[arg(long)]
    allow_shrink: bool,
    #[arg(long, default_value_t = 8000)]
    request_timeout_ms: u64,
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
    ranked: bool,
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

#[derive(Debug, Clone)]
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
    reliable: bool,
    drag_ratio: f64,
    chart_listed: Option<bool>,
}

impl StudyRow {
    fn header() -> &'static str {
        "chart_id,chart_name,notes,duration_sec,search_center_sec,suggested_offset_sec,lag_sec,raw_peak,note_energy,audio_energy,normalized_peak,reliable,drag_ratio,chart_listed"
    }

    fn to_csv(&self) -> String {
        format!(
            "{},{},{},{:.6},{:.6},{:.6},{:.6},{:.9},{:.9},{:.9},{:.9},{},{:.9},{}",
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
            self.reliable,
            self.drag_ratio,
            csv_optional_bool(self.chart_listed),
        )
    }

    fn listing_label(&self) -> &'static str {
        match self.chart_listed {
            Some(true) => "listed",
            Some(false) => "unlisted",
            None => "unknown",
        }
    }
}

#[derive(Debug, Clone)]
struct NoteStats {
    events: Vec<NoteEvent>,
    drags: usize,
}

impl NoteStats {
    fn drag_ratio(&self) -> f64 {
        if self.events.is_empty() {
            0.0
        } else {
            self.drags as f64 / self.events.len() as f64
        }
    }
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

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    fs::create_dir_all(cli.root.join("charts"))?;

    ensure_samples(&cli).await?;
    let mut rows = analyze_samples(&cli).await?;
    enrich_listing_metadata(&cli, &mut rows).await;

    let plane = fit_log_peak_plane(&rows)?;
    write_csv(&cli.root.join("results.csv"), &rows, cli.allow_shrink)?;
    write_report_html(&cli.root.join(REPORT_FILE), &rows, plane)?;

    println!("rows: {}", rows.len());
    println!(
        "fit: log_raw = {:.6} + {:.6}*log_note + {:.6}*log_audio (r2={:.4}, rmse={:.4})",
        plane.intercept, plane.note_coef, plane.audio_coef, plane.r2, plane.rmse
    );
    println!("csv: {}", cli.root.join("results.csv").display());
    println!("report: {}", cli.root.join(REPORT_FILE).display());
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
    let ids: Vec<i32> = cached_chart_ids(&charts_dir)?
        .into_iter()
        .filter(|id| !done.contains(id))
        .take(target_rows.saturating_sub(rows.len()))
        .collect();
    let jobs = cli.jobs.max(1);

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
                println!("analyzed {id}: raw={:.3} score={:.4} lag={:+.0}ms", row.raw_peak, row.normalized_peak, row.lag_sec * 1000.0);
                rows.push(row);
            }
            Err(err) => eprintln!("skip analysis {id}: {err:#}"),
        }
    }

    rows.sort_by_key(|row| row.chart_id);
    Ok(rows)
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
    let note = PreprocessedNoteGaussian::new(note_stats.events.clone(), cli.blur_sigma);
    let search_center = chart_offset + info.offset as f64;
    let config = AlignConfig {
        search_range_sec: cli.range,
        sampling_interval_sec: cli.interval,
        search_center_sec: search_center,
    };
    let result = estimate_with(&audio, &note, duration, &config);

    Ok(StudyRow {
        chart_id: id,
        chart_name: info.name,
        notes: note_stats.events.len(),
        duration_sec: duration,
        search_center_sec: search_center,
        suggested_offset_sec: result.offset,
        lag_sec: result.offset - search_center,
        raw_peak: result.raw_peak,
        note_energy: result.note_energy,
        audio_energy: result.audio_energy,
        normalized_peak: result.correlation,
        reliable: result.reliable,
        drag_ratio: note_stats.drag_ratio(),
        chart_listed: None,
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
    let source = String::from_utf8_lossy(bytes);
    match format {
        ChartFormat::Rpe => parse_rpe(&source, fs, Default::default(), info.use_rpe_170_speed.unwrap_or_default()).await,
        ChartFormat::Pgr => parse_phigros(&source, Default::default()),
        ChartFormat::Pec => parse_pec(&source, Default::default()),
        ChartFormat::Pbc => bail!("pbc charts are not supported by this study tool"),
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
    let drags = events.iter().filter(|event| event.kind == AutoOffsetNoteKind::Drag).count();
    Ok((rpe.meta.offset as f64 / 1000.0, NoteStats { events, drags }))
}

fn rpe_note_kind(kind: u8) -> AutoOffsetNoteKind {
    match kind {
        2 => AutoOffsetNoteKind::Hold,
        3 => AutoOffsetNoteKind::Flick,
        4 => AutoOffsetNoteKind::Drag,
        _ => AutoOffsetNoteKind::Tap,
    }
}

fn infer_chart_format(info: &ChartInfo, bytes: &[u8]) -> ChartFormat {
    info.format.clone().unwrap_or_else(|| {
        if let Ok(text) = String::from_utf8(bytes.to_vec()) {
            if text.trim_start().starts_with('{') {
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
    let drags = events.iter().filter(|event| event.kind == AutoOffsetNoteKind::Drag).count();
    NoteStats { events, drags }
}

fn auto_offset_note_kind(kind: &NoteKind) -> AutoOffsetNoteKind {
    match kind {
        NoteKind::Click => AutoOffsetNoteKind::Tap,
        NoteKind::Hold { .. } => AutoOffsetNoteKind::Hold,
        NoteKind::Flick => AutoOffsetNoteKind::Flick,
        NoteKind::Drag => AutoOffsetNoteKind::Drag,
    }
}

async fn enrich_listing_metadata(cli: &Cli, rows: &mut [StudyRow]) {
    let ids: Vec<i32> = rows.iter().filter(|row| row.chart_listed.is_none()).map(|row| row.chart_id).collect();
    if ids.is_empty() {
        return;
    }

    let client = match reqwest::Client::builder().timeout(Duration::from_millis(cli.request_timeout_ms)).build() {
        Ok(client) => client,
        Err(err) => {
            eprintln!("skip listing metadata fetch: {err:#}");
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
                        metadata.insert(item.id, item.ranked);
                    }
                }
                Err(err) => eprintln!("skip listing metadata chunk {ids_str}: {err:#}"),
            },
            Err(err) => eprintln!("skip listing metadata chunk {ids_str}: {err:#}"),
        }
    }

    let mut filled = 0;
    for row in rows {
        if let Some(listed) = metadata.get(&row.chart_id) {
            row.chart_listed = Some(*listed);
            filled += 1;
        }
    }
    if filled > 0 {
        println!("listing metadata: backfilled {filled} rows from Phira API");
    }
}
fn read_existing_csv(path: &Path) -> Result<Vec<StudyRow>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(path)?;
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let Some(header_line) = lines.next() else {
        return Ok(Vec::new());
    };
    let headers = split_csv_line(header_line);
    let mut rows = Vec::new();
    for line in lines {
        let cols = split_csv_line(line);
        if let Some(row) = parse_existing_row(&headers, &cols)? {
            rows.push(row);
        }
    }
    Ok(rows)
}

fn parse_existing_row(headers: &[String], cols: &[String]) -> Result<Option<StudyRow>> {
    let Some(chart_id) = get_col(headers, cols, "chart_id").and_then(|value| value.parse().ok()) else {
        return Ok(None);
    };
    let Some(note_energy) = parse_col_f64(headers, cols, "note_energy")? else {
        return Ok(None);
    };
    let Some(audio_energy) = parse_col_f64(headers, cols, "audio_energy")? else {
        return Ok(None);
    };

    let normalized_peak = first_f64(headers, cols, &["preprocessed_normalized_peak", "normalized_peak"])?.unwrap_or(0.0);
    let row = StudyRow {
        chart_id,
        chart_name: get_col(headers, cols, "chart_name").unwrap_or_default().to_owned(),
        notes: get_col(headers, cols, "notes").and_then(|value| value.parse().ok()).unwrap_or(0),
        duration_sec: parse_col_f64(headers, cols, "duration_sec")?.unwrap_or(0.0),
        search_center_sec: parse_col_f64(headers, cols, "search_center_sec")?.unwrap_or(0.0),
        suggested_offset_sec: first_f64(headers, cols, &["preprocessed_suggested_offset_sec", "suggested_offset_sec"])?.unwrap_or(0.0),
        lag_sec: first_f64(headers, cols, &["preprocessed_lag_sec", "lag_sec"])?.unwrap_or(0.0),
        raw_peak: first_f64(headers, cols, &["preprocessed_raw_peak", "raw_peak"])?.unwrap_or(0.0),
        note_energy,
        audio_energy,
        normalized_peak,
        reliable: get_col(headers, cols, "reliable")
            .and_then(|value| value.parse().ok())
            .unwrap_or(normalized_peak > 0.2),
        drag_ratio: parse_col_f64(headers, cols, "drag_ratio")?.unwrap_or(0.0),
        chart_listed: get_col(headers, cols, "chart_listed").and_then(parse_optional_bool),
    };
    Ok(Some(row))
}

fn first_f64(headers: &[String], cols: &[String], names: &[&str]) -> Result<Option<f64>> {
    for name in names {
        if let Some(value) = parse_col_f64(headers, cols, name)? {
            return Ok(Some(value));
        }
    }
    Ok(None)
}

fn parse_col_f64(headers: &[String], cols: &[String], name: &str) -> Result<Option<f64>> {
    get_col(headers, cols, name).map_or(Ok(None), |value| if value.is_empty() { Ok(None) } else { Ok(Some(value.parse()?)) })
}

fn get_col<'a>(headers: &[String], cols: &'a [String], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .position(|header| header == name)
        .and_then(|index| cols.get(index))
        .map(String::as_str)
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
            ',' if !quoted => cols.push(std::mem::take(&mut cur)),
            _ => cur.push(ch),
        }
    }
    cols.push(cur);
    cols
}

fn parse_optional_bool(value: &str) -> Option<bool> {
    match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn csv_optional_bool(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "true",
        Some(false) => "false",
        None => "",
    }
}
fn csv_escape(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
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

fn fit_log_peak_plane(rows: &[StudyRow]) -> Result<FittedPlane> {
    let rows: Vec<&StudyRow> = rows
        .iter()
        .filter(|row| row.raw_peak > 0.0 && row.note_energy > 0.0 && row.audio_energy > 0.0)
        .collect();
    if rows.len() < 3 {
        bail!("need at least 3 valid rows to fit log peak plane")
    }

    let mut xtx = [[0.0; 3]; 3];
    let mut xtz = [0.0; 3];
    let mut zs = Vec::with_capacity(rows.len());
    for row in &rows {
        let x = row.note_energy.log10();
        let y = row.audio_energy.log10();
        let z = row.raw_peak.log10();
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
        ss_tot += (z - mean_z).powi(2);
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

fn write_report_html(path: &Path, rows: &[StudyRow], plane: FittedPlane) -> Result<()> {
    let (hist_x, hist_listed, hist_unlisted, hist_unknown) = listing_histograms(rows, 0.025);
    let energy_points: Vec<&StudyRow> = rows
        .iter()
        .filter(|row| row.raw_peak > 0.0 && row.note_energy > 0.0 && row.audio_energy > 0.0)
        .collect();
    let x: Vec<f64> = energy_points.iter().map(|row| row.note_energy.log10()).collect();
    let y: Vec<f64> = energy_points.iter().map(|row| row.audio_energy.log10()).collect();
    let z: Vec<f64> = energy_points.iter().map(|row| row.raw_peak.log10()).collect();
    let residual: Vec<f64> = energy_points
        .iter()
        .map(|row| row.raw_peak.log10() - plane.predict_log_raw(row.note_energy, row.audio_energy))
        .collect();
    let text: Vec<String> = energy_points
        .iter()
        .map(|row| {
            format!(
                "#{} {}<br>notes: {}<br>listing: {}<br>lag: {:+.0}ms<br>score: {:.4}<br>drag ratio: {:.3}<br>raw: {:.3}",
                row.chart_id,
                row.chart_name,
                row.notes,
                row.listing_label(),
                row.lag_sec * 1000.0,
                row.normalized_peak,
                row.drag_ratio,
                row.raw_peak
            )
        })
        .collect();
    let (x_min, x_max) = min_max(&x);
    let (y_min, y_max) = min_max(&y);
    let plane_x = vec![vec![x_min, x_max], vec![x_min, x_max]];
    let plane_y = vec![vec![y_min, y_min], vec![y_max, y_max]];
    let plane_z: Vec<Vec<f64>> = plane_y
        .iter()
        .enumerate()
        .map(|(row_index, row)| {
            row.iter()
                .enumerate()
                .map(|(col_index, &audio)| plane.intercept + plane.note_coef * plane_x[row_index][col_index] + plane.audio_coef * audio)
                .collect()
        })
        .collect();
    let color_abs = percentile_abs(&residual, 0.95).max(0.05);

    let html = format!(
        r#"<!doctype html>
<html>
<head>
  <meta charset="utf-8">
  <title>Auto-offset study report</title>
  <script src="https://cdn.plot.ly/plotly-2.35.2.min.js"></script>
  <style>
    html, body {{ margin: 0; background: #f6f5f1; color: #242424; font-family: system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; }}
    header {{ padding: 20px 28px 12px; }}
    h1 {{ margin: 0 0 6px; font-size: 22px; font-weight: 650; }}
    .summary {{ margin: 0; color: #555; font-size: 14px; }}
    section {{ padding: 0 20px 28px; }}
    .plot {{ width: 100%; height: 72vh; min-height: 520px; background: #fff; border: 1px solid #d9d7cf; box-sizing: border-box; }}
    .spacer {{ height: 20px; }}
  </style>
</head>
<body>
  <header>
    <h1>Auto-offset study report</h1>
    <p class="summary">rows: {row_count}; fit log_raw = {intercept:.3} + {note_coef:.3} log_note + {audio_coef:.3} log_audio, R2={r2:.3}, RMSE={rmse:.3}</p>
  </header>
  <section>
    <div id="score-distribution" class="plot"></div>
    <div class="spacer"></div>
    <div id="energy-space" class="plot"></div>
  </section>
  <script>
    const histListed = {{
      type: 'bar', name: 'listed', x: {hist_x}, y: {hist_listed}, width: 0.023,
      marker: {{ color: '#3d79c9', line: {{ color: '#244a7a', width: 0.5 }} }},
      hovertemplate: 'listed<br>score bin: %{{x:.3f}}<br>charts: %{{y}}<extra></extra>'
    }};
    const histUnlisted = {{
      type: 'bar', name: 'unlisted', x: {hist_x}, y: {hist_unlisted}, width: 0.023,
      marker: {{ color: '#d18a48', line: {{ color: '#7a4f25', width: 0.5 }} }},
      hovertemplate: 'unlisted<br>score bin: %{{x:.3f}}<br>charts: %{{y}}<extra></extra>'
    }};
    const histUnknown = {{
      type: 'bar', name: 'unknown', x: {hist_x}, y: {hist_unknown}, width: 0.023,
      marker: {{ color: '#9a9a9a', line: {{ color: '#5f5f5f', width: 0.5 }} }},
      hovertemplate: 'unknown<br>score bin: %{{x:.3f}}<br>charts: %{{y}}<extra></extra>'
    }};
    Plotly.newPlot('score-distribution', [histListed, histUnlisted, histUnknown], {{
      title: 'Preprocessed normalized score distribution by listing status',
      xaxis: {{ title: 'normalized score', range: [0, 1] }},
      yaxis: {{ title: 'chart count', rangemode: 'tozero' }},
      barmode: 'stack',
      bargap: 0.03,
      margin: {{ l: 58, r: 24, b: 54, t: 54 }}
    }}, {{ responsive: true }});

    const scatter = {{
      type: 'scatter3d', mode: 'markers', name: 'charts', x: {x}, y: {y}, z: {z}, text: {text},
      marker: {{
        size: 3.5, color: {residual}, colorscale: 'RdBu', reversescale: true,
        cmin: -{color_abs}, cmax: {color_abs}, cmid: 0,
        colorbar: {{ title: 'log(raw / fitted)' }}, opacity: 0.78
      }},
      hovertemplate: '%{{text}}<br>log note: %{{x:.3f}}<br>log audio: %{{y:.3f}}<br>log raw: %{{z:.3f}}<br>residual: %{{marker.color:.4f}}<extra></extra>'
    }};
    const fittedPlane = {{
      type: 'surface', name: 'fitted plane', x: {plane_x}, y: {plane_y}, z: {plane_z},
      showscale: false, opacity: 0.34,
      colorscale: [[0, 'rgba(236, 183, 42, 0.34)'], [1, 'rgba(236, 183, 42, 0.34)']],
      hovertemplate: 'fitted plane<br>log note=%{{x:.3f}}<br>log audio=%{{y:.3f}}<br>log raw=%{{z:.3f}}<extra></extra>'
    }};
    Plotly.newPlot('energy-space', [scatter, fittedPlane], {{
      title: 'Corrected raw peak vs note/audio energy',
      scene: {{
        xaxis: {{ title: 'log10(note energy)' }},
        yaxis: {{ title: 'log10(audio energy)' }},
        zaxis: {{ title: 'log10(raw peak)' }}
      }},
      margin: {{ l: 0, r: 0, b: 0, t: 54 }}
    }}, {{ responsive: true }});
  </script>
</body>
</html>
"#,
        row_count = rows.len(),
        intercept = plane.intercept,
        note_coef = plane.note_coef,
        audio_coef = plane.audio_coef,
        r2 = plane.r2,
        rmse = plane.rmse,
        hist_x = serde_json::to_string(&hist_x)?,
        hist_listed = serde_json::to_string(&hist_listed)?,
        hist_unlisted = serde_json::to_string(&hist_unlisted)?,
        hist_unknown = serde_json::to_string(&hist_unknown)?,
        x = serde_json::to_string(&x)?,
        y = serde_json::to_string(&y)?,
        z = serde_json::to_string(&z)?,
        text = serde_json::to_string(&text)?,
        residual = serde_json::to_string(&residual)?,
        color_abs = color_abs,
        plane_x = serde_json::to_string(&plane_x)?,
        plane_y = serde_json::to_string(&plane_y)?,
        plane_z = serde_json::to_string(&plane_z)?,
    );
    fs::write(path, html)?;
    Ok(())
}

fn listing_histograms(rows: &[StudyRow], bin_size: f64) -> (Vec<f64>, Vec<usize>, Vec<usize>, Vec<usize>) {
    let bins = (1.0 / bin_size).ceil() as usize;
    let mut listed = vec![0usize; bins];
    let mut unlisted = vec![0usize; bins];
    let mut unknown = vec![0usize; bins];

    for row in rows {
        let value = row.normalized_peak;
        if !value.is_finite() {
            continue;
        }
        let index = ((value.clamp(0.0, 1.0) / bin_size).floor() as usize).min(bins - 1);
        match row.chart_listed {
            Some(true) => listed[index] += 1,
            Some(false) => unlisted[index] += 1,
            None => unknown[index] += 1,
        }
    }

    let centers = (0..bins).map(|index| (index as f64 + 0.5) * bin_size).collect();
    (centers, listed, unlisted, unknown)
}
fn min_max(values: &[f64]) -> (f64, f64) {
    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if !min.is_finite() || !max.is_finite() {
        (0.0, 1.0)
    } else if (max - min).abs() < 1e-9 {
        (min, min + 1.0)
    } else {
        (min, max)
    }
}

fn percentile_abs(values: &[f64], percentile: f64) -> f64 {
    let mut values: Vec<f64> = values.iter().map(|value| value.abs()).filter(|value| value.is_finite()).collect();
    if values.is_empty() {
        return 0.3;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let index = ((values.len() - 1) as f64 * percentile.clamp(0.0, 1.0)).round() as usize;
    values[index]
}
