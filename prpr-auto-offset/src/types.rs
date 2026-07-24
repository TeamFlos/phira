/// Configuration for the alignment algorithm.
#[derive(Debug, Clone)]
pub struct AlignConfig {
    /// Search range for offset, in seconds. Default ±0.30s (narrow, centered at
    /// [`search_center_sec`]).
    pub search_range_sec: f64,
    /// Time step for the shared sampling grid, in seconds. Default 0.005 (5ms).
    pub sampling_interval_sec: f64,
    /// Center of the search window, in seconds.
    ///
    /// Set this to the chart author's configured offset so the algorithm only
    /// searches for a small correction nearby. Default 0.0.
    pub search_center_sec: f64,
}

impl Default for AlignConfig {
    fn default() -> Self {
        Self {
            search_range_sec: 0.30,
            sampling_interval_sec: 0.005,
            search_center_sec: 0.0,
        }
    }
}

/// Full result of automatic offset detection.
#[derive(Debug, Clone)]
pub struct AlignmentResult {
    /// Suggested global offset in seconds.
    /// This value is in absolute time. To get the chart offset correction, subtract the search center:
    ///     chart_offset_correction = offset - search_center_sec
    pub offset: f64,
    /// Normalized cross-correlation peak, in [0.0, 1.0].
    ///
    /// Values near 0 suggest the note pattern has no discernible match in
    /// the audio novelty, and the detected offset may be unreliable.
    pub correlation: f64,
    /// Unnormalized dot-product peak at the selected offset.
    pub raw_peak: f64,
    /// Squared L2 energy of the sampled note signal used for this estimate.
    pub note_energy: f64,
    /// Squared L2 energy of the sampled audio novelty signal used for this estimate.
    pub audio_energy: f64,
    /// Whether the correlation exceeds the default reliability threshold.
    pub reliable: bool,
    /// Full correlation curve: (offset_seconds, normalized_correlation_score).
    /// Useful for visualization of the score-vs-offset landscape.
    /// The offset_seconds values are in absolute time, so the search center is at `search_center_sec`.
    pub correlation_curve: Vec<(f64, f32)>,
}
