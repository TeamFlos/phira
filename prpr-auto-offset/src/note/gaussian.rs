use crate::Signal;

/// A signal constructed from discrete note events by placing a Gaussian kernel
/// at each note time.
///
/// `template(t) = Σ exp(-0.5 * ((t - tᵢ) / σ)²)` — an analytic function that
/// can be sampled at arbitrary timestamps.
pub struct NoteGaussian {
    times: Vec<f64>,
    sigma: f64,
}

impl NoteGaussian {
    pub fn new(times: Vec<f64>, sigma: f64) -> Self {
        Self { times, sigma }
    }
}

impl Signal for NoteGaussian {
    fn samples(&self, ts: &[f64]) -> Vec<f32> {
        if ts.is_empty() || self.times.is_empty() {
            return vec![0.0; ts.len()];
        }
        let inv_sigma = 1.0 / self.sigma;
        ts.iter()
            .map(|&t| {
                self.times
                    .iter()
                    .map(|&nt| {
                        let d = (t - nt) * inv_sigma;
                        ((-0.5 * d * d).exp()) as f32
                    })
                    .sum::<f32>()
            })
            .collect()
    }
}
