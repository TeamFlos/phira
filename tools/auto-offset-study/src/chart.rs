use std::path::Path;

use anyhow::{bail, Context, Result};
use prpr::{
    core::{BpmList, Chart, NoteKind, Triple},
    fs::{fs_from_file, load_info, FileSystem},
    info::{ChartFormat, ChartInfo},
    parse::{parse_pec, parse_phigros, parse_rpe},
};
use prpr_auto_offset::{estimate_with, AlignConfig, AutoOffsetNoteKind, NoteEvent, PreprocessedNoteGaussian, SuperFlux};
use serde::Deserialize;

use crate::{
    config::Cli,
    model::{NoteStats, StudyRow},
};

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

pub async fn analyze_chart(id: i32, dir: &Path, cli: &Cli) -> Result<StudyRow> {
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
