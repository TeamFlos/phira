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
            this.channel_layout = format.channel_layout as u64;
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

    pub fn get_buffer(&mut self) -> Result<()> {
        unsafe { handle(ffi::av_frame_get_buffer(self.0 .0, 0)) }
    }

    pub fn raw_data(&self) -> [*mut u8; 8] {
        unsafe { self.0.as_ref().data }
    }

    // TODO: is this correct?
    pub fn data(&self, index: usize) -> &[u8] {
        unsafe {
            let this = self.0.as_ref();
            std::slice::from_raw_parts(this.data[index], this.linesize[index] as usize * this.height as usize)
        }
    }

    pub fn data_half(&self, index: usize) -> &[u8] {
        let data = self.data(index);
        &data[..data.len() / 2]
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
