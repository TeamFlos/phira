use crate::{ffi, handle, AVFrame, AVPacket, AVPixelFormat, Error, OwnedPtr, Result, VideoStreamFormat};
use std::{
    ptr::null_mut,
    sync::{
        atomic::{AtomicI32, Ordering},
        Mutex,
    },
};

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct AVCodecParamsRef(pub(crate) *const ffi::AVCodecParameters);
impl AVCodecParamsRef {
    pub fn codec_id(&self) -> ffi::AVCodecID {
        unsafe { (*self.0).codec_id }
    }

    pub fn channel_layout(&self) -> u64 {
        unsafe { (*self.0).channel_layout }
    }

    pub fn channels(&self) -> i32 {
        unsafe { (*self.0).channels }
    }

    pub fn sample_format(&self) -> ffi::AVSampleFormat {
        unsafe { (*self.0).format }
    }

    pub fn sample_rate(&self) -> i32 {
        unsafe { (*self.0).sample_rate }
    }
}

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct AVCodecRef(*const ffi::AVCodec);
impl AVCodecRef {
    pub fn find_decoder(id: ffi::AVCodecID) -> Result<Self> {
        unsafe {
            let ptr = ffi::avcodec_find_decoder(id);
            if ptr.is_null() {
                Err(Error::DecoderNotFound(id))
            } else {
                Ok(Self(ptr))
            }
        }
    }
}

static EXPECTED_PIX_FMT_EDIT: Mutex<()> = Mutex::new(());
static EXPECTED_PIX_FMT: AtomicI32 = AtomicI32::new(-1);

unsafe fn get_format(s: *mut ffi::AVCodecContext, fmt: *const ffi::AVPixelFormat) -> ffi::AVPixelFormat {
    let expected = EXPECTED_PIX_FMT.load(Ordering::SeqCst);
    for i in 0.. {
        let fmt = fmt.add(i).read();
        if fmt == -1 {
            break;
        }
        if fmt == expected {
            return fmt;
        }
    }
    ffi::avcodec_default_get_format(s, fmt)
}

#[repr(transparent)]
pub struct AVCodecContext(OwnedPtr<ffi::AVCodecContext>);
impl AVCodecContext {
    pub fn new(codec: AVCodecRef, par: AVCodecParamsRef, expected: Option<AVPixelFormat>) -> Result<Self> {
        unsafe {
            let mut ptr = OwnedPtr::new(ffi::avcodec_alloc_context3(codec.0)).ok_or(Error::AllocationFailed)?;
            handle(ffi::avcodec_parameters_to_context(ptr.0, par.0))?;
            let _guard = expected.map(|pix_fmt| {
                let guard = EXPECTED_PIX_FMT_EDIT.lock().unwrap();
                EXPECTED_PIX_FMT.store(pix_fmt.0, Ordering::SeqCst);
                ptr.as_mut().get_format = get_format as _;
                guard
            });
            handle(ffi::avcodec_open2(ptr.0, codec.0, null_mut()))?;
            Ok(Self(ptr))
        }
    }

    pub fn frame_size(&self) -> i32 {
        unsafe { self.0.as_ref().frame_size }
    }

    pub fn video_stream_format(&self) -> VideoStreamFormat {
        unsafe {
            let this = self.0.as_ref();
            VideoStreamFormat {
                width: this.width,
                height: this.height,
                pix_fmt: AVPixelFormat(this.pix_fmt),
            }
        }
    }

    pub fn send_packet(&mut self, packet: &AVPacket) -> Result<()> {
        unsafe { handle(ffi::avcodec_send_packet(self.0 .0, packet.0 .0)) }
    }

    pub fn receive_frame(&mut self, frame: &mut AVFrame) -> Result<bool> {
        unsafe {
            match handle(ffi::avcodec_receive_frame(self.0 .0, frame.0 .0)) {
                Err(Error::TryAgain) => return Ok(false),
                x => {
                    x?;
                    Ok(true)
                }
            }
        }
    }
}

impl Drop for AVCodecContext {
    fn drop(&mut self) {
        unsafe {
            ffi::avcodec_free_context(self.0.as_self_mut());
        }
    }
}

unsafe impl Send for AVCodecContext {}
