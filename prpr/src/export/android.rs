//! Android encoder backend.
//!
//! TODO: replace this stub with a real `MediaCodec` (H.264) + `MediaMuxer`
//! mp4 implementation driven over JNI. The infrastructure for picking a
//! save path and copying the encoder's output into the user-chosen file is
//! already in place via the chart-export pipeline; only the encoder itself
//! needs to land.

use super::{EncoderBackend, ExportConfig};
use anyhow::Result;
use std::path::{Path, PathBuf};

pub struct AndroidEncoder {
    output: PathBuf,
}

impl AndroidEncoder {
    pub fn new(cfg: &ExportConfig) -> Result<Self> {
        Ok(Self { output: cfg.output.clone() })
    }
}

impl EncoderBackend for AndroidEncoder {
    fn encode_rgba(&mut self, _frame: &[u8]) -> Result<()> {
        Ok(())
    }

    fn finish(self: Box<Self>, _output: &Path) -> Result<PathBuf> {
        anyhow::bail!(
            "Android MP4 export backend is not yet implemented. \
             See prpr/src/export/android.rs for the planned MediaCodec integration."
        )
    }
}

#[allow(dead_code)]
impl AndroidEncoder {
    fn _placeholder_uses_output(&self) -> &Path {
        &self.output
    }
}
