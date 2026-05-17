//! Android encoder backend: MediaCodec H.264 + MediaMuxer mp4.
//!
//! All native interop is via JNI; the host APK does not need any extra
//! Java/Kotlin helper classes. The codec is fed YUV420SemiPlanar (NV12)
//! frames converted from RGBA on the CPU, which is fast enough for 1080p60
//! on any modern device.
//!
//! Audio is not yet captured. The output mp4 has a single H.264 video track.

use super::{EncoderBackend, ExportConfig};
use anyhow::{anyhow, Result};
use jni::{
    jni_sig, jni_str,
    objects::{GlobalRef, JObject, JValue},
    sys::jint,
    vm::JavaVM,
    Env,
};
use std::path::{Path, PathBuf};

const TIMEOUT_US: i64 = 10_000;

const BUFFER_FLAG_END_OF_STREAM: jint = 4;
const BUFFER_FLAG_CODEC_CONFIG: jint = 2;
const INFO_TRY_AGAIN_LATER: jint = -1;
const INFO_OUTPUT_FORMAT_CHANGED: jint = -2;
const COLOR_FORMAT_YUV420_SEMI_PLANAR: jint = 21;
const CONFIGURE_FLAG_ENCODE: jint = 1;
const MUXER_OUTPUT_MPEG_4: jint = 0;

pub struct AndroidEncoder {
    width: u32,
    height: u32,
    fps: u32,
    codec: GlobalRef,
    muxer: GlobalRef,
    /// Pre-allocated YUV plane buffer reused for every frame.
    yuv: Vec<u8>,
    frame_idx: u64,
    track_idx: Option<jint>,
    muxer_started: bool,
    output: PathBuf,
}

impl AndroidEncoder {
    pub fn new(cfg: &ExportConfig) -> Result<Self> {
        if let Some(parent) = cfg.output.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let path_str = cfg.output.to_str().ok_or_else(|| anyhow!("non-utf8 output path"))?.to_owned();
        let width = cfg.width;
        let height = cfg.height;
        let fps = cfg.fps;

        let (codec, muxer) = JavaVM::singleton()?.attach_current_thread(|env| -> jni::errors::Result<(GlobalRef, GlobalRef)> {
            // 1) Build MediaFormat for H.264.
            let mime = env.new_string("video/avc")?;
            let format = env
                .call_static_method(
                    jni_str!("android/media/MediaFormat"),
                    jni_str!("createVideoFormat"),
                    jni_sig!("(Ljava/lang/String;II)Landroid/media/MediaFormat;"),
                    &[(&mime).into(), (width as jint).into(), (height as jint).into()],
                )?
                .l()?;

            let bitrate_key = env.new_string("bitrate")?;
            let framerate_key = env.new_string("frame-rate")?;
            let iframe_key = env.new_string("i-frame-interval")?;
            let color_key = env.new_string("color-format")?;
            let bitrate = (width as u64 * height as u64 * fps as u64 / 4).min(40_000_000) as jint;
            env.call_method(&format, jni_str!("setInteger"), jni_sig!("(Ljava/lang/String;I)V"), &[(&bitrate_key).into(), bitrate.into()])?;
            env.call_method(&format, jni_str!("setInteger"), jni_sig!("(Ljava/lang/String;I)V"), &[(&framerate_key).into(), (fps as jint).into()])?;
            env.call_method(&format, jni_str!("setInteger"), jni_sig!("(Ljava/lang/String;I)V"), &[(&iframe_key).into(), 1.into()])?;
            env.call_method(
                &format,
                jni_str!("setInteger"),
                jni_sig!("(Ljava/lang/String;I)V"),
                &[(&color_key).into(), COLOR_FORMAT_YUV420_SEMI_PLANAR.into()],
            )?;

            // 2) Create encoder.
            let mime_avc = env.new_string("video/avc")?;
            let codec = env
                .call_static_method(
                    jni_str!("android/media/MediaCodec"),
                    jni_str!("createEncoderByType"),
                    jni_sig!("(Ljava/lang/String;)Landroid/media/MediaCodec;"),
                    &[(&mime_avc).into()],
                )?
                .l()?;
            env.call_method(
                &codec,
                jni_str!("configure"),
                jni_sig!("(Landroid/media/MediaFormat;Landroid/view/Surface;Landroid/media/MediaCrypto;I)V"),
                &[
                    (&format).into(),
                    (&JObject::null()).into(),
                    (&JObject::null()).into(),
                    CONFIGURE_FLAG_ENCODE.into(),
                ],
            )?;
            env.call_method(&codec, jni_str!("start"), jni_sig!("()V"), &[])?;

            // 3) Create MediaMuxer.
            let path_jstr = env.new_string(&path_str)?;
            let muxer = env.new_object(
                jni_str!("android/media/MediaMuxer"),
                jni_sig!("(Ljava/lang/String;I)V"),
                &[(&path_jstr).into(), MUXER_OUTPUT_MPEG_4.into()],
            )?;

            Ok((env.new_global_ref(codec)?, env.new_global_ref(muxer)?))
        })?;

        let yuv_size = (width as usize) * (height as usize) * 3 / 2;
        Ok(Self {
            width,
            height,
            fps,
            codec,
            muxer,
            yuv: vec![0u8; yuv_size],
            frame_idx: 0,
            track_idx: None,
            muxer_started: false,
            output: cfg.output.clone(),
        })
    }

