use crate::ffi;
use std::{ffi::CStr, fmt::Display};

#[derive(Debug)]
pub struct AVError {
    pub code: i32,
    pub msg: Option<String>,
}

impl AVError {
    pub fn new(code: ::std::os::raw::c_int) -> Self {
        let code = code as i32;
        let msg = unsafe {
            let mut buf = [0; ffi::AV_ERROR_MAX_STRING_SIZE as usize];
            if ffi::av_strerror(
                code,
                buf.as_mut_ptr(),
                ffi::AV_ERROR_MAX_STRING_SIZE as usize,
            ) == 0
            {
                Some(CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned())
            } else {
                None
            }
        };
        Self { code, msg }
    }
}

impl Display for AVError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "AVError ({:x})", self.code)?;
        if let Some(msg) = &self.msg {
            write!(f, ": {msg}")?;
        }
        Ok(())
    }
}
impl std::error::Error for AVError {}

pub type AVResult<T> = Result<T, AVError>;

#[derive(Debug)]
pub struct AVRational {
    pub num: i32,
    pub den: i32,
}

impl AVRational {
    pub fn to_f64(&self) -> f64 {
        self.num as f64 / self.den as f64
    }

    pub fn to_f64_inv(&self) -> f64 {
        self.den as f64 / self.num as f64
    }
}

impl From<ffi::AVRational> for AVRational {
    fn from(value: ffi::AVRational) -> Self {
        Self {
            num: value.num as i32,
            den: value.den as i32,
        }
    }
}

impl Display for AVRational {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.num, self.den)
    }
}

#[repr(transparent)]
#[derive(Debug, Clone, Copy)]
pub struct AVPixelFormat(pub ffi::AVPixelFormat);
impl AVPixelFormat {
    pub const YUV420P: AVPixelFormat = AVPixelFormat(0);
    pub const RGB24: AVPixelFormat = AVPixelFormat(2);
}

#[derive(Debug, Clone)]
pub struct StreamFormat {
    pub width: i32,
    pub height: i32,
    pub pix_fmt: AVPixelFormat,
}
