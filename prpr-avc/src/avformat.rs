use crate::{ffi, handle, AVPacket, AVStreamRef, Error, OwnedPtr, Result};
use std::{ffi::CString, ptr::null_mut};

#[repr(transparent)]
pub struct AVFormatContext(OwnedPtr<ffi::AVFormatContext>);
impl AVFormatContext {
    pub fn new() -> Result<Self> {
        unsafe { OwnedPtr::new(ffi::avformat_alloc_context()).map(Self).ok_or(Error::AllocationFailed) }
    }

    pub fn open_input(&mut self, url: &str) -> Result<()> {
        unsafe {
            let url = CString::new(url).unwrap();
            handle(ffi::avformat_open_input(self.0.as_self_mut(), url.as_ptr(), null_mut(), null_mut()))
        }
    }

    pub fn find_stream_info(&mut self) -> Result<()> {
        unsafe { handle(ffi::avformat_find_stream_info(self.0 .0, null_mut())) }
    }

    pub fn streams(&self) -> Vec<AVStreamRef> {
        unsafe {
            let this = self.0.as_ref();
            std::slice::from_raw_parts(this.streams as *const AVStreamRef, this.nb_streams as _).to_vec()
        }
    }

    pub fn read_frame(&mut self, frame: &mut AVPacket) -> Result<bool> {
        unsafe {
            match handle(ffi::av_read_frame(self.0 .0, frame.0 .0)) {
                Err(Error::EndOfFile) => return Ok(false),
                x => {
                    x?;
                    Ok(true)
                }
            }
        }
    }
}

unsafe impl Send for AVFormatContext {}

impl Drop for AVFormatContext {
    fn drop(&mut self) {
        unsafe {
            ffi::avformat_free_context(self.0 .0);
        }
    }
}
