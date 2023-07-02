use crate::{ffi, OwnedPtr};
use anyhow::{Context, Result};

#[repr(transparent)]
pub struct AVPacket(pub(crate) OwnedPtr<ffi::AVPacket>);
impl AVPacket {
    pub fn new() -> Result<Self> {
        unsafe {
            Ok(Self(
                OwnedPtr::new(ffi::av_packet_alloc()).context("failed to allocate packet")?,
            ))
        }
    }

    pub fn stream_index(&self) -> i32 {
        unsafe { self.0.as_ref().stream_index }
    }
}

unsafe impl Send for AVPacket {}
