//! Capture the current game scene frame-by-frame into an MP4. Each platform
//! has its own backend:
//!
//! * `desktop` (Linux / macOS / Windows): pipe raw RGBA frames into a
//!   spawned `ffmpeg` CLI. The ffmpeg binary must be on PATH.
//! * `android`: drive Android's native `MediaCodec` H.264 encoder + the
//!   `MediaMuxer` muxer over JNI. No external dependencies.
//! * `ios`: drive `AVAssetWriter` over the objc bridge. No external deps.
//!
//! The host (e.g. `phira`) constructs an `Exporter` on the main thread,
//! queues it for the next `GameScene`, and the engine pumps each rendered
//! frame into `Exporter::capture_frame` after rendering. When the game
//! finishes, the engine calls `Exporter::finish` to flush and close the file.

use anyhow::{Context, Result};
use macroquad::prelude::*;
use std::{cell::RefCell, path::PathBuf};

thread_local! {
    /// Pending exporter that the next-built `GameScene` picks up.
    static PENDING_EXPORTER: RefCell<Option<Exporter>> = const { RefCell::new(None) };
    /// Set to `Ok(path)` when the most recent export finished writing the
    /// mp4, or `Err(message)` on failure. Consumers (like `ReplayListPage`)
    /// poll this once per frame to react.
    static LAST_EXPORT_RESULT: RefCell<Option<Result<PathBuf, String>>> =
        const { RefCell::new(None) };
}

pub fn set_pending_exporter(e: Exporter) {
    PENDING_EXPORTER.with(|cell| *cell.borrow_mut() = Some(e));
}

pub fn take_pending_exporter() -> Option<Exporter> {
    PENDING_EXPORTER.with(|cell| cell.borrow_mut().take())
}

/// Called by the engine after the encoder has flushed, to publish the
/// finished mp4 path (or an error) for the host to consume.
pub fn publish_export_result(r: Result<PathBuf, String>) {
    LAST_EXPORT_RESULT.with(|cell| *cell.borrow_mut() = Some(r));
}

pub fn take_export_result() -> Option<Result<PathBuf, String>> {
    LAST_EXPORT_RESULT.with(|cell| cell.borrow_mut().take())
}

/// Configuration for an in-progress export.
#[derive(Clone, Debug)]
pub struct ExportConfig {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// Final output path. Must end with `.mp4`.
    pub output: PathBuf,
    /// Optional audio file to mux into the final mp4. Currently honored only
    /// by the desktop ffmpeg backend.
    pub audio_path: Option<PathBuf>,
}

/// Backend-specific encoder. Each platform implements this; the public
/// `Exporter` facade routes to the appropriate one.
trait EncoderBackend {
    fn encode_rgba(&mut self, frame: &[u8]) -> Result<()>;
    fn finish(self: Box<Self>, output: &std::path::Path) -> Result<PathBuf>;
}

pub struct Exporter {
    cfg: ExportConfig,
    target: RenderTarget,
    backend: Box<dyn EncoderBackend>,
    frame_buf: Vec<u8>,
    frame_count: u64,
}

impl Exporter {
    pub fn new(cfg: ExportConfig) -> Result<Self> {
        let target = render_target(cfg.width, cfg.height);

        // Pick a platform-appropriate encoder backend. Each impl is a no-op
        // shim on platforms it doesn't support, so we can dispatch without
        // sprinkling cfg() across the call sites.
        let backend: Box<dyn EncoderBackend> = pick_backend(&cfg)?;

        Ok(Self {
            cfg,
            target,
            backend,
            frame_buf: Vec::new(),
            frame_count: 0,
        })
    }

    #[inline]
    pub fn render_target(&self) -> RenderTarget {
        self.target
    }

    #[inline]
    pub fn width(&self) -> u32 {
        self.cfg.width
    }

    #[inline]
    pub fn height(&self) -> u32 {
        self.cfg.height
    }

    /// Read back the render target and feed one RGBA frame into the encoder.
    pub fn capture_frame(&mut self) -> Result<()> {
        let w = self.cfg.width as usize;
        let h = self.cfg.height as usize;
        let size = w * h * 4;
        if self.frame_buf.len() != size {
            self.frame_buf.resize(size, 0);
        }
        self.target.texture.raw_miniquad_texture_handle().read_pixels(&mut self.frame_buf);
        self.backend.encode_rgba(&self.frame_buf)?;
        self.frame_count += 1;
        Ok(())
    }

    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    /// Close the encoder, finalize the file, and (optionally) mux audio.
    pub fn finish(self) -> Result<PathBuf> {
        self.backend.finish(&self.cfg.output)
    }
}

