use crate::ffi;
use std::fmt::Display;

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
pub struct VideoStreamFormat {
    pub width: i32,
    pub height: i32,
    pub pix_fmt: AVPixelFormat,
}

#[derive(Debug, Clone)]
pub struct AudioStreamFormat {
    pub channel_layout: u64,
    pub channels: i32,
    pub sample_fmt: ffi::AVSampleFormat,
    pub sample_rate: i32,
}
