use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use prpr::{
    core::{Chart, NoteKind},
    fs::{fs_from_file, load_info},
    parse::{parse_pec, parse_phigros, parse_rpe},
};
use prpr_auto_offset::{
    AlignConfig, AlignmentResult, AutoOffsetNoteKind, EnergyDiff, NoteEvent, NoteGaussian, PreprocessedNoteGaussian, SpectralFlux, SuperFlux,
};
use std::io::Write;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, ValueEnum)]
enum AudioMethod {
    /// Recommended. Lowest measured timing bias on game hit sounds.
    Superflux,
    /// Diagnostic only. Plain spectral flux tends to report late peaks.
    Spectral,
    /// Diagnostic only. Simple RMS-energy onset, less robust for music.
    Energy,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum NoteMethod {
    /// Recommended. Uses note kinds, caps dense chords, and downweights drag runs.
    PreprocessedGaussian,
    /// Diagnostic only. Ignores note kinds and uses every note equally.
    Gaussian,
}

#[derive(Parser)]
#[command(name = "prpr-auto-offset")]
#[command(about = "Automatic chart offset detection for Phira")]
struct Cli {
    /// Path to a Phira chart file (zip archive)
    chart: PathBuf,

    /// Search range in seconds (centered at chart's author offset, or at 0 with --wide)
    #[arg(short, long, default_value = "0.30")]
    range: f64,

    /// Wide-range search: ignore author offset, search full ±range from 0
    #[arg(short = 'w', long)]
    wide: bool,

    /// Audio novelty method
    #[arg(long, value_enum, default_value_t = AudioMethod::Superflux)]
    audio_method: AudioMethod,

    /// Note signal method
    #[arg(long, value_enum, default_value_t = NoteMethod::PreprocessedGaussian)]
    note_method: NoteMethod,

    /// Sampling interval for the cross-correlation grid, in seconds
    #[arg(short, long, default_value = "0.005")]
    interval: f64,

    /// Gaussian blur sigma for the note signal, in seconds
    #[arg(long, default_value = "0.02")]
    blur_sigma: f64,

    /// Verbose output
    #[arg(short, long)]
    verbose: bool,
}

fn auto_offset_note_kind(kind: &NoteKind) -> AutoOffsetNoteKind {
    match kind {
        NoteKind::Click => AutoOffsetNoteKind::Tap,
        NoteKind::Hold { .. } => AutoOffsetNoteKind::Hold,
        NoteKind::Flick => AutoOffsetNoteKind::Flick,
        NoteKind::Drag => AutoOffsetNoteKind::Drag,
    }
}

fn extract_note_events(chart: &Chart) -> Vec<NoteEvent> {
    let mut notes: Vec<NoteEvent> = chart
        .lines
        .iter()
        .flat_map(|line| {
            line.notes
                .iter()
                .filter(|note| !note.fake && note.time >= 0.0)
                .map(|note| NoteEvent::new(note.time, auto_offset_note_kind(&note.kind)))
        })
        .collect();
    notes.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());
    notes
}