    /// RGBA → NV12 (BT.601 limited range), with vertical flip because
    /// OpenGL textures are bottom-up.
    fn rgba_to_nv12(&mut self, rgba: &[u8]) {
        let w = self.width as usize;
        let h = self.height as usize;
        let y_size = w * h;
        let (y_plane, uv_plane) = self.yuv.split_at_mut(y_size);

        for row in 0..h {
            let src_row = h - 1 - row;
            let row_off = src_row * w * 4;
            let dst_y_row = row * w;
            for col in 0..w {
                let i = row_off + col * 4;
                let r = rgba[i] as f32;
                let g = rgba[i + 1] as f32;
                let b = rgba[i + 2] as f32;
                let y = (0.257 * r + 0.504 * g + 0.098 * b + 16.0).clamp(0., 255.) as u8;
                y_plane[dst_y_row + col] = y;
                if (row & 1) == 0 && (col & 1) == 0 {
                    let u = (-0.148 * r - 0.291 * g + 0.439 * b + 128.0).clamp(0., 255.) as u8;
                    let v = (0.439 * r - 0.368 * g - 0.071 * b + 128.0).clamp(0., 255.) as u8;
                    let uv_off = (row / 2) * w + col;
                    uv_plane[uv_off] = u;
                    uv_plane[uv_off + 1] = v;
                }
            }
        }
    }

    /// Pull all available output buffers and feed them to MediaMuxer.
    fn drain_output(&mut self, env: &mut Env, end_of_stream: bool) -> jni::errors::Result<()> {
        loop {
            let info = env.new_object(jni_str!("android/media/MediaCodec$BufferInfo"), jni_sig!("()V"), &[])?;
            let idx = env
                .call_method(
                    &self.codec,
                    jni_str!("dequeueOutputBuffer"),
                    jni_sig!("(Landroid/media/MediaCodec$BufferInfo;J)I"),
                    &[(&info).into(), TIMEOUT_US.into()],
                )?
                .i()?;
            if idx == INFO_TRY_AGAIN_LATER {
                if !end_of_stream {
                    return Ok(());
                }
                continue;
            }
            if idx == INFO_OUTPUT_FORMAT_CHANGED {
                let new_fmt = env
                    .call_method(&self.codec, jni_str!("getOutputFormat"), jni_sig!("()Landroid/media/MediaFormat;"), &[])?
                    .l()?;
                let tk = env
                    .call_method(&self.muxer, jni_str!("addTrack"), jni_sig!("(Landroid/media/MediaFormat;)I"), &[(&new_fmt).into()])?
                    .i()?;
                self.track_idx = Some(tk);
                env.call_method(&self.muxer, jni_str!("start"), jni_sig!("()V"), &[])?;
                self.muxer_started = true;
                continue;
            }
            if idx < 0 {
                continue;
            }

            let buf = env
                .call_method(&self.codec, jni_str!("getOutputBuffer"), jni_sig!("(I)Ljava/nio/ByteBuffer;"), &[idx.into()])?
                .l()?;

            let flags = env.get_field(&info, jni_str!("flags"), jni_str!("I"))?.i()?;
            if flags & BUFFER_FLAG_CODEC_CONFIG == 0 && self.muxer_started {
                if let Some(tk) = self.track_idx {
                    env.call_method(
                        &self.muxer,
                        jni_str!("writeSampleData"),
                        jni_sig!("(ILjava/nio/ByteBuffer;Landroid/media/MediaCodec$BufferInfo;)V"),
                        &[tk.into(), (&buf).into(), (&info).into()],
                    )?;
                }
            }

            env.call_method(&self.codec, jni_str!("releaseOutputBuffer"), jni_sig!("(IZ)V"), &[idx.into(), false.into()])?;

            if flags & BUFFER_FLAG_END_OF_STREAM != 0 {
                return Ok(());
            }
        }
    }
}

