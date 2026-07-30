use crate::Signal;

/// A diagnostic note signal constructed by placing a Gaussian kernel at each
/// note time.
///
/// Prefer [`WeightedGaussianNote`](crate::WeightedGaussianNote) for
/// offset suggestions because it uses note kinds and suppresses drag-run
/// artifacts.
pub struct GaussianNote {
    times: Vec<f64>,
    sigma: f64,
}

impl GaussianNote {
    pub fn new(times: Vec<f64>, sigma: f64) -> Self {
        debug_assert!(sigma.is_finite(), "sigma must be finite");
        debug_assert!(sigma > 0.0, "sigma must be positive");
        Self { times, sigma }
    }
}

impl Signal for GaussianNote {
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
