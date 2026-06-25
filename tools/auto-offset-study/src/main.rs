use anyhow::{bail, Context, Result};
use clap::Parser;
use futures_util::StreamExt;
use plotters::prelude::*;
use prpr::{
    core::{BpmList, Chart, Triple},
    fs::{fs_from_file, load_info, FileSystem},
    info::{ChartFormat, ChartInfo},
    parse::{parse_pec, parse_phigros, parse_rpe},
};
use prpr_auto_offset::{AlignConfig, NoteGaussian, Signal, SuperFlux};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
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

    /// Target number of downloaded chart samples.
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

    /// Per-request timeout in milliseconds.
    #[arg(long, default_value_t = 8000)]
    request_timeout_ms: u64,

    /// Number of attempts per HTTP request.
    #[arg(long, default_value_t = 10)]
    retries: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteChart {
    id: i32,
    file: String,
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
    reliable: bool,
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
        "chart_id,chart_name,notes,duration_sec,search_center_sec,suggested_offset_sec,lag_sec,raw_peak,note_energy,audio_energy,normalized_peak,reliable"
    }

    fn to_csv(&self) -> String {
        format!(
            "{},{},{},{:.6},{:.6},{:.6},{:.6},{:.9},{:.9},{:.9},{:.9},{}",
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
            self.reliable
        )
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    fs::create_dir_all(cli.root.join("charts"))?;

    ensure_samples(&cli).await?;
    let rows = analyze_samples(&cli).await?;
    write_csv(&cli.root.join("results.csv"), &rows)?;
    draw_plot(&cli.root.join("peak-energy-3d.png"), &rows)?;
    write_plotly_html(&cli.root.join("peak-energy-3d.html"), &rows)?;

    println!("rows: {}", rows.len());
    println!("csv: {}", cli.root.join("results.csv").display());
    println!("plot: {}", cli.root.join("peak-energy-3d.png").display());
    println!("html: {}", cli.root.join("peak-energy-3d.html").display());
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

    for id in cached_chart_ids(&charts_dir)? {
        if rows.len() >= cli.samples {
            break;
        }
        if done.contains(&id) {
            continue;
        }
        let dir = charts_dir.join(id.to_string());
        match analyze_chart(id, &dir, cli).await {
            Ok(row) => {
                println!("analyzed {id}: raw={:.3} norm={:.4}", row.raw_peak, row.normalized_peak);
                rows.push(row);
            }
            Err(err) => eprintln!("skip analysis {id}: {err:#}"),
        }
    }

    rows.sort_by_key(|row| row.chart_id);
    rows.truncate(cli.samples.min(rows.len()));
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
        if cols.len() != 12 {
            continue;
        }
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
            reliable: cols[11].parse()?,
        });
    }
    Ok(rows)
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
    let (chart_offset, note_times) = load_chart_timing(&mut *fs, &info).await?;
    if note_times.len() < 16 {
        bail!("too few notes")
    }

    let audio_path = dir.join(&info.music);
    let clip = prpr_avc::demux_audio(audio_path.to_str().context("invalid audio path")?)?.context("no audio stream found")?;
    let pcm: Vec<f32> = clip.frames().iter().map(|f| (f.0 + f.1) / 2.0).collect();
    let sample_rate = clip.sample_rate();
    let duration = pcm.len() as f64 / sample_rate as f64;

    let audio = SuperFlux::new(&pcm, sample_rate, 2048, 1024);
    let note = NoteGaussian::new(note_times.clone(), cli.blur_sigma);
    let search_center = chart_offset + info.offset as f64;
    let config = AlignConfig {
        search_range_sec: cli.range,
        sampling_interval_sec: cli.interval,
        search_center_sec: search_center,
    };

    let stats = estimate_energy_stats(&audio, &note, duration, &config);
    Ok(StudyRow {
        chart_id: id,
        chart_name: info.name,
        notes: note_times.len(),
        duration_sec: duration,
        search_center_sec: search_center,
        suggested_offset_sec: stats.offset,
        lag_sec: stats.offset - search_center,
        raw_peak: stats.raw_peak,
        note_energy: stats.note_energy,
        audio_energy: stats.audio_energy,
        normalized_peak: stats.normalized_peak,
        reliable: stats.normalized_peak > 0.05,
    })
}

async fn load_chart_timing(fs: &mut dyn FileSystem, info: &ChartInfo) -> Result<(f64, Vec<f64>)> {
    let bytes = fs.load_file(&info.chart).await?;
    let format = infer_chart_format(info, &bytes);
    if matches!(format, ChartFormat::Rpe) {
        return parse_rpe_timing(&String::from_utf8_lossy(&bytes));
    }
    let chart = load_chart(fs, info, &bytes, format).await?;
    Ok((chart.offset as f64, extract_note_times(&chart)))
}

async fn load_chart(fs: &mut dyn FileSystem, info: &ChartInfo, bytes: &[u8], format: ChartFormat) -> Result<Chart> {
    match format {
        ChartFormat::Rpe => parse_rpe(&String::from_utf8_lossy(&bytes), fs, Default::default(), info.use_rpe_170_speed.unwrap_or_default()).await,
        ChartFormat::Pgr => parse_phigros(&String::from_utf8_lossy(&bytes), Default::default()),
        ChartFormat::Pec => parse_pec(&String::from_utf8_lossy(&bytes), Default::default()),
        ChartFormat::Pbc => bail!("pbc charts are not supported by this study tool"),
    }
}

