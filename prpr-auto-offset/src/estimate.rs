use crate::{AlignConfig, AlignmentResult, Signal};

/// Reliability threshold for normalized cross-correlation.
///
/// If the normalized peak `r` exceeds this value, the detected offset is
/// considered reliable. The threshold is heuristic; 0.05 works well across
/// the tested chart corpus.
const RELIABILITY_THRESHOLD: f64 = 0.05;

/// Cross-correlation between two arrays, limited lag range.
///
/// Returns `(correlation_values, best_lag_index, peak_value)` where
/// `correlation[lag]` is the dot product of `a` with `b` shifted by
/// `lag - max_lag_bins`.
fn cross_correlation(a: &[f32], b: &[f32], max_lag_bins: usize) -> (Vec<f32>, usize, f32) {
    let n = a.len().min(b.len());
    if n == 0 {
        return (vec![], 0, 0.0);
    }

    let mut best_lag = max_lag_bins;
    let mut best_val = f32::NEG_INFINITY;
    let mut corr = Vec::with_capacity(2 * max_lag_bins + 1);

    for lag_offset in 0..=2 * max_lag_bins {
        let lag = lag_offset as isize - max_lag_bins as isize;
        let mut sum = 0.0f32;
        (0..n).for_each(|i| {
            let j = i as isize + lag;
            if j >= 0 && j < b.len() as isize {
                sum += a[i] * b[j as usize];
            }
        });
        corr.push(sum);
        if sum > best_val {
            best_val = sum;
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

/// Compute the normalized cross-correlation `r` at a specific lag.
///
/// `r = Σ a[i] · b[i+lag] / √(Σ a[i]² · Σ b[i+lag]²)` over the overlapping
/// region. By Cauchy-Schwarz, `r ∈ [0, 1]` for non-negative signals.
fn normalized_correlation(a: &[f32], b: &[f32], lag: isize, best_val: f32) -> f64 {
    let mut norm_a = 0.0f64;
    let mut norm_b = 0.0f64;

    (0..a.len().min(b.len())).for_each(|i| {
        let j = i as isize + lag;
        if j >= 0 && j < b.len() as isize {
            norm_a += (a[i] as f64).powi(2);
            norm_b += (b[j as usize] as f64).powi(2);
        }
    });

    let denom = (norm_a * norm_b).sqrt();
    if denom <= 0.0 {
        return 0.0;
    }

    (best_val as f64 / denom).clamp(0.0, 1.0)
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

    // Build shared sampling grid centered at search_center_sec
    let t_min = config.search_center_sec - config.search_range_sec;
    let t_max = config.search_center_sec + duration_sec + config.search_range_sec;
    let ts = build_ts_grid(t_min, t_max, config.sampling_interval_sec);

    // Sample both signals
    let audio_samples = audio.samples(&ts);
    let note_samples = note.samples(&ts);

    if audio_samples.is_empty() || note_samples.is_empty() {
        return AlignmentResult {
            offset: 0.0,
            correlation: 0.0,
            reliable: false,
            correlation_curve: Vec::new(),
        };
    }

    // Cross-correlation
    let max_lag_bins = (config.search_range_sec / config.sampling_interval_sec).ceil() as usize;
    let (corr, best_lag, best_val) = cross_correlation(&note_samples, &audio_samples, max_lag_bins);
    let offset = (best_lag as isize - max_lag_bins as isize) as f64 * config.sampling_interval_sec;

    // Build correlation curve
    let correlation_curve: Vec<(f64, f32)> = corr
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let lag = i as isize - max_lag_bins as isize;
            (lag as f64 * config.sampling_interval_sec, v)
        })
        .collect();

    // Normalized correlation at best lag
    let lag = best_lag as isize - max_lag_bins as isize;
    let correlation = normalized_correlation(&note_samples, &audio_samples, lag, best_val);

    AlignmentResult {
        offset,
        correlation,
        reliable: correlation > RELIABILITY_THRESHOLD,
        correlation_curve,
    }
}
