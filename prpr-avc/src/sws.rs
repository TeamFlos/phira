use crate::{ffi, AVFrame, Error, OwnedPtr, Result, VideoStreamFormat};
use std::{
    mem::transmute,
    ptr::{null, null_mut},
};

#[repr(transparent)]
pub struct SwsContext(OwnedPtr<ffi::SwsContext>);
impl SwsContext {
    pub fn new(src: VideoStreamFormat, dst: VideoStreamFormat) -> Result<Self> {
        unsafe {
            OwnedPtr::new(ffi::sws_getContext(
                src.width,
                src.height,
                src.pix_fmt.0 as _,
                dst.width,
                dst.height,
                dst.pix_fmt.0 as _,
                ffi::SWS_BICUBIC as _,
                null_mut(),
                null_mut(),
                null(),
            ))
            .map(Self)
            .ok_or(Error::AllocationFailed)
        }
    }

    pub fn scale(&mut self, src: &AVFrame, dst: &mut AVFrame) {
        unsafe {
            let src = src.0.as_ref();
            let dst = dst.0.as_mut();
            ffi::sws_scale(
                self.0 .0,
                transmute(src.data.as_ptr()),
                src.linesize.as_ptr(),
                0,
                src.height,
                transmute(dst.data.as_ptr()),
                dst.linesize.as_ptr(),
            );
        }
    }
}

unsafe impl Send for SwsContext {}
