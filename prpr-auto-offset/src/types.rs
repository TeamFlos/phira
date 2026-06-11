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
    /// Positive means notes should be delayed (hit later).
    pub offset: f64,
    /// Normalized cross-correlation peak, in [0.0, 1.0].
    ///
    /// Values near 0 suggest the note pattern has no discernible match in
    /// the audio novelty, and the detected offset may be unreliable.
    pub correlation: f64,
    /// Whether the correlation exceeds the default reliability threshold.
    pub reliable: bool,
}