fn parse_rpe_timing(source: &str) -> Result<(f64, Vec<f64>)> {
    let rpe: RpeTimingChart = serde_json::from_str(source).context("failed to parse RPE timing")?;
    let mut bpm = BpmList::new(rpe.bpm_list.into_iter().map(|it| (it.start_time.beats(), it.bpm)).collect());
    let mut note_times: Vec<f64> = rpe
        .judge_line_list
        .into_iter()
        .flat_map(|line| line.notes.unwrap_or_default())
        .filter(|note| note.is_fake == 0)
        .map(|note| bpm.time(&note.start_time))
        .filter(|&time| time >= 0.0)
        .collect();
    note_times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    Ok((rpe.meta.offset as f64 / 1000.0, note_times))
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

fn extract_note_times(chart: &Chart) -> Vec<f64> {
    let mut times: Vec<f64> = chart
        .lines
        .iter()
        .flat_map(|line| line.notes.iter())
        .filter(|note| !note.fake)
        .map(|note| note.time)
        .filter(|&t| t >= 0.0)
        .collect();
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    times
}

struct EnergyStats {
    offset: f64,
    raw_peak: f64,
    note_energy: f64,
    audio_energy: f64,
    normalized_peak: f64,
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
    }
}

fn write_csv(path: &Path, rows: &[StudyRow]) -> Result<()> {
    let mut file = File::create(path)?;
    writeln!(file, "{}", StudyRow::header())?;
    for row in rows {
        writeln!(file, "{}", row.to_csv())?;
    }
    Ok(())
}

fn write_plotly_html(path: &Path, rows: &[StudyRow]) -> Result<()> {
    let x: Vec<f64> = rows.iter().map(|row| (row.note_energy.max(1e-12)).log10()).collect();
    let y: Vec<f64> = rows.iter().map(|row| (row.audio_energy.max(1e-12)).log10()).collect();
    let z: Vec<f64> = rows.iter().map(|row| (row.raw_peak.max(1e-12)).log10()).collect();
    let color: Vec<f64> = rows.iter().map(|row| row.normalized_peak).collect();
    let text: Vec<String> = rows
        .iter()
        .map(|row| {
            format!(
                "#{} {}<br>notes: {}<br>offset: {:.0}ms<br>raw: {:.3}<br>norm: {:.4}",
                row.chart_id,
                row.chart_name,
                row.notes,
                row.suggested_offset_sec * 1000.0,
                row.raw_peak,
                row.normalized_peak
            )
        })
        .collect();
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
        colorscale: 'Turbo',
        cmin: 0,
        cmax: 1,
        colorbar: {{ title: 'normalized peak' }},
        opacity: 0.82
      }}
    }};
    const layout = {{
      title: 'Auto-offset energy study',
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
    );
    fs::write(path, html)?;
    Ok(())
}

fn draw_plot(path: &Path, rows: &[StudyRow]) -> Result<()> {
    let root = BitMapBackend::new(path, (1200, 900)).into_drawing_area();
    root.fill(&RGBColor(250, 250, 248))?;

    let mut chart = ChartBuilder::on(&root)
        .caption("Auto-offset energy study: log raw peak vs. log note/audio energy", ("sans-serif", 28).into_font())
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
    let zs: Vec<f64> = rows.iter().map(|r| (r.raw_peak.max(1e-12)).log10()).collect();
    let (xmin, xmax) = min_max(&xs);
    let (ymin, ymax) = min_max(&ys);
    let (zmin, zmax) = min_max(&zs);

    chart.draw_series(rows.iter().zip(&xs).zip(ys.iter().zip(&zs)).map(|((row, &x), (&y, &z))| {
        let px = norm(x, xmin, xmax) - 0.5;
        let py = norm(y, ymin, ymax) - 0.5;
        let pz = norm(z, zmin, zmax) - 0.5;
        let sx = px + py * 0.38;
        let sy = pz - py * 0.30;
        Circle::new((sx, sy), 4, ShapeStyle::from(&heat(row.normalized_peak)).filled())
    }))?;

    draw_color_legend(&root)?;
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

fn heat(value: f64) -> RGBColor {
    let t = value.clamp(0.0, 1.0);
    let r = (40.0 + 210.0 * t) as u8;
    let g = (80.0 + 120.0 * (1.0 - (t - 0.5).abs() * 2.0).max(0.0)) as u8;
    let b = (220.0 - 180.0 * t) as u8;
    RGBColor(r, g, b)
}

fn draw_color_legend(root: &DrawingArea<BitMapBackend<'_>, plotters::coord::Shift>) -> Result<()> {
    let x0 = 1030;
    let y0 = 140;
    let h = 300;
    for i in 0..h {
        let t = 1.0 - i as f64 / (h - 1) as f64;
        root.draw(&Rectangle::new([(x0, y0 + i), (x0 + 24, y0 + i + 1)], heat(t).filled()))?;
    }
    root.draw(&Text::new("normalized", (x0 - 15, y0 - 28), ("sans-serif", 18).into_font()))?;
    root.draw(&Text::new("peak", (x0 + 1, y0 - 8), ("sans-serif", 18).into_font()))?;
    root.draw(&Text::new("1.0", (x0 + 34, y0 + 6), ("sans-serif", 16).into_font()))?;
    root.draw(&Text::new("0.0", (x0 + 34, y0 + h - 4), ("sans-serif", 16).into_font()))?;
    Ok(())
}
