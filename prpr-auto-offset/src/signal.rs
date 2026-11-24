/// A dense time-varying signal that can be sampled at arbitrary timestamps.
///
/// The signal is conceptually continuous: implementations may store native
/// samples at a fixed internal resolution and interpolate, or compute values
/// analytically on demand.
pub trait Signal<T = f32>: Send + Sync {
    /// Sample the signal at the given timestamps.
    ///
    /// Returns one value per timestamp, in the same order.
    fn samples(&self, ts: &[f64]) -> Vec<T>;
}
