use std::{mem, slice};

use crate::{ffi, handle, Error, OwnedPtr, Result, VideoStreamFormat};

#[repr(transparent)]
pub struct AVFrame(pub(crate) OwnedPtr<ffi::AVFrame>);
impl AVFrame {
    pub fn new() -> Result<Self> {
        unsafe { Ok(Self(OwnedPtr::new(ffi::av_frame_alloc()).ok_or(Error::AllocationFailed)?)) }
    }

    pub fn set_video_format(&mut self, format: &VideoStreamFormat) {
        unsafe {
            let this = self.0.as_mut();
            this.width = format.width;
            this.height = format.height;
            this.format = format.pix_fmt.0;
        }
    }

    pub fn set_audio_format(&mut self, format: &crate::AudioStreamFormat) {
        unsafe {
            let this = self.0.as_mut();
            this.channels = format.channels;
            this.channel_layout = format.channel_layout;
            this.format = format.sample_fmt;
            this.sample_rate = format.sample_rate;
        }
    }

    pub fn number_of_samples(&self) -> i32 {
        unsafe { self.0.as_ref().nb_samples }
    }

    pub fn set_number_of_samples(&mut self, samples: i32) {
        unsafe {
            self.0.as_mut().nb_samples = samples;
        }
    }

    pub fn pts(&self) -> i64 {
        unsafe { self.0.as_ref().pts }
    }

    pub fn get_buffer(&mut self) -> Result<()> {
        unsafe { handle(ffi::av_frame_get_buffer(self.0 .0, 0)) }
    }

    pub fn raw_data(&self) -> &[*const u8; 8] {
        unsafe { mem::transmute::<&[*mut u8; 8], &[*const u8; 8]>(&self.0.as_ref().data) }
    }
    pub fn raw_data_mut(&mut self) -> &mut [*mut u8; 8] {
        unsafe { &mut self.0.as_mut().data }
    }

    pub fn get_data(&self, index: usize, dest: &mut Vec<u8>) {
        unsafe {
            let this = self.0.as_ref();

            let linesize = this.linesize[index] as usize;
            let src_ptr = this.data[index];

            let w = this.width as usize;
            let h = this.height as usize;

            dest.clear();
            dest.reserve(w * h);

            let slice = slice::from_raw_parts(src_ptr, linesize * h);
            for row in slice.chunks_exact(linesize) {
                dest.extend_from_slice(&row[..w]);
            }
        }
    }

    pub fn get_data_half(&self, index: usize, dest: &mut Vec<u8>) {
        unsafe {
            let this = self.0.as_ref();

            let linesize = this.linesize[index] as usize;
            let src_ptr = this.data[index];

            let w = (this.width as usize).div_ceil(2);
            let h = (this.height as usize).div_ceil(2);

            dest.clear();
            dest.reserve(w * h);

            let slice = slice::from_raw_parts(src_ptr, linesize * h);
            for row in slice.chunks_exact(linesize) {
                dest.extend_from_slice(&row[..w]);
            }
        }
    }

    pub fn line_size(&self) -> i32 {
        unsafe { self.0.as_ref().linesize[0] }
    }
}

impl Drop for AVFrame {
    fn drop(&mut self) {
        unsafe {
            ffi::av_frame_free(self.0.as_self_mut());
        }
    }
}

unsafe impl Send for AVFrame {}
unsafe impl Sync for AVFrame {}
