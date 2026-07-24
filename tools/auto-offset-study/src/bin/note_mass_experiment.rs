use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
    println,
};

use anyhow::{bail, Context, Result};
use clap::Parser;
use prpr::info::ChartInfo;
use prpr_auto_offset::{AutoOffsetNoteKind, NoteEvent, NotePreprocessor, PreprocessedNote};
use prpr_auto_offset_study::{csv::read_existing_csv, model::StudyRow};
use serde::Deserialize;

#[derive(Parser)]
#[command(about = "Side-car experiment for note-mass normalization; does not modify results.csv")]
struct Args {
    #[arg(long, default_value = "data/auto-offset-study")]
    root: PathBuf,
    #[arg(long, default_value_t = 0.30)]
    range: f64,
    #[arg(long, default_value_t = 0.005)]
    interval: f64,
    #[arg(long, default_value_t = 0.02)]
    blur_sigma: f64,
    #[arg(long, default_value = "note-mass-experiment.csv")]
    output: String,
}

#[derive(Clone)]
struct Row {
    base: StudyRow,
    note_l1: f64,
    note_l2_sq: f64,
    effective_count: f64,
    mass_score: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PgrChart {
    judge_line_list: Vec<PgrLine>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PgrLine {
    bpm: f64,
    notes_above: Vec<PgrNote>,
    notes_below: Vec<PgrNote>,
}

#[derive(Deserialize)]
struct PgrNote {
    #[serde(rename = "type")]
    kind: u8,
    time: f64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let rows = read_existing_csv(&args.root.join("results.csv"))?;
    if rows.is_empty() {
        bail!("no rows in {}", args.root.join("results.csv").display());
    } else {
        println!("read {} rows in {}", rows.len(), args.root.join("results.csv").display());
    }

    let mut out_rows = Vec::new();
    for row in rows {
        let chart_dir = args.root.join("charts").join(row.chart_id.to_string());
        match load_pgr_events(&chart_dir).await {
            Ok(events) => {
                let (note_l1, note_l2_sq, effective_count) = note_mass(&events, row.duration_sec, &args);
                if note_l1 > 0.0 && note_l2_sq > 0.0 && row.audio_energy > 0.0 && row.raw_peak > 0.0 {
                    let mass_score = row.raw_peak / (note_l1 * row.audio_energy.sqrt()).max(1e-12);
                    out_rows.push(Row {
                        base: row,
                        note_l1,
                        note_l2_sq,
                        effective_count,
                        mass_score,
                    });
                    println!(
                        "{}: note_l1={note_l1:.9}, note_l2_sq={note_l2_sq:.9}, effective_count={effective_count:.9}, mass_score={mass_score:.9}",
                        chart_dir.display()
                    );
                } else {
                    eprintln!(
                        "skip {}: note_l1={note_l1:.9}, note_l2_sq={note_l2_sq:.9}, audio_energy={:.9}, raw_peak={:.9}",
                        row.chart_id, row.audio_energy, row.raw_peak
                    );
                }
            }
            Err(err) => eprintln!("skip {}: {err:#}", row.chart_id),
        }
    }

    if out_rows.len() < 3 {
        bail!("need at least 3 valid rows");
    }

    write_csv(&args.root.join(&args.output), &out_rows)?;
    print_fits(&out_rows)?;
    println!("csv: {}", args.root.join(&args.output).display());
    Ok(())
}

async fn load_pgr_events(dir: &Path) -> Result<Vec<NoteEvent>> {
    let mut fs = prpr::fs::fs_from_file(dir)?;
    let info = prpr::fs::load_info(&mut *fs).await?;
    let bytes = fs.load_file(&info.chart).await?;
    parse_pgr_events(&info, &bytes)
}

fn parse_pgr_events(_info: &ChartInfo, bytes: &[u8]) -> Result<Vec<NoteEvent>> {
    let chart: PgrChart = serde_json::from_slice(bytes).context("failed to parse PGR chart")?;
    let mut events = Vec::new();
    for line in chart.judge_line_list {
        let scale = 60.0 / line.bpm / 32.0;
        for note in line.notes_above.into_iter().chain(line.notes_below) {
            let kind = match note.kind {
                1 => AutoOffsetNoteKind::Tap,
                2 => AutoOffsetNoteKind::Drag,
                3 => AutoOffsetNoteKind::Hold,
                4 => AutoOffsetNoteKind::Flick,
                _ => continue,
            };
            let time = note.time * scale;
            if time.is_finite() && time >= 0.0 {
                events.push(NoteEvent::new(time, kind));
            }
        }
    }
    events.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());
    Ok(events)
}

fn note_mass(events: &[NoteEvent], duration_sec: f64, args: &Args) -> (f64, f64, f64) {
    let notes = NotePreprocessor::new().preprocess(events.to_vec());
    let (integral_l1, integral_l2_sq) = gaussian_mass_integrals(&notes, args.blur_sigma, -args.range, duration_sec + args.range);
    let l1 = integral_l1 / args.interval;
    let l2_sq = integral_l2_sq / args.interval;
    let effective_count = if l2_sq > 0.0 { l1 * l1 / l2_sq } else { 0.0 };
    (l1, l2_sq, effective_count)
}

fn gaussian_mass_integrals(notes: &[PreprocessedNote], sigma: f64, start: f64, end: f64) -> (f64, f64) {
    let l1 = notes
        .iter()
        .map(|note| note.weight as f64 * gaussian_l1_between(note.time, sigma, start, end))
        .sum::<f64>();
    let mut l2_sq = 0.0;
    for (index, a) in notes.iter().enumerate() {
        for b in &notes[index..] {
            if b.time - a.time > sigma * 10.0 {
                break;
            }
            let value = a.weight as f64 * b.weight as f64 * gaussian_product_between(a.time, b.time, sigma, start, end);
            l2_sq += if (a.time - b.time).abs() < f64::EPSILON { value } else { 2.0 * value };
        }
    }
    (l1, l2_sq)
}

fn gaussian_l1_between(center: f64, sigma: f64, start: f64, end: f64) -> f64 {
    let scale = sigma * (std::f64::consts::PI / 2.0).sqrt();
    scale * (erf((end - center) / (sigma * 2.0_f64.sqrt())) - erf((start - center) / (sigma * 2.0_f64.sqrt())))
}

fn gaussian_product_between(a: f64, b: f64, sigma: f64, start: f64, end: f64) -> f64 {
    let midpoint = (a + b) * 0.5;
    let distance = a - b;
    let overlap = (-distance * distance / (4.0 * sigma * sigma)).exp();
    let scale = sigma * std::f64::consts::PI.sqrt() * 0.5;
    overlap * scale * (erf((end - midpoint) / sigma) - erf((start - midpoint) / sigma))
}

fn erf(x: f64) -> f64 {
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs();
    let t = 1.0 / (1.0 + 0.3275911 * x);
    let y = 1.0 - (((((1.061405429 * t - 1.453152027) * t) + 1.421413741) * t - 0.284496736) * t + 0.254829592) * t * (-x * x).exp();
    sign * y
}

fn write_csv(path: &Path, rows: &[Row]) -> Result<()> {
    let mut file = File::create(path)?;
    writeln!(file, "chart_id,chart_name,notes,raw_peak,audio_energy,note_energy,note_l1,note_l2_sq,note_l2_ratio,effective_count,cs_score,mass_score,lag_ms,group")?;
    for row in rows {
        writeln!(
            file,
            "{},{},{},{:.9},{:.9},{:.9},{:.9},{:.9},{:.9},{:.9},{:.9},{:.9},{:.3},{}",
            row.base.chart_id,
            csv_escape(&row.base.chart_name),
            row.base.notes,
            row.base.raw_peak,
            row.base.audio_energy,
            row.base.note_energy,
            row.note_l1,
            row.note_l2_sq,
            row.note_l2_sq / row.base.note_energy.max(1e-12),
            row.effective_count,
            row.base.normalized_peak,
            row.mass_score,
            row.base.lag_sec * 1000.0,
            row.base.listing_label(),
        )?;
    }
    Ok(())
}

fn print_fits(rows: &[Row]) -> Result<()> {
    fit("baseline: log raw ~ log note_l2_sq + log audio_energy", rows, |r| {
        Some((vec![log10(r.base.note_energy)?, log10(r.base.audio_energy)?], log10(r.base.raw_peak)?))
    })?;
    fit("mass:     log raw ~ log note_l1 + log audio_energy", rows, |r| {
        Some((vec![log10(r.note_l1)?, log10(r.base.audio_energy)?], log10(r.base.raw_peak)?))
    })?;
    fit("cs leak:  log cs_score ~ log effective_count", rows, |r| Some((vec![log10(r.effective_count)?], log10(r.base.normalized_peak)?)))?;
    fit("count:    log effective_count ~ log note_l2_sq", rows, |r| Some((vec![log10(r.base.note_energy)?], log10(r.effective_count)?)))?;
    Ok(())
}

fn fit<F>(name: &str, rows: &[Row], mut build: F) -> Result<()>
where
    F: FnMut(&Row) -> Option<(Vec<f64>, f64)>,
{
    let points: Vec<_> = rows.iter().filter_map(|r| build(r)).collect();
    let p = points[0].0.len() + 1;
    let mut xtx = vec![vec![0.0; p]; p];
    let mut xty = vec![0.0; p];
    let mut ys = Vec::new();
    for (xs, y) in &points {
        let mut v = vec![1.0];
        v.extend(xs);
        for i in 0..p {
            xty[i] += v[i] * y;
            for j in 0..p {
                xtx[i][j] += v[i] * v[j];
            }
        }
        ys.push(*y);
    }
    let coef = solve(xtx, xty).context("singular regression")?;
    let mean = ys.iter().sum::<f64>() / ys.len() as f64;
    let mut ss_res = 0.0;
    let mut ss_tot = 0.0;
    for (xs, y) in points {
        let pred = coef[0] + xs.iter().zip(coef.iter().skip(1)).map(|(x, c)| x * c).sum::<f64>();
        ss_res += (y - pred).powi(2);
        ss_tot += (y - mean).powi(2);
    }
    let r2 = if ss_tot > 0.0 { 1.0 - ss_res / ss_tot } else { 1.0 };
    println!("{name}");
    println!("  coef={coef:?}, r2={r2:.4}, rmse={:.4}, n={}", (ss_res / ys.len() as f64).sqrt(), ys.len());
    Ok(())
}

fn solve(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let n = b.len();
    for col in 0..n {
        let pivot = (col..n).max_by(|&x, &y| a[x][col].abs().partial_cmp(&a[y][col].abs()).unwrap())?;
        if a[pivot][col].abs() < 1e-12 {
            return None;
        }
        a.swap(col, pivot);
        b.swap(col, pivot);
        let div = a[col][col];
        for v in a[col].iter_mut().skip(col) {
            *v /= div;
        }
        b[col] /= div;
        for row in 0..n {
            if row == col {
                continue;
            }
            let factor = a[row][col];
            let pivot_row = a[col].clone();
            for (v, p) in a[row].iter_mut().zip(pivot_row).skip(col) {
                *v -= factor * p;
            }
            b[row] -= factor * b[col];
        }
    }
    Some(b)
}

fn log10(value: f64) -> Option<f64> {
    (value > 0.0 && value.is_finite()).then(|| value.log10())
}

fn csv_escape(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}
