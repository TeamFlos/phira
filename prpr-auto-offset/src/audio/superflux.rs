use crate::Signal;

/// SuperFlux onset detection signal.
///
/// Computes a percussion-onset novelty curve using the SuperFlux algorithm
/// from:
/// "Maximum Filter Vibrato Suppression for Onset Detection"
/// Sebastian Böck and Gerhard Widmer, DAFx-13, Maynooth, Ireland, September 2013.
///
/// Paper: https://www.dafx.de/paper-archive/2013/papers/09.dafx2013_submission_12.pdf
///
/// Reference Python implementation: https://github.com/CPJKU/SuperFlux/blob/master/SuperFlux.py
///
/// Processing steps:
///   1. High-pass filter (50 Hz) to remove sub-bass rumble
///   2. Log-scale triangular filterbank (24 bands/octave, 30 Hz – 17 kHz)
///   3. Magnitude spectrogram → filterbank → log10 scaling
///   4. Per-band spectral whitening (subtract local running mean)
///   5. Frequency-direction maximum filter (vibrato suppression) + temporal difference
///   6. Adaptive threshold via running median
///
/// The result is a dense time series with one onset-strength value per STFT
/// frame, suitable for cross-correlation with note event signals.
pub struct SuperFlux {
    /// Native onset-strength samples at the STFT hop rate.
    native: Vec<f32>,
    /// Time step between native samples, in seconds.
    native_dt: f64,
}

impl SuperFlux {
    /// Build the SuperFlux onset signal from raw mono PCM audio.
    ///
    /// # Arguments
    /// * `pcm` - Mono f32 audio samples.
    /// * `sample_rate` - Sample rate in Hz.
    /// * `window_size` - STFT window size in samples (default: 2048).
    /// * `hop_size` - STFT hop size in samples (default: 1024).
    pub fn new(pcm: &[f32], sample_rate: u32, window_size: usize, hop_size: usize) -> Self {
        assert!(window_size.is_power_of_two());
        let native_dt = hop_size as f64 / sample_rate as f64;

        // 1. Clone and high-pass filter
        let mut samples = pcm.to_vec();
        highpass_50hz(&mut samples, sample_rate);

        // 2. Log-scale filterbank (24 bands/octave, 30Hz–17kHz, matching paper)
        let filterbank = Filterbank::new(sample_rate, window_size, 24, 30.0, 17000.0, false);

        // 3. Spectrogram: |STFT| → filterbank → log10 (matching paper)
        let (mut spec_frames, frame_rate) = compute_spectrogram(&samples, sample_rate, window_size, hop_size, &filterbank, 1.0, 1.0);

        // 4. Spectral whitening (1-second window)
        whiten_spectrogram(&mut spec_frames, (frame_rate * 1.0) as usize);

        // 5. SuperFlux: frequency-direction max filter + temporal diff
        //    diff_frames=3, max_bins=3 (matching paper defaults)
        let onset = compute_superflux(&spec_frames, 3, 3);

        // 6. Adaptive threshold
        let onset = adaptive_threshold(&onset, frame_rate * 2.0, 0.5);

        // Use the declared native_dt (frame_rate may differ slightly due to rounding)
        let _ = frame_rate;
        Self { native: onset, native_dt }
    }

    /// Access the native onset-strength samples (after adaptive threshold).
    pub fn onset_samples(&self) -> &[f32] {
        &self.native
    }

    /// Time step between native onset samples, in seconds.
    pub fn onset_dt(&self) -> f64 {
        self.native_dt
    }
}

impl Signal for SuperFlux {
    fn samples(&self, ts: &[f64]) -> Vec<f32> {
        if ts.is_empty() {
            return vec![];
        }
        ts.iter().map(|&t| interpolate(&self.native, self.native_dt, t)).collect()
    }
}

// ─── High-pass filter (50 Hz) ──────────────────────────────────────────

