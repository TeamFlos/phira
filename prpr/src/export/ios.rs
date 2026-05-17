//! iOS encoder backend.
//!
//! TODO: implement on top of `AVAssetWriter` + `AVAssetWriterInput` via the
//! objc bridge. Outline:
//!     let url = NSURL fileURLWithPath: cfg.output;
//!     let writer = AVAssetWriter assetWriterWithURL: url fileType: AVFileTypeMPEG4;
//!     let input = AVAssetWriterInput assetWriterInputWithMediaType: AVMediaTypeVideo
//!                 outputSettings: { AVVideoCodecKey: AVVideoCodecTypeH264, ... };
//!     let adaptor = AVAssetWriterInputPixelBufferAdaptor with input: input
//!                   sourcePixelBufferAttributes: { kCVPixelBufferPixelFormatTypeKey:
//!                                                  kCVPixelFormatType_32BGRA, ... };
//!     writer.add(input); writer.startWriting(); writer.startSession(atSourceTime: 0);
//!     // For each frame: create CVPixelBuffer, copy RGBA->BGRA flipping Y, append.
//!     // On finish: input.markAsFinished(); writer.finishWriting().

use super::{EncoderBackend, ExportConfig};
use anyhow::Result;
use std::path::{Path, PathBuf};

pub struct IosEncoder;

impl IosEncoder {
    pub fn new(_cfg: &ExportConfig) -> Result<Self> {
        Ok(Self)
    }
}

impl EncoderBackend for IosEncoder {
    fn encode_rgba(&mut self, _frame: &[u8]) -> Result<()> {
        Ok(())
    }

    fn finish(self: Box<Self>, _output: &Path) -> Result<PathBuf> {
        anyhow::bail!(
            "iOS MP4 export backend is not yet implemented. \
             See prpr/src/export/ios.rs for the planned AVAssetWriter integration."
        )
    }
}
