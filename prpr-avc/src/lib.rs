mod avformat;
mod codec;
mod error;
mod ffi;
mod frame;
mod misc;
mod packet;
mod stream;
mod swr;
mod sws;
mod video;

pub use avformat::*;
pub use codec::*;
pub use error::*;
pub use frame::*;
pub use misc::*;
pub use packet::*;
pub use stream::*;
pub use swr::*;
pub use sws::*;
pub use video::*;

use sasa::{AudioClip, Frame};

const AUDIO_DECODING_SAMPLE_RATE: i32 = 44100;

#[repr(transparent)]
struct OwnedPtr<T>(pub *mut T);
impl<T> OwnedPtr<T> {
    pub fn new(ptr: *mut T) -> Option<Self> {
        if ptr.is_null() {
            None
        } else {
            Some(Self(ptr))
        }
    }

    pub unsafe fn as_ref(&self) -> &T {
        std::mem::transmute(self.0)
    }

    pub unsafe fn as_mut(&mut self) -> &mut T {
        std::mem::transmute(self.0)
    }

    pub fn as_self_mut(&mut self) -> *mut *mut T {
        &mut self.0
    }
}

fn handle(code: ::std::os::raw::c_int) -> Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(Error::from_error_code(code))
    }
}

pub fn demux_audio(file: impl AsRef<str>) -> Result<Option<AudioClip>> {
    let mut format_ctx = AVFormatContext::new()?;
    format_ctx.open_input(file.as_ref())?;
    format_ctx.find_stream_info()?;

    let stream = match format_ctx.streams().into_iter().find(|it| it.is_audio()) {
        Some(stream) => stream,
        None => return Ok(None),
    };
    let decoder = stream.find_decoder()?;
    let mut codec_ctx = AVCodecContext::new(decoder, stream.codec_params(), None)?;

    let params = stream.codec_params();
    let in_format = AudioStreamFormat {
        channel_layout: params.channel_layout(),
        channels: params.channels(),
        sample_fmt: params.sample_format(),
        sample_rate: params.sample_rate(),
    };
    let out_format = AudioStreamFormat {
        channel_layout: ffi::AV_CH_LAYOUT_STEREO,
        channels: 2,
        sample_fmt: ffi::AV_SAMPLE_FMT_FLT,
        sample_rate: AUDIO_DECODING_SAMPLE_RATE,
    };
    let mut swr = SwrContext::new(&in_format, &out_format)?;
    swr.init()?;

    let mut in_frame = AVFrame::new()?;
    let mut frames = Vec::new();
    let mut packet = AVPacket::new()?;
    while format_ctx.read_frame(&mut packet)? {
        if packet.stream_index() == stream.index() {
            codec_ctx.send_packet(&packet)?;

            while codec_ctx.receive_frame(&mut in_frame)? {
                let end = frames.len();
                let out_samples = unsafe {
                    ffi::av_rescale_rnd(
                        swr.get_delay(in_format.sample_rate) + in_frame.number_of_samples() as i64,
                        AUDIO_DECODING_SAMPLE_RATE as _,
                        in_format.sample_rate as _,
                        ffi::AV_ROUND_UP,
                    )
                };

                frames.extend(std::iter::repeat_with(Frame::default).take(out_samples as usize));
                let out_samples = swr.convert(
                    in_frame.raw_data()[0],
                    in_frame.number_of_samples(),
                    unsafe { frames.as_mut_ptr().add(end) as *mut _ },
                    out_samples as _,
                )?;
                frames.truncate(end + out_samples);
            }
        }
    }

    Ok(Some(AudioClip::from_raw(frames, AUDIO_DECODING_SAMPLE_RATE as _)))
}