fn highpass_50hz(samples: &mut [f32], sample_rate: u32) {
    // 1st-order Butterworth: y[n] = alpha*y[n-1] + alpha*(x[n] - x[n-1])
    // Remove DC offset first, then initialize state to avoid transient
    let dc = samples.iter().take((sample_rate as usize / 10).min(samples.len())).sum::<f32>() / (sample_rate as f32 / 10.0).min(samples.len() as f32);
    for s in &mut *samples {
        *s -= dc;
    }

    let cutoff = 50.0;
    let rc = 1.0 / (2.0 * std::f32::consts::PI * cutoff);
    let dt = 1.0 / sample_rate as f32;
    let alpha = rc / (rc + dt);
    // Initial state: assume steady state (no change)
    let mut x_prev = samples[0];
    let mut y_prev = 0.0; // HP filter: output is 0 at DC
    samples[0] = y_prev;
    for s in &mut samples[1..] {
        let x = *s;
        let y = alpha * y_prev + alpha * (x - x_prev);
        *s = y;
        x_prev = x;
        y_prev = y;
    }
}

// ─── Log-scale frequency generation ────────────────────────────────────

/// Generate frequencies on a logarithmic scale, matching Python's
/// `Filter.frequencies()` with A0 = 440 Hz as the reference.
fn log_frequencies(bands_per_octave: usize, fmin: f32, fmax: f32) -> Vec<f32> {
    let factor = 2.0f32.powf(1.0 / bands_per_octave as f32);
    let a = 440.0f32;

    let mut frequencies = vec![a];

    // Go upwards from A0
    let mut freq = a;
    while freq <= fmax {
        freq *= factor;
        frequencies.push(freq);
    }

    // Go downwards from A0
    freq = a;
    while freq >= fmin {
        freq /= factor;
        frequencies.push(freq);
    }

    frequencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
    frequencies
}

// ─── Triangular filterbank (log-scale, paper-compatible) ────────────────

/// Log-spaced triangular filterbank matching the Python reference `Filter` class.
///
/// Uses logarithmic frequency spacing (bands per octave) with A0 = 440 Hz as
/// the reference pitch, and maps triangular filters to FFT bins.
pub struct Filterbank {
    /// Triangular filter weights: `[fft_bin][filter_band]`
    pub weights: Vec<Vec<f32>>,
    pub n_bands: usize,
}

impl Filterbank {
    /// Create a log-spaced triangular filterbank.
    ///
    /// # Arguments
    /// * `sample_rate` - Audio sample rate in Hz.
    /// * `window_size` - STFT window size in samples.
    /// * `bands_per_octave` - Number of filter bands per octave (default: 24).
    /// * `fmin` - Minimum frequency in Hz (default: 30).
    /// * `fmax` - Maximum frequency in Hz (default: 17000, capped at Nyquist).
    /// * `equal` - If true, normalize each triangular filter to have area 1.
    pub fn new(sample_rate: u32, window_size: usize, bands_per_octave: usize, fmin: f32, fmax: f32, equal: bool) -> Self {
        let n_fft_bins = window_size / 2;
        let fmax = fmax.min(sample_rate as f32 / 2.0);

        // Generate log-spaced frequencies and map to FFT bins
        let frequencies = log_frequencies(bands_per_octave, fmin, fmax);
        let factor = (sample_rate as f32 / 2.0) / n_fft_bins as f32;
        let mut bins: Vec<usize> = frequencies.iter().map(|&f| (f / factor).round() as usize).collect();
        bins.sort();
        bins.dedup();
        bins.retain(|&b| b < n_fft_bins);

        let n_bands = bins.len().saturating_sub(2);
        assert!(n_bands >= 3, "cannot create filterbank with less than 3 frequencies");

        let mut weights = vec![vec![0.0f32; n_bands]; n_fft_bins];

        for band in 0..n_bands {
            let start = bins[band];
            let mid = bins[band + 1];
            let stop = bins[band + 2];

            let height = if equal { 2.0 / (stop - start) as f32 } else { 1.0 };

            // Rising edge: start..mid
            let n_rise = mid - start;
            for (offset, w) in weights[start..mid].iter_mut().enumerate() {
                w[band] = height * offset as f32 / n_rise as f32;
            }
            // Falling edge: mid..stop
            let n_fall = stop - mid;
            for (offset, w) in weights[mid..stop].iter_mut().enumerate() {
                w[band] = height * (n_fall - offset) as f32 / n_fall as f32;
            }
        }

        Filterbank { weights, n_bands }
    }

