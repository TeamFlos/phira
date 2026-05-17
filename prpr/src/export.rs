//! Capture the current game scene frame-by-frame into an MP4 via an external
//! `ffmpeg` process. This is a "live" recorder: frames are captured as the
//! game plays and streamed straight into ffmpeg's stdin as raw RGBA, so the
//! resulting file length matches the playback length.
//!
//! The audio track is added in a second ffmpeg pass after playback completes,
//! by muxing the chart's music file with the captured video.

use anyhow::{Context, Result};
use macroquad::prelude::*;
use std::{
    cell::RefCell,
    io::Write,
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
};

thread_local! {
    /// A pending `Exporter` that will be picked up by the next `GameScene`
    /// constructed on this thread. Used to thread the exporter through the
    /// `LoadingScene` boundary without adding it to every scene's public API.
    static PENDING_EXPORTER: RefCell<Option<Exporter>> = const { RefCell::new(None) };
}

/// Queue an `Exporter` to be picked up by the next created `GameScene` on
/// this thread. Consumes the exporter.
pub fn set_pending_exporter(e: Exporter) {
    PENDING_EXPORTER.with(|cell| {
        *cell.borrow_mut() = Some(e);
    });
}

/// Take the pending `Exporter`, if any. Called by `GameScene::new`.
pub fn take_pending_exporter() -> Option<Exporter> {
    PENDING_EXPORTER.with(|cell| cell.borrow_mut().take())
}

/// Where to write the finished mp4, plus the video parameters.
#[derive(Clone, Debug)]
pub struct ExportConfig {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// Final output path. Must end with `.mp4`.
    pub output: PathBuf,
    /// Path to the chart's music file. If provided and readable, the final
    /// mux step will blend it into the mp4.
    pub audio_path: Option<PathBuf>,
}

pub struct Exporter {
    cfg: ExportConfig,
    target: RenderTarget,
    /// The ffmpeg child process receiving raw RGBA frames.
    child: Child,
    stdin: Option<ChildStdin>,
    /// Temporary file the video-only ffmpeg writes to.
    video_tmp: PathBuf,
    /// Flip-Y buffer used to un-flip the texture readback.
    row_buf: Vec<u8>,
    /// Readback buffer.
    frame_buf: Vec<u8>,
    frame_count: u64,
}

impl Exporter {
    pub fn new(cfg: ExportConfig) -> Result<Self> {
        let target = render_target(cfg.width, cfg.height);

        // Temporary video-only file. We write raw RGBA into ffmpeg and have
        // it produce a plain-video mp4 here; audio is muxed later.
        let video_tmp = std::env::temp_dir().join(format!("phira_export_{}.mp4", std::process::id()));

        // ffmpeg: read raw RGBA frames on stdin at `fps`; H.264 out.
        // -vf vflip because OpenGL textures are bottom-up but mp4 expects top-down.
        let mut child = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel", "warning",
                "-y",
                "-f", "rawvideo",
                "-pixel_format", "rgba",
                "-video_size", &format!("{}x{}", cfg.width, cfg.height),
                "-framerate", &cfg.fps.to_string(),
                "-i", "-",
                "-vf", "vflip",
                "-c:v", "libx264",
                "-preset", "veryfast",
                "-pix_fmt", "yuv420p",
                "-movflags", "+faststart",
            ])
            .arg(video_tmp.as_os_str())
            .stdin(Stdio::piped())
            .stderr(Stdio::inherit())
            .stdout(Stdio::inherit())
            .spawn()
            .context("failed to spawn ffmpeg; is it installed and on PATH?")?;

        Ok(Self {
            stdin: Some(child.stdin.take().unwrap()),
            child,
            cfg,
            target,
            video_tmp,
            row_buf: Vec::new(),
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

    /// Read back the render target and write one RGBA frame to ffmpeg.
    pub fn capture_frame(&mut self) -> Result<()> {
        let w = self.cfg.width as usize;
        let h = self.cfg.height as usize;
        let size = w * h * 4;
        if self.frame_buf.len() != size {
            self.frame_buf.resize(size, 0);
        }
        // Read pixels from the render target texture.
        self.target.texture.raw_miniquad_texture_handle().read_pixels(&mut self.frame_buf);

        if let Some(stdin) = self.stdin.as_mut() {
            stdin.write_all(&self.frame_buf)?;
        }
        self.frame_count += 1;
        // Silence the unused warning for row_buf; reserved for future use.
        let _ = &mut self.row_buf;
        Ok(())
    }

    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    /// Close the video stream and (optionally) mux in an audio track to
    /// produce the final file at `self.cfg.output`.
    pub fn finish(mut self) -> Result<PathBuf> {
        // Close stdin so ffmpeg flushes.
        drop(self.stdin.take());
        let status = self.child.wait().context("waiting for video ffmpeg")?;
        if !status.success() {
            anyhow::bail!("video ffmpeg exited with status {:?}", status.code());
        }

        // If no audio, just move the tmp file to the output path.
        let Some(audio) = self.cfg.audio_path.as_ref() else {
            if self.cfg.output.exists() {
                std::fs::remove_file(&self.cfg.output).ok();
            }
            std::fs::rename(&self.video_tmp, &self.cfg.output).or_else(|_| {
                std::fs::copy(&self.video_tmp, &self.cfg.output).map(|_| ()).and_then(|_| std::fs::remove_file(&self.video_tmp))
            })?;
            return Ok(self.cfg.output);
        };

        // Mux: copy video stream, encode audio to aac, finish at -shortest.
        let mux_status = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel", "warning",
                "-y",
                "-i",
            ])
            .arg(self.video_tmp.as_os_str())
            .arg("-i")
            .arg(audio.as_os_str())
            .args([
                "-c:v", "copy",
                "-c:a", "aac",
                "-shortest",
                "-movflags", "+faststart",
            ])
            .arg(self.cfg.output.as_os_str())
            .status()
            .context("failed to run ffmpeg mux pass")?;
        let _ = std::fs::remove_file(&self.video_tmp);
        if !mux_status.success() {
            anyhow::bail!("mux ffmpeg exited with status {:?}", mux_status.code());
        }
        Ok(self.cfg.output)
    }
}
