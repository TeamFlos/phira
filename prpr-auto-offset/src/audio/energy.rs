use crate::Signal;

/// Energy-difference novelty signal.
///
/// Computes the positive first-order difference of short-time RMS energy.
/// No thresholding — every frame gets a value.
pub struct EnergyDiff {
    /// Native novelty samples at the energy hop rate.
    native: Vec<f32>,
    /// Time step between native samples, in seconds.
    native_dt: f64,
}

impl EnergyDiff {
    pub fn new(pcm: &[f32], sample_rate: u32, frame_ms: f64, hop_ms: f64) -> Self {
        let frame_samples = (frame_ms / 1000.0 * sample_rate as f64).round() as usize;
        let hop_samples = (hop_ms / 1000.0 * sample_rate as f64).round() as usize;
        let native_dt = hop_samples as f64 / sample_rate as f64;

        let native = compute_energy_diff(pcm, frame_samples, hop_samples);
        Self { native, native_dt }
    }
}

impl Signal for EnergyDiff {
    fn samples(&self, ts: &[f64]) -> Vec<f32> {
        if ts.is_empty() {
            return vec![];
        }
        ts.iter().map(|&t| interpolate(&self.native, self.native_dt, t)).collect()
    }
}

fn compute_energy_diff(pcm: &[f32], frame_samples: usize, hop_samples: usize) -> Vec<f32> {
    if pcm.len() < frame_samples || frame_samples == 0 || hop_samples == 0 {
        return vec![];
    }

    let energies: Vec<f32> = (0..)
        .step_by(hop_samples)
        .take_while(|&start| start + frame_samples <= pcm.len())
        .map(|start| {
            let sum_sq: f32 = pcm[start..start + frame_samples].iter().map(|&x| x * x).sum();
            (sum_sq / frame_samples as f32).sqrt()
        })
        .collect();

    if energies.len() < 2 {
        return vec![];
    }

    energies.windows(2).map(|w| (w[1] - w[0]).max(0.0)).collect()
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