impl EncoderBackend for AndroidEncoder {
    fn encode_rgba(&mut self, frame: &[u8]) -> Result<()> {
        self.rgba_to_nv12(frame);
        let yuv_len = self.yuv.len() as jint;
        let pts_us = ((self.frame_idx as f64) * 1_000_000.0 / self.fps as f64).round() as i64;
        let yuv_ptr = self.yuv.as_ptr();
        let yuv_size = self.yuv.len();

        JavaVM::singleton()?.attach_current_thread(|env| -> jni::errors::Result<()> {
            let in_idx = env
                .call_method(&self.codec, jni_str!("dequeueInputBuffer"), jni_sig!("(J)I"), &[(TIMEOUT_US * 5).into()])?
                .i()?;
            if in_idx >= 0 {
                let buf = env
                    .call_method(&self.codec, jni_str!("getInputBuffer"), jni_sig!("(I)Ljava/nio/ByteBuffer;"), &[in_idx.into()])?
                    .l()?;
                env.call_method(&buf, jni_str!("clear"), jni_sig!("()Ljava/nio/Buffer;"), &[])?;
                // Copy YUV bytes into the direct buffer.
                unsafe {
                    let raw_env = env.get_native_interface();
                    if let Some(get_dba) = (**raw_env).GetDirectBufferAddress {
                        let addr = get_dba(raw_env, buf.as_raw());
                        if !addr.is_null() {
                            std::ptr::copy_nonoverlapping(yuv_ptr, addr as *mut u8, yuv_size);
                        }
                    }
                }
                env.call_method(
                    &self.codec,
                    jni_str!("queueInputBuffer"),
                    jni_sig!("(IIIJI)V"),
                    &[in_idx.into(), 0i32.into(), yuv_len.into(), pts_us.into(), 0i32.into()],
                )?;
            }
            self.drain_output(env, false)
        })?;
        self.frame_idx += 1;
        Ok(())
    }

    fn finish(self: Box<Self>, _output: &Path) -> Result<PathBuf> {
        let mut me = *self;
        JavaVM::singleton()?.attach_current_thread(|env| -> jni::errors::Result<()> {
            let in_idx = env
                .call_method(&me.codec, jni_str!("dequeueInputBuffer"), jni_sig!("(J)I"), &[(TIMEOUT_US * 50).into()])?
                .i()?;
            if in_idx >= 0 {
                env.call_method(
                    &me.codec,
                    jni_str!("queueInputBuffer"),
                    jni_sig!("(IIIJI)V"),
                    &[in_idx.into(), 0i32.into(), 0i32.into(), 0i64.into(), BUFFER_FLAG_END_OF_STREAM.into()],
                )?;
            }
            me.drain_output(env, true)?;

            env.call_method(&me.codec, jni_str!("stop"), jni_sig!("()V"), &[])?;
            env.call_method(&me.codec, jni_str!("release"), jni_sig!("()V"), &[])?;
            if me.muxer_started {
                env.call_method(&me.muxer, jni_str!("stop"), jni_sig!("()V"), &[])?;
            }
            env.call_method(&me.muxer, jni_str!("release"), jni_sig!("()V"), &[])?;
            Ok(())
        })?;
        Ok(me.output)
    }
}
