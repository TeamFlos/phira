use crate::{ffi, Error, OwnedPtr, Result};

#[repr(transparent)]
pub struct AVPacket(pub(crate) OwnedPtr<ffi::AVPacket>);
impl AVPacket {
    pub fn new() -> Result<Self> {
        unsafe { OwnedPtr::new(ffi::av_packet_alloc()).map(Self).ok_or(Error::AllocationFailed) }
    }

    pub fn stream_index(&self) -> i32 {
        unsafe { self.0.as_ref().stream_index }
    }
}

unsafe impl Send for AVPacket {}

impl Drop for AVPacket {
    fn drop(&mut self) {
        unsafe { ffi::av_packet_free(self.0.as_self_mut()) }
    }
}
