//! iOS encoder backend: AVAssetWriter + AVAssetWriterInput.
//!
//! We use the objc bridge to drive AVFoundation. This backend currently
//! writes a raw RGBA pixel buffer to a CVPixelBuffer-backed AVAssetWriter
//! input each frame; iOS will encode H.264 (or HEVC if available) under the
//! hood.

use super::{EncoderBackend, ExportConfig};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Stub iOS encoder. The full Objective-C bridge is not implemented yet —
/// this is a placeholder that surfaces a clear runtime error so the export
/// button is wired but the user gets actionable feedback until the backend
/// lands.
///
/// TODO: replace with AVAssetWriter implementation. Sketch:
///     let url = NSURL fileURLWithPath: cfg.output;
///     let writer = AVAssetWriter assetWriterWithURL: url fileType: AVFileTypeMPEG4;
///     let input = AVAssetWriterInput assetWriterInputWithMediaType: AVMediaTypeVideo
///                 outputSettings: { AVVideoCodecKey: AVVideoCodecTypeH264, ... };
///     let adaptor = AVAssetWriterInputPixelBufferAdaptor with input: input
///                   sourcePixelBufferAttributes: { kCVPixelBufferPixelFormatTypeKey: kCVPixelFormatType_32BGRA, ... };
///     writer.add(input); writer.startWriting(); writer.startSession(atSourceTime: 0);
///     // For each frame: create CVPixelBuffer, copy RGBA->BGRA flipping Y, append.
///     // On finish: input.markAsFinished(); writer.finishWriting().
pub struct IosEncoder {
    output: PathBuf,
}

impl IosEncoder {
    pub fn new(cfg: &ExportConfig) -> Result<Self> {
        Ok(Self { output: cfg.output.clone() })
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