// --------------------------------------------------------------------------
// Backend selection.
// --------------------------------------------------------------------------

fn pick_backend(cfg: &ExportConfig) -> Result<Box<dyn EncoderBackend>> {
    #[cfg(target_os = "android")]
    {
        android::AndroidEncoder::new(cfg).map(|b| Box::new(b) as Box<dyn EncoderBackend>)
    }
    #[cfg(target_os = "ios")]
    {
        ios::IosEncoder::new(cfg).map(|b| Box::new(b) as Box<dyn EncoderBackend>)
    }
    #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
    {
        desktop::FfmpegCliEncoder::new(cfg).map(|b| Box::new(b) as Box<dyn EncoderBackend>)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = cfg;
        anyhow::bail!("MP4 export is not supported on wasm32")
    }
}

// --------------------------------------------------------------------------
// Desktop backend: spawn ffmpeg CLI.
// --------------------------------------------------------------------------

#[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
mod desktop {
    use super::*;
    use std::io::Write;
    use std::path::Path;
    use std::process::{Child, ChildStdin, Command, Stdio};

    pub struct FfmpegCliEncoder {
        child: Child,
        stdin: Option<ChildStdin>,
        video_tmp: PathBuf,
        audio_path: Option<PathBuf>,
    }

    impl FfmpegCliEncoder {
        pub fn new(cfg: &ExportConfig) -> Result<Self> {
            let video_tmp = std::env::temp_dir().join(format!("phira_export_{}.mp4", std::process::id()));

            let mut child = Command::new("ffmpeg")
                .args([
                    "-hide_banner",
                    "-loglevel",
                    "warning",
                    "-y",
                    "-f",
                    "rawvideo",
                    "-pixel_format",
                    "rgba",
                    "-video_size",
                    &format!("{}x{}", cfg.width, cfg.height),
                    "-framerate",
                    &cfg.fps.to_string(),
                    "-i",
                    "-",
                    "-vf",
                    "vflip",
                    "-c:v",
                    "libx264",
                    "-preset",
                    "veryfast",
                    "-pix_fmt",
                    "yuv420p",
                    "-movflags",
                    "+faststart",
                ])
                .arg(video_tmp.as_os_str())
                .stdin(Stdio::piped())
                .stderr(Stdio::inherit())
                .stdout(Stdio::inherit())
                .spawn()
                .context("failed to spawn ffmpeg; is it installed and on PATH?")?;
            let stdin = child.stdin.take();
            Ok(Self {
                child,
                stdin,
                video_tmp,
                audio_path: cfg.audio_path.clone(),
            })
        }
    }

    impl EncoderBackend for FfmpegCliEncoder {
        fn encode_rgba(&mut self, frame: &[u8]) -> Result<()> {
            if let Some(stdin) = self.stdin.as_mut() {
                stdin.write_all(frame)?;
            }
            Ok(())
        }

        fn finish(mut self: Box<Self>, output: &Path) -> Result<PathBuf> {
            drop(self.stdin.take());
            let status = self.child.wait().context("waiting for video ffmpeg")?;
            if !status.success() {
                anyhow::bail!("video ffmpeg exited with status {:?}", status.code());
            }

            let Some(audio) = self.audio_path.as_ref() else {
                if output.exists() {
                    let _ = std::fs::remove_file(output);
                }
                std::fs::rename(&self.video_tmp, output).or_else(|_| {
                    std::fs::copy(&self.video_tmp, output)
                        .map(|_| ())
                        .and_then(|_| std::fs::remove_file(&self.video_tmp))
                })?;
                return Ok(output.to_owned());
            };

            let mux_status = Command::new("ffmpeg")
                .args(["-hide_banner", "-loglevel", "warning", "-y", "-i"])
                .arg(self.video_tmp.as_os_str())
                .arg("-i")
                .arg(audio.as_os_str())
                .args(["-c:v", "copy", "-c:a", "aac", "-shortest", "-movflags", "+faststart"])
                .arg(output.as_os_str())
                .status()
                .context("failed to run ffmpeg mux pass")?;
            let _ = std::fs::remove_file(&self.video_tmp);
            if !mux_status.success() {
                anyhow::bail!("mux ffmpeg exited with status {:?}", mux_status.code());
            }
            Ok(output.to_owned())
        }
    }
}

// --------------------------------------------------------------------------
// Android backend: pure JNI to MediaCodec + MediaMuxer.
// --------------------------------------------------------------------------

#[cfg(target_os = "android")]
mod android;

// --------------------------------------------------------------------------
// iOS backend: AVAssetWriter via objc.
// --------------------------------------------------------------------------

#[cfg(target_os = "ios")]
mod ios;