    /// Apply filterbank to a **magnitude** spectrum, returning per-band
    /// energy (linear magnitude, not dB).
    pub fn apply(&self, magnitude_spectrum: &[f32]) -> Vec<f32> {
        let mut bands = vec![0.0f32; self.n_bands];
        for (b, w) in self.weights.iter().enumerate() {
            // w is [band] at this FFT bin — sum up contributions per band
            // weights layout: [fft_bin][band]
            for band in 0..self.n_bands {
                bands[band] += magnitude_spectrum[b] * w[band];
            }
        }
        bands
    }
}

// ─── Spectrogram computation ────────────────────────────────────────────

/// Compute a log-magnitude spectrogram through a filterbank.
///
/// Matches the Python reference `Spectrogram` class:
///   `|STFT| → filterbank → log10(mul · X + add)`
///
/// Defaults: `mul = 1.0`, `add = 1.0` (log scaling on, matching Python defaults).
pub fn compute_spectrogram(
    samples: &[f32],
    sample_rate: u32,
    window_size: usize,
    hop_size: usize,
    filterbank: &Filterbank,
    mul: f32,
    add: f32,
) -> (Vec<Vec<f32>>, f32) {
    use rayon::prelude::*;
    use realfft::RealFftPlanner;
    use std::sync::Arc;

    let num_frames = if samples.len() < window_size {
        0
    } else {
        (samples.len() - window_size) / hop_size + 1
    };

    // Hann window
    let n2 = (window_size - 1) as f32;
    let window: Vec<f32> = (0..window_size)
        .map(|n| 0.5 * (1.0 - (2.0 * std::f32::consts::PI * n as f32 / n2).cos()))
        .collect();

    let mut planner = RealFftPlanner::<f32>::new();
    let r2c = Arc::new(planner.plan_fft_forward(window_size));

    let spec_frames: Vec<Vec<f32>> = (0..num_frames)
        .into_par_iter()
        .map(|frame_idx| {
            let start = frame_idx * hop_size;
            let mut windowed: Vec<f32> = samples[start..start + window_size].iter().zip(&window).map(|(&s, &w)| s * w).collect();

            let mut spectrum = r2c.make_output_vec();
            r2c.process(&mut windowed, &mut spectrum).unwrap();

            // Magnitude spectrum (not power)
            let magnitude: Vec<f32> = spectrum.iter().map(|c| c.norm()).collect();

            // Apply filterbank → linear per-band energy
            let mut bands = filterbank.apply(&magnitude);

            // Log scaling: log10(mul * X + add), matching Python defaults
            for v in &mut bands {
                *v = (mul * *v + add).log10();
            }
            bands
        })
        .collect();

    let frame_rate = sample_rate as f32 / hop_size as f32;
    (spec_frames, frame_rate)
}

// ─── Spectral whitening ─────────────────────────────────────────────────

/// For each filter band, subtract a local running mean (half-width = window_frames/2).
/// Clamps negative values to -120 dB floor.
fn whiten_spectrogram(frames: &mut [Vec<f32>], window_frames: usize) {
    let half = window_frames / 2;
    let n_frames = frames.len();
    if n_frames == 0 {
        return;
    }
    let n_bands = frames[0].len();

    for band in 0..n_bands {
        // Compute local means
        let mut smoothed = vec![0.0f32; n_frames];
        for (t, s) in smoothed.iter_mut().enumerate() {
            let lo = t.saturating_sub(half);
            let hi = (t + half).min(n_frames - 1);
            let count = (hi - lo + 1) as f32;
            let sum: f32 = frames[lo..=hi].iter().map(|f| f[band]).sum();
            *s = sum / count;
        }
        // Subtract local mean from each frame
        for t in 0..n_frames {
            frames[t][band] -= smoothed[t];
            // Clamp negative values to a small floor (onset is about INCREASE in energy)
            frames[t][band] = frames[t][band].max(-120.0);
        }
    }
}

// ─── SuperFlux onset detection ──────────────────────────────────────────

