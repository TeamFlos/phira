use crate::ffi;
use std::ffi::CStr;
use thiserror::Error;

#[cfg(any(target_os = "ios", target_os = "macos"))]
const EAGAIN: i32 = 35;

#[cfg(not(any(target_os = "ios", target_os = "macos")))]
const EAGAIN: i32 = 11;

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to allocate")]
    AllocationFailed,

    #[error("no video stream found")]
    NoVideoStream,

    #[error("try again")]
    TryAgain,

    #[error("decoder not found for codec id {0}")]
    DecoderNotFound(ffi::AVCodecID),

    #[error("end of file")]
    EndOfFile,

    #[error("AVError #{0}: {1:?}")]
    Unhandled(i32, Option<String>),

    #[error("IO error: {0}")]
    IOError(#[from] std::io::Error),
}

impl Error {
    pub fn from_error_code(code: std::os::raw::c_int) -> Self {
        let code = code as i32;
        match -code {
            541478725 => return Self::EndOfFile,
            EAGAIN => return Self::TryAgain,
            _ => {}
        }

        let msg = unsafe {
            let mut buf = [0; ffi::AV_ERROR_MAX_STRING_SIZE as usize];
            if ffi::av_strerror(code, buf.as_mut_ptr(), ffi::AV_ERROR_MAX_STRING_SIZE as usize) == 0 {
                Some(CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned())
            } else {
                None
            }
        };
        Self::Unhandled(code, msg)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
