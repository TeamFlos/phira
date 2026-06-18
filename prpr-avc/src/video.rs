use crate::{
    AVCodecContext, AVFormatContext, AVFrame, AVPacket, AVPixelFormat, AVRational, AVStreamRef, Error, Result, SwsContext, VideoStreamFormat,
};
use std::{
    sync::{Arc, Condvar, Mutex},
    thread::JoinHandle,
};
use tracing::error;

pub struct Video {
    stream_format: VideoStreamFormat,
    video_stream: AVStreamRef,

    position: Arc<(Mutex<i64>, Condvar)>,
    frame: Arc<Mutex<(AVFrame, i64)>>,
    decode_thread: Option<JoinHandle<()>>,
}

impl Video {
    pub fn open(file: impl AsRef<str>, pix_fmt: AVPixelFormat) -> Result<Self> {
        let mut format_ctx = AVFormatContext::new()?;
        format_ctx.open_input(file.as_ref())?;
        format_ctx.find_stream_info()?;

        let video_stream = format_ctx.streams().into_iter().find(|it| it.is_video()).ok_or(Error::NoVideoStream)?;
        let time_base = video_stream.time_base().to_f64();

        let decoder = video_stream.find_decoder()?;
        let mut codec_ctx = AVCodecContext::new(decoder, video_stream.codec_params(), Some(pix_fmt))?;

        let stream_format = codec_ctx.video_stream_format();
        let out_format = VideoStreamFormat {
            pix_fmt,
            ..stream_format.clone()
        };

        let mut sws = SwsContext::new(stream_format.clone(), out_format.clone())?;
        let mut in_frame = AVFrame::new()?;
        let mut out_frame = AVFrame::new()?;
        out_frame.set_video_format(&out_format);
        out_frame.get_buffer()?;

        let mut buf_frame = AVFrame::new()?;
        buf_frame.set_video_format(&stream_format);
        buf_frame.get_buffer()?;

        let position = Arc::new((Mutex::new(0), Condvar::new()));
        let frame = Arc::new(Mutex::new((buf_frame, -1)));

        let video_index = video_stream.index();

        let decode_thread = std::thread::spawn({
            let position = position.clone();
            let frame = frame.clone();

            move || {
                let mut run = || -> Result<()> {
                    let mut packet = AVPacket::new()?;
                    let mut current_ts = -1;

                    let mut catching_up_to: Option<i64> = None;

                    loop {
                        let ts = {
                            let mut guard = position.0.lock().unwrap();
                            loop {
                                let ts = *guard;
                                if ts == i64::MAX {
                                    return Ok(());
                                }

                                let diff_sec = (current_ts - ts) as f64 * time_base;
                                if current_ts != -1 && diff_sec > 0.0 && diff_sec < 0.5 {
                                    guard = position.1.wait(guard).unwrap();
                                } else {
                                    break ts;
                                }
                            }
                        };

                        if (current_ts - ts) as f64 * time_base > 0.5 {
                            let is_already_catching_up = catching_up_to.is_some_and(|target| ((ts - target) as f64 * time_base).abs() < 0.5);

                            if !is_already_catching_up {
                                format_ctx.seek_frame(video_index, ts, crate::ffi::AVSEEK_FLAG_BACKWARD)?;
                                codec_ctx.flush_buffers();

                                catching_up_to = Some(ts);
                                current_ts = -1;
                                continue;
                            }
                        }

                        if !format_ctx.read_frame(&mut packet)? {
                            frame.lock().unwrap().1 = -1;
                            continue;
                        }

                        if packet.stream_index() != video_index {
                            continue;
                        }

                        codec_ctx.send_packet(&packet)?;
                        let mut sent = false;
                        while codec_ctx.receive_frame(&mut in_frame)? {
                            current_ts = in_frame.pts();

                            if catching_up_to.is_some_and(|target| current_ts >= target) {
                                catching_up_to = None;
                            }

                            if !sent && current_ts >= ts {
                                sws.scale(&in_frame, &mut out_frame);
                                let mut guard = frame.lock().unwrap();
                                std::mem::swap(&mut guard.0, &mut out_frame);
                                guard.1 = current_ts;
                                sent = true;
                            }
                        }
                    }
                };

                if let Err(e) = run() {
                    error!("decode error: {e:?}");
                    frame.lock().unwrap().1 = -1;
                }
            }
        });

        Ok(Self {
            stream_format,
            video_stream,

            frame,
            position,
            decode_thread: Some(decode_thread),
        })
    }

    pub fn stream_format(&self) -> VideoStreamFormat {
        self.stream_format.clone()
    }

    pub fn frame_rate(&self) -> AVRational {
        self.video_stream.frame_rate()
    }

    pub fn time_base(&self) -> AVRational {
        self.video_stream.time_base()
    }

    pub fn duration(&self) -> f64 {
        self.video_stream.duration() as f64 * self.time_base().to_f64()
    }

    pub fn elapsed_to_timestamp(&self, elapsed: f64) -> i64 {
        let time_base = self.time_base();
        (elapsed * time_base.den as f64 / time_base.num as f64).round() as i64
    }

    pub fn seek(&self, timestamp: i64) {
        let mut guard = self.position.0.lock().unwrap();
        *guard = timestamp;
        self.position.1.notify_one();
    }

    pub fn with_frame<R>(&self, mut f: impl FnMut(&AVFrame, i64) -> R) -> R {
        let guard = self.frame.lock().unwrap();
        f(&guard.0, guard.1)
    }
}

impl Drop for Video {
    fn drop(&mut self) {
        self.seek(i64::MAX);
        if let Some(handle) = self.decode_thread.take() {
            handle.join().unwrap();
        }
    }
}