/// Core SuperFlux algorithm.
///
/// Implements the method described in:
/// "Maximum Filter Vibrato Suppression for Onset Detection"
/// Sebastian Böck and Gerhard Widmer, DAFx-13, Maynooth, Ireland, September 2013.
///
/// Steps:
///   1. Apply a maximum filter of width `max_bins` in the **frequency** direction
///      on the spectrogram to suppress vibrato (the key contribution).
///   2. For each frame `t` and band `b`:
///      `diff(t,b) = max(0, X[t][b] - max_filtered(X)[t-diff_frames][b])`
///   3. Sum across all bands: `onset(t) = Σ_b diff(t,b)`
///
/// Robust-normalized by the 99th percentile (skipping the first ~1 s to avoid
/// HP filter transient).
fn compute_superflux(spec_frames: &[Vec<f32>], diff_frames: usize, max_bins: usize) -> Vec<f32> {
    let n_frames = spec_frames.len();
    if n_frames == 0 {
        return vec![];
    }
    let n_bands = spec_frames[0].len();

    let mut onset = vec![0.0f32; n_frames];

    if n_frames <= diff_frames {
        return onset;
    }

    // Step 1: Maximum filter in frequency direction (vibrato suppression).
    // For each bin [t][b], replace with max over [b - half, b + half].
    let half = max_bins / 2;
    let max_spec: Vec<Vec<f32>> = spec_frames
        .iter()
        .map(|frame| {
            (0..n_bands)
                .map(|b| {
                    let lo = b.saturating_sub(half);
                    let hi = (b + half).min(n_bands - 1);
                    frame[lo..=hi].iter().cloned().fold(0.0f32, f32::max)
                })
                .collect()
        })
        .collect();

    // Step 2: Temporal difference — current raw spec vs. max-filtered previous frame.
    for t in diff_frames..n_frames {
        let mut flux = 0.0f32;
        for b in 0..n_bands {
            let diff = spec_frames[t][b] - max_spec[t - diff_frames][b];
            if diff > 0.0 {
                flux += diff;
            }
        }
        onset[t] = flux;
    }

    // Robust normalize: skip first ~1s (HP filter transient), use 99th pct
    let skip_frames = 40.min(onset.len() / 4);
    if skip_frames < onset.len() {
        let mut sorted: Vec<f32> = onset[skip_frames..].iter().cloned().filter(|&v| v > 0.0).collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p99 = if sorted.is_empty() {
            0.0
        } else {
            sorted[(sorted.len() as f32 * 0.99) as usize]
        };
        if p99 > 0.0 {
            for v in &mut onset {
                *v /= p99;
            }
        }
    }

    onset
}

// ─── Adaptive threshold ─────────────────────────────────────────────────

/// Running median-based threshold with IQR multiplier.
///
/// For each frame, computes `max(0, onset[t] - (median + multiplier * IQR))`
/// over a local window, then re-normalizes by the 99th percentile.
fn adaptive_threshold(onset: &[f32], median_window: f32, multiplier: f32) -> Vec<f32> {
    let n = onset.len();
    let half = (median_window / 2.0).round() as usize;
    let mut thresholded = vec![0.0f32; n];

    for t in 0..n {
        let lo = t.saturating_sub(half);
        let hi = (t + half).min(n - 1);
        let count = hi - lo + 1;
        let mut window_vals: Vec<f32> = onset[lo..=hi].to_vec();
        window_vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median = window_vals[count / 2];
        // IQR-based threshold
        let iqr = window_vals[3 * count / 4] - median;
        let threshold = median + multiplier * iqr;
        thresholded[t] = (onset[t] - threshold).max(0.0);
    }

    // Robust re-normalize: skip first ~1s, use 99th percentile
    let skip = 40.min(thresholded.len() / 4);
    if skip < thresholded.len() {
        let mut vals: Vec<f32> = thresholded[skip..].iter().cloned().filter(|&v| v > 0.0).collect();
        vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p99 = vals.get((vals.len() as f32 * 0.99) as usize).copied().unwrap_or(0.0);
        if p99 > 0.0 {
            for v in &mut thresholded {
                *v /= p99;
            }
        }
    }

    thresholded
}

// ─── Linear interpolation ───────────────────────────────────────────────

/// Linear interpolation at time `t` (seconds) in a signal sampled every `dt`.
fn interpolate(data: &[f32], dt: f64, t: f64) -> f32 {
    if data.is_empty() {
        return 0.0;
    }
    let idx = t / dt;
    if idx < 0.0 {
        return data[0];
    }
    let i = idx as usize;
    if i + 1 >= data.len() {
        return data[data.len() - 1];
    }
    let frac = (idx - i as f64) as f32;
    let a = data[i];
    let b = data[i + 1];
    a + (b - a) * frac
}
