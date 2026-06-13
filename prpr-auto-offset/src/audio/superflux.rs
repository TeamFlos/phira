use crate::Signal;

/// SuperFlux onset detection signal.
///
/// Computes a percussion-onset novelty curve using the SuperFlux algorithm:
///   1. High-pass filter (50 Hz) to remove sub-bass rumble
///   2. Mel filterbank (80 bands, 50 Hz – 12 kHz)
///   3. Mel-spectrogram (Hann-windowed STFT → mel band energy in dB)
///   4. Per-band spectral whitening (subtract local running mean)
///   5. SuperFlux temporal difference (max-filtered spectral flux)
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

        // 2. Mel filterbank (80 bands, 50Hz–12kHz)
        let mel = MelFilterbank::new(sample_rate, window_size, 80, 50.0, 12000.0);

        // 3. Mel-spectrogram
        let (mut mel_frames, frame_rate) =
            compute_mel_spectrogram(&samples, sample_rate, window_size, hop_size, &mel);

        // 4. Spectral whitening (1-second window)
        whiten_spectrogram(&mut mel_frames, (frame_rate * 1.0) as usize);

        // 5. SuperFlux temporal difference (lag=3)
        let onset = compute_superflux(&mel_frames, 3);

        // 6. Adaptive threshold
        let onset = adaptive_threshold(&onset, frame_rate * 2.0, 0.5);

        // Use the declared native_dt (frame_rate may differ slightly due to rounding)
        let _ = frame_rate;
        Self {
            native: onset,
            native_dt,
        }
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
        ts.iter()
            .map(|&t| interpolate(&self.native, self.native_dt, t))
            .collect()
    }
}

// ─── High-pass filter (50 Hz) ──────────────────────────────────────────

