use crate::{AlignConfig, AlignmentResult, Signal};

/// Reliability threshold for normalized cross-correlation.
///
/// If the normalized peak `r` exceeds this value, the detected offset is
/// considered reliable. The threshold is heuristic; 0.05 works well across
/// the tested chart corpus.
const RELIABILITY_THRESHOLD: f64 = 0.05;

/// Normalized cross-correlation between two arrays, limited lag range.
///
/// Returns `(correlation_values, best_lag_index, peak_value)` where each
/// correlation value is the normalized dot product of `a` with `b` shifted by
/// `lag - max_lag_bins`.
fn normalized_cross_correlation(a: &[f32], b: &[f32], max_lag_bins: usize) -> (Vec<f32>, usize, f32) {
    let n = a.len().min(b.len());
    if n == 0 {
        return (vec![], 0, 0.0);
    }

    let norm_a = a.iter().map(|&v| (v as f64).powi(2)).sum::<f64>();
    let norm_b = b.iter().map(|&v| (v as f64).powi(2)).sum::<f64>();
    let denom = (norm_a * norm_b).sqrt();

    let mut best_lag = max_lag_bins;
    let mut best_val = f32::NEG_INFINITY;
    let mut corr = Vec::with_capacity(2 * max_lag_bins + 1);

    for lag_offset in 0..=2 * max_lag_bins {
        let lag = lag_offset as isize - max_lag_bins as isize;
        let mut dot = 0.0f64;

        (0..n).for_each(|i| {
            let j = i as isize + lag;
            if j >= 0 && j < b.len() as isize {
                let av = a[i] as f64;
                let bv = b[j as usize] as f64;
                dot += av * bv;
            }
        });

        let value = if denom > 0.0 { (dot / denom).clamp(0.0, 1.0) as f32 } else { 0.0 };
        corr.push(value);
        if value > best_val {
            best_val = value;
            best_lag = lag_offset;
        }
    }

    (corr, best_lag, best_val)
}

/// Build a uniform time grid from `t_min` to `t_max` (inclusive) with step `dt`.
fn build_ts_grid(t_min: f64, t_max: f64, dt: f64) -> Vec<f64> {
    let n = ((t_max - t_min) / dt).ceil() as usize + 1;
    (0..n).map(|i| t_min + i as f64 * dt).collect()
}

/// Estimate the timing offset between two signals.
///
/// Uses default [`AlignConfig`]. See [`estimate_with`] for custom config.
pub fn estimate<A: Signal, N: Signal>(audio: &A, note: &N, duration_sec: f64) -> AlignmentResult {
    estimate_with(audio, note, duration_sec, &AlignConfig::default())
}

/// Estimate the timing offset between two signals with custom config.
///
/// `audio` is a [`Signal`] produced from the audio track (e.g.
/// [`SpectralFlux`](crate::SpectralFlux)). `note` is a [`Signal`]
/// produced from the chart's note events (e.g.
/// [`NoteGaussian`](crate::NoteGaussian)).
pub fn estimate_with<A: Signal, N: Signal>(audio: &A, note: &N, duration_sec: f64, config: &AlignConfig) -> AlignmentResult {
    if duration_sec <= 0.0 {
        return AlignmentResult {
            offset: 0.0,
            correlation: 0.0,
            reliable: false,
            correlation_curve: Vec::new(),
        };
    }

    // Build absolute-time sampling grid for the audio signal.
    let t_min = config.search_center_sec - config.search_range_sec;
    let t_max = config.search_center_sec + duration_sec + config.search_range_sec;
    let ts = build_ts_grid(t_min, t_max, config.sampling_interval_sec);

    // Sample audio on the absolute-time grid.
    let audio_samples = audio.samples(&ts);

    // Shift the note signal into absolute time by sampling it at
    //    ts_note[i] = ts[i] - search_center_sec
    // so that a note event at chart time `note.time` appears at absolute
    // time `note.time + search_center_sec`. After this shift the two
    // signals share a single coordinate system and the cross-correlation lag
    // is a small residual rather than the full offset.
    let note_ts: Vec<f64> = ts.iter().map(|&t| t - config.search_center_sec).collect();
    let note_samples = note.samples(&note_ts);

    if audio_samples.is_empty() || note_samples.is_empty() {
        return AlignmentResult {
            offset: 0.0,
            correlation: 0.0,
            reliable: false,
            correlation_curve: Vec::new(),
        };
    }

    // Normalized cross-correlation: now the best lag is a small residual around zero.
    let max_lag_bins = (config.search_range_sec / config.sampling_interval_sec).ceil() as usize;
    let (corr, best_lag, best_val) = normalized_cross_correlation(&note_samples, &audio_samples, max_lag_bins);

    // Residual lag, then add search_center_sec to get absolute offset.
    let best_lag_sec = (best_lag as isize - max_lag_bins as isize) as f64 * config.sampling_interval_sec;
    let offset = config.search_center_sec + best_lag_sec;

    // Correlation curve: x = absolute offset (search_center + lag).
    let correlation_curve: Vec<(f64, f32)> = corr
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let lag = (i as isize - max_lag_bins as isize) as f64 * config.sampling_interval_sec;
            (config.search_center_sec + lag, v)
        })
        .collect();

    AlignmentResult {
        offset,
        correlation: best_val as f64,
        reliable: best_val as f64 > RELIABILITY_THRESHOLD,
        correlation_curve,
    }
}
