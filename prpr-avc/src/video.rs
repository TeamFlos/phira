use crate::{
    AVCodecContext, AVFormatContext, AVFrame, AVPacket, AVPixelFormat, AVRational, AVStreamRef, Error, Result, SwsContext, VideoStreamFormat,
};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    thread::JoinHandle,
};
use tracing::error;

pub struct Video {
    stream_format: VideoStreamFormat,
    video_stream: AVStreamRef,

    dropped: Arc<AtomicBool>,
    ended: AtomicBool,

    mutex: Arc<(Mutex<Option<Option<&'static AVFrame>>>, Condvar)>,
    decode_thread: Option<JoinHandle<()>>,
}

impl Video {
    pub fn open(file: impl AsRef<str>, pix_fmt: AVPixelFormat) -> Result<Self> {
        let mut format_ctx = AVFormatContext::new()?;
        format_ctx.open_input(file.as_ref())?;
        format_ctx.find_stream_info()?;

        let video_stream = format_ctx.streams().into_iter().find(|it| it.is_video()).ok_or(Error::NoVideoStream)?;

        let decoder = video_stream.find_decoder()?;
        let mut codec_ctx = AVCodecContext::new(decoder, video_stream.codec_params(), Some(pix_fmt))?;

        let out_format = VideoStreamFormat {
            pix_fmt,
            ..codec_ctx.video_stream_format()
        };

        let mutex = Arc::new((Mutex::new(None), Condvar::new()));

        let stream_format = codec_ctx.video_stream_format();

        let mut sws = SwsContext::new(stream_format.clone(), out_format.clone())?;
        let mut in_frame = AVFrame::new()?;
        let mut out_frame = AVFrame::new()?;
        out_frame.set_video_format(&out_format);
        out_frame.get_buffer()?;

        let dropped = Arc::new(AtomicBool::default());

        let decode_thread = std::thread::spawn({
            let mut packet = AVPacket::new()?;
            let video_index = video_stream.index();
            let mutex = Arc::clone(&mutex);
            let dropped = Arc::clone(&dropped);
            move || {
                let mut decode_main = {
                    let mutex = Arc::clone(&mutex);
                    move || -> Result<()> {
                        while !dropped.load(Ordering::Relaxed) && format_ctx.read_frame(&mut packet)? {
                            if packet.stream_index() != video_index {
                                continue;
                            }
                            codec_ctx.send_packet(&packet)?;

                            while codec_ctx.receive_frame(&mut in_frame)? {
                                sws.scale(&in_frame, &mut out_frame);
                                let mut frame = mutex.0.lock().unwrap();
                                *frame = Some(Some(unsafe { std::mem::transmute(&out_frame) }));
                                mutex.1.notify_one();
                                while frame.is_some() {
                                    if dropped.load(Ordering::Relaxed) {
                                        return Ok(());
                                    }
                                    frame = mutex.1.wait(frame).unwrap();
                                }
                            }
                        }
                        let mut frame = mutex.0.lock().unwrap();
                        *frame = Some(None);
                        mutex.1.notify_one();
                        Ok(())
                    }
                };
                if let Err(err) = decode_main() {
                    error!("decode failed: {err:?}");
                    let mut frame = mutex.0.lock().unwrap();
                    *frame = Some(None);
                    mutex.1.notify_one();
                }
            }
        });

        Ok(Self {
            stream_format,
            video_stream,

            dropped,
            ended: AtomicBool::default(),

            mutex,
            decode_thread: Some(decode_thread),
        })
    }

    pub fn stream_format(&self) -> VideoStreamFormat {
        self.stream_format.clone()
    }

    pub fn frame_rate(&self) -> AVRational {
        self.video_stream.frame_rate()
    }

    pub fn with_frame<R>(&self, f: impl FnOnce(&AVFrame) -> R) -> Option<R> {
        let mut frame = self.mutex.0.lock().unwrap();
        loop {
            let Some(data) = *frame else {
                frame = self.mutex.1.wait(frame).unwrap();
                continue;
            };
            let Some(data) = data else {
                self.ended.store(true, Ordering::SeqCst);
                return None;
            };
            let res = f(data);
            *frame = None;
            self.mutex.1.notify_one();
            break Some(res);
        }
    }
}

impl Drop for Video {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::Relaxed);
        {
            let _guard = self.mutex.0.lock().unwrap();
            self.mutex.1.notify_one();
        }
        if let Some(handle) = self.decode_thread.take() {
            handle.join().unwrap();
        }
    }
}
