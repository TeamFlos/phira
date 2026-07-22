use crate::Signal;

/// A diagnostic note signal constructed by placing a Gaussian kernel at each
/// note time.
///
/// Prefer [`PreprocessedNoteGaussian`](crate::PreprocessedNoteGaussian) for
/// offset suggestions because it uses note kinds and suppresses drag-run
/// artifacts.
pub struct NoteGaussian {
    times: Vec<f64>,
    sigma: f64,
}

impl NoteGaussian {
    pub fn new(times: Vec<f64>, sigma: f64) -> Self {
        assert!(sigma.is_finite(), "sigma must be finite");
        assert!(sigma > 0.0, "sigma must be positive");
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