fn highpass_50hz(samples: &mut [f32], sample_rate: u32) {
    // 1st-order Butterworth: y[n] = alpha*y[n-1] + alpha*(x[n] - x[n-1])
    // Remove DC offset first, then initialize state to avoid transient
    let dc = samples
        .iter()
        .take((sample_rate as usize / 10).min(samples.len()))
        .sum::<f32>()
        / (sample_rate as f32 / 10.0).min(samples.len() as f32);
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

// ─── Mel scale conversion ──────────────────────────────────────────────

fn hz_to_mel(hz: f32) -> f32 {
    2595.0 * (1.0 + hz / 700.0).log10()
}

fn mel_to_hz(mel: f32) -> f32 {
    700.0 * (10.0f32.powf(mel / 2595.0) - 1.0)
}

// ─── Mel filterbank ─────────────────────────────────────────────────────

pub struct MelFilterbank {
    /// Triangular filter weights: [mel_band][fft_bin]
    pub weights: Vec<Vec<f32>>,
    pub n_mels: usize,
}

impl MelFilterbank {
    pub fn new(sample_rate: u32, window_size: usize, n_mels: usize, f_min: f32, f_max: f32) -> Self {
        let n_fft_bins = window_size / 2 + 1;
        let mel_min = hz_to_mel(f_min);
        let mel_max = hz_to_mel(f_max.min(sample_rate as f32 / 2.0));
        let mel_step = (mel_max - mel_min) / (n_mels + 1) as f32;

        // Center frequencies of mel bands
        let mel_centers: Vec<f32> = (0..n_mels)
            .map(|i| mel_to_hz(mel_min + (i + 1) as f32 * mel_step))
            .collect();

        let bin_hz = |k: usize| k as f32 * sample_rate as f32 / window_size as f32;

        // Build triangular filter weights
        let mut weights = vec![vec![0.0f32; n_fft_bins]; n_mels];
        for (m, &center) in mel_centers.iter().enumerate() {
            let left = if m == 0 {
                f_min
            } else {
                mel_centers[m - 1]
            };
            let right = if m == n_mels - 1 {
                f_max
            } else {
                mel_centers[m + 1]
            };
            for (k, w) in weights[m].iter_mut().enumerate() {
                let f = bin_hz(k);
                if f >= left && f <= center {
                    *w = (f - left) / (center - left).max(1e-10);
                } else if f > center && f <= right {
                    *w = (right - f) / (right - center).max(1e-10);
                }
            }
        }

        MelFilterbank { weights, n_mels }
    }

    /// Apply mel filterbank to a power spectrum, returns log-magnitudes per mel band (dB).
    pub fn apply(&self, power_spectrum: &[f32]) -> Vec<f32> {
        let mut mel = vec![0.0f32; self.n_mels];
        for (m, w) in self.weights.iter().enumerate() {
            let sum: f32 = power_spectrum.iter().zip(w).map(|(&p, &w)| p * w).sum();
            mel[m] = 20.0 * (sum.sqrt().max(1e-10)).log10(); // dB
        }
        mel
    }
}

// ─── Mel-spectrogram computation ────────────────────────────────────────

pub fn compute_mel_spectrogram(
    samples: &[f32],
    sample_rate: u32,
    window_size: usize,
    hop_size: usize,
    mel: &MelFilterbank,
) -> (Vec<Vec<f32>>, f32) {
    use rayon::prelude::*;
    use realfft::RealFftPlanner;
    use std::sync::Arc;

    let num_frames = if samples.len() < window_size {
        0
    } else {
        (samples.len() - window_size) / hop_size + 1
    };

    let window: Vec<f32> = (0..window_size)
        .map(|n| {
            0.5 * (1.0
                - (2.0 * std::f32::consts::PI * n as f32 / (window_size - 1) as f32).cos())
        })
        .collect();

    let mut planner = RealFftPlanner::<f32>::new();
    let r2c = Arc::new(planner.plan_fft_forward(window_size));

    let mel_frames: Vec<Vec<f32>> = (0..num_frames)
        .into_par_iter()
        .map(|frame_idx| {
            let start = frame_idx * hop_size;
            let mut windowed: Vec<f32> = samples[start..start + window_size]
                .iter()
                .zip(&window)
                .map(|(&s, &w)| s * w)
                .collect();

            let mut spectrum = r2c.make_output_vec();
            r2c.process(&mut windowed, &mut spectrum).unwrap();

            let power: Vec<f32> = spectrum
                .iter()
                .map(|c| c.re * c.re + c.im * c.im)
                .collect();
            mel.apply(&power)
        })
        .collect();

    let frame_rate = sample_rate as f32 / hop_size as f32;
    (mel_frames, frame_rate)
}

// ─── Spectral whitening ─────────────────────────────────────────────────

/// For each mel band, subtract a local running mean (half-width = window_frames/2).
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
/// For each frame `t` and each mel band `b`:
///   `diff(t) = Σ_b max(0, X[t][b] - max(X[t-1][b], ..., X[t-lag][b]))`
///
/// Robust-normalized by the 99th percentile (skipping the first ~1 s to avoid
/// HP filter transient).
fn compute_superflux(mel_frames: &[Vec<f32>], lag: usize) -> Vec<f32> {
    let n_frames = mel_frames.len();
    if n_frames <= lag {
        return vec![0.0; n_frames];
    }

    let mut onset = vec![0.0f32; n_frames];

    for t in lag..n_frames {
        let mut flux = 0.0f32;
        for (b, &cur) in mel_frames[t].iter().enumerate() {
            // Max of previous `lag` frames
            let mut max_prev = mel_frames[t - 1][b];
            for d in 2..=lag {
                max_prev = max_prev.max(mel_frames[t - d][b]);
            }
            let diff = cur - max_prev;
            if diff > 0.0 {
                flux += diff;
            }
        }
        onset[t] = flux;
    }

    // Robust normalize: skip first ~1s (HP filter transient), use 99th pct
    let skip_frames = 40.min(onset.len() / 4);
    if skip_frames < onset.len() {
        let mut sorted: Vec<f32> = onset[skip_frames..]
            .iter()
            .cloned()
            .filter(|&v| v > 0.0)
            .collect();
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
        let mut vals: Vec<f32> = thresholded[skip..]
            .iter()
            .cloned()
            .filter(|&v| v > 0.0)
            .collect();
        vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p99 = vals
            .get((vals.len() as f32 * 0.99) as usize)
            .copied()
            .unwrap_or(0.0);
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