fn print_result(result: &AlignmentResult, verbose: bool) {
    if verbose {
        println!();
    }
    println!("═══════════════════════════════════════");
    println!("  Suggested offset: {:.3}s ({:.0}ms)", result.offset, result.offset * 1000.0);
    println!("  Match score:      {:.4}", result.correlation);
    println!("  Reliable:          {}", if result.reliable { "yes" } else { "no" });
    println!("═══════════════════════════════════════");
}
#[allow(clippy::too_many_arguments)]
async fn run(
    chart_path: &PathBuf,
    search_range: f64,
    wide: bool,
    audio_method: AudioMethod,
    note_method: NoteMethod,
    sampling_interval: f64,
    blur_sigma: f64,
    verbose: bool,
) -> Result<()> {
    // 1. Open zip as filesystem
    let mut fs = fs_from_file(chart_path).with_context(|| format!("failed to open {:?}", chart_path))?;

    // 2. Load chart info
    let info = load_info(&mut *fs).await.context("failed to load chart info")?;

    // 3. Load and parse chart
    let chart_bytes = fs
        .load_file(&info.chart)
        .await
        .with_context(|| format!("failed to load chart file: {}", info.chart))?;

    let extra = if let Ok(data) = fs.load_file("extra.json").await {
        let s = String::from_utf8(data).context("extra.json is not valid UTF-8")?;
        prpr::parse::parse_extra(&s, &mut *fs).await.context("failed to parse extra")?
    } else {
        Default::default()
    };

    let format = info.format.as_ref().map(|f| match f {
        prpr::info::ChartFormat::Rpe => "rpe",
        prpr::info::ChartFormat::Pec => "pec",
        prpr::info::ChartFormat::Pgr => "pgr",
        prpr::info::ChartFormat::Pbc => "pbc",
    });

    let source = String::from_utf8_lossy(&chart_bytes);
    let chart = match format {
        Some("rpe") => parse_rpe(&source, &mut *fs, extra, info.use_rpe_170_speed.unwrap_or_default())
            .await
            .context("failed to parse RPE chart")?,
        Some("pec") => parse_pec(&source, extra).context("failed to parse PEC chart")?,
        Some("pgr") | None => parse_phigros(&source, extra).context("failed to parse PGR chart")?,
        Some(other) => anyhow::bail!("unsupported chart format: {other}"),
    };

    // 4. Extract note events
    let note_events = extract_note_events(&chart);
    if verbose {
        println!("Chart: {} — {} by {}", info.name, info.level, info.charter);
        println!("  Notes: {}, Chart offset: {:.0}ms", note_events.len(), info.offset * 1000.0);
    }

    // 5. Extract and decode audio
    let audio_data = fs
        .load_file(&info.music)
        .await
        .with_context(|| format!("failed to load audio: {}", info.music))?;

    let ext = info.music.rsplit('.').next().unwrap_or("ogg");
    let mut tmp = tempfile::Builder::new()
        .suffix(&format!(".{ext}"))
        .tempfile()
        .context("failed to create temp file")?;
    tmp.write_all(&audio_data).context("failed to write audio to temp file")?;

    let tmp_path = tmp.into_temp_path();
    let tmp_str = tmp_path.to_str().context("temp path is not valid UTF-8")?;

    let clip = prpr_avc::demux_audio(tmp_str)
        .context("failed to decode audio")?
        .context("no audio stream found")?;

    let pcm: Vec<f32> = clip.frames().iter().map(|f| (f.0 + f.1) / 2.0).collect();
    let sample_rate = clip.sample_rate();
    let duration = pcm.len() as f64 / sample_rate as f64;
    if verbose {
        println!("  Audio: {:.1}s, {}Hz, mono", duration, sample_rate);
    }

    // 6. Configure
    let author_offset = info.offset as f64;
    let config = AlignConfig {
        search_range_sec: search_range,
        sampling_interval_sec: sampling_interval,
        search_center_sec: if wide { 0.0 } else { author_offset },
    };

    if verbose {
        if wide {
            println!("  Search: +/-{:.0}ms (wide, centered at 0)", search_range * 1000.0);
        } else {
            println!("  Search: +/-{:.0}ms (centered at author offset {:.0}ms)", search_range * 1000.0, author_offset * 1000.0);
        }
    }

    // 7. Select methods and run
    if verbose {
        println!("  Audio method: {}", audio_method_description(audio_method));
        println!("  Note method: {} (sigma={}ms)", note_method_description(note_method), blur_sigma * 1000.0);
    }

    let result = match audio_method {
        AudioMethod::Superflux => {
            let audio = SuperFlux::new(&pcm, sample_rate, 2048, 1024);
            estimate_with_note_method(&audio, &note_events, note_method, blur_sigma, duration, &config)
        }
        AudioMethod::Spectral => {
            let audio = SpectralFlux::new(&pcm, sample_rate, 1024, 512);
            estimate_with_note_method(&audio, &note_events, note_method, blur_sigma, duration, &config)
        }
        AudioMethod::Energy => {
            let audio = EnergyDiff::new(&pcm, sample_rate, 10.0, 5.0);
            estimate_with_note_method(&audio, &note_events, note_method, blur_sigma, duration, &config)
        }
    };
    print_result(&result, verbose);
    Ok(())
}

fn audio_method_description(method: AudioMethod) -> &'static str {
    match method {
        AudioMethod::Superflux => "superflux (recommended; lowest measured hit-sound timing bias)",
        AudioMethod::Spectral => "spectral flux (diagnostic; tends to report late peaks)",
        AudioMethod::Energy => "energy diff (diagnostic; simple and less robust for music)",
    }
}

fn note_method_description(method: NoteMethod) -> &'static str {
    match method {
        NoteMethod::PreprocessedGaussian => "preprocessed gaussian (recommended; uses note kinds)",
        NoteMethod::Gaussian => "gaussian (diagnostic; ignores note kinds)",
    }
}

fn estimate_with_note_method<A: prpr_auto_offset::Signal>(
    audio: &A,
    note_events: &[NoteEvent],
    note_method: NoteMethod,
    blur_sigma: f64,
    duration: f64,
    config: &AlignConfig,
) -> AlignmentResult {
    match note_method {
        NoteMethod::PreprocessedGaussian => {
            let note = PreprocessedNoteGaussian::new(note_events.to_vec(), blur_sigma);
            prpr_auto_offset::estimate_with(audio, &note, duration, config)
        }
        NoteMethod::Gaussian => {
            let note = NoteGaussian::new(note_events.iter().map(|note| note.time).collect(), blur_sigma);
            prpr_auto_offset::estimate_with(audio, &note, duration, config)
        }
    }
}
#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    run(&cli.chart, cli.range, cli.wide, cli.audio_method, cli.note_method, cli.interval, cli.blur_sigma, cli.verbose).await
}
