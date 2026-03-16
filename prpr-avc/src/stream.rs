use crate::{ffi, AVCodecParamsRef, AVCodecRef, AVRational, Result};

#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct AVStreamRef(*const ffi::AVStream);
impl AVStreamRef {
    pub fn index(&self) -> i32 {
        #[allow(clippy::unnecessary_cast)]
        unsafe {
            (*self.0).index as i32
        }
    }

    pub fn frame_rate(&self) -> AVRational {
        unsafe { (*self.0).r_frame_rate.into() }
    }

    pub fn codec_type(&self) -> ffi::AVMediaType {
        unsafe { (*(*self.0).codecpar).codec_type }
    }

    pub fn is_video(&self) -> bool {
        self.codec_type() == 0
    }

    pub fn is_audio(&self) -> bool {
        self.codec_type() == 1
    }

    pub fn codec_params(&self) -> AVCodecParamsRef {
        AVCodecParamsRef(unsafe { (*self.0).codecpar })
    }

    pub fn find_decoder(&self) -> Result<AVCodecRef> {
        AVCodecRef::find_decoder(self.codec_params().codec_id())
    }
}
