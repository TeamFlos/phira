use crate::Signal;
use rustfft::{num_complex::Complex32, FftPlanner};

/// Spectral-flux novelty signal computed via STFT.
///
/// For each STFT frame, computes the sum of positive magnitude-spectrum
/// differences from the previous frame. The result is a dense time series
/// with one value per STFT frame.
pub struct SpectralFlux {
    /// Native novelty samples at the STFT hop rate.
    native: Vec<f32>,
    /// Time step between native samples, in seconds.
    native_dt: f64,
}

impl SpectralFlux {
    pub fn new(pcm: &[f32], sample_rate: u32, fft_size: usize, hop_size: usize) -> Self {
        assert!(fft_size.is_power_of_two());
        let native_dt = hop_size as f64 / sample_rate as f64;
        let native = compute_spectral_flux(pcm, fft_size, hop_size);
        Self { native, native_dt }
    }
}

impl Signal for SpectralFlux {
    fn samples(&self, ts: &[f64]) -> Vec<f32> {
        if ts.is_empty() {
            return vec![];
        }
        ts.iter().map(|&t| interpolate(&self.native, self.native_dt, t)).collect()
    }
}

fn compute_spectral_flux(pcm: &[f32], n: usize, hop: usize) -> Vec<f32> {
    if pcm.len() < n {
        return vec![];
    }

    let n2 = (n - 1) as f32;
    let window: Vec<f32> = (0..n).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n2).cos()).collect();

    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(n);

    let num_frames = (pcm.len() - n) / hop + 1;
    let num_bins = n / 2 + 1;
    let mut prev_mags = vec![0.0f32; num_bins];
    let mut buffer = vec![Complex32::new(0.0, 0.0); n];
    let mut novelty = Vec::with_capacity(num_frames);

    for frame in 0..num_frames {
        let start = frame * hop;
        for (i, &w) in window.iter().enumerate() {
            buffer[i] = Complex32::new(pcm[start + i] * w, 0.0);
        }

        fft.process(&mut buffer);

        for i in 0..num_bins {
            prev_mags[i] = core::mem::replace(&mut prev_mags[i], buffer[i].norm());
        }

        if frame == 0 {
            novelty.push(0.0);
            continue;
        }

        let flux: f32 = buffer[..num_bins]
            .iter()
            .enumerate()
            .map(|(i, c)| (c.norm() - prev_mags[i]).max(0.0))
            .sum();

        novelty.push(flux);
    }

    novelty
}

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
