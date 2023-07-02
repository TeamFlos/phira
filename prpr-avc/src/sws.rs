use crate::{ffi, AVFrame, OwnedPtr, StreamFormat};
use anyhow::{Context, Result};
use std::{
    mem::transmute,
    ptr::{null, null_mut},
};

pub struct SwsContext(OwnedPtr<ffi::SwsContext>);
impl SwsContext {
    pub fn new(src: StreamFormat, dst: StreamFormat) -> Result<Self> {
        unsafe {
            Ok(Self(
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
                .context("failed to create sws context")?,
            ))
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
