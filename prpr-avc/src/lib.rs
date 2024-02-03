mod ffi;

mod common;

use std::{
    io::Write,
    ptr::{null, null_mut},
    usize,
};

pub use common::*;

mod avformat;
pub use avformat::*;

mod codec;
pub use codec::*;

mod frame;
pub use frame::*;

mod packet;
pub use packet::*;

mod stream;
pub use stream::*;

mod sws;
pub use sws::*;

mod video;
pub use video::*;

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

fn handle(code: ::std::os::raw::c_int) -> AVResult<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(AVError::new(code))
    }
}

extern "C" fn write_packet(
    buffer_vec: *mut ::std::os::raw::c_void,
    buf: *mut ::std::os::raw::c_uchar,
    buf_size: ::std::os::raw::c_int,
) -> ::std::os::raw::c_int {
    unsafe {
        // println!("write_packet");
        let buffer = std::slice::from_raw_parts(buf, buf_size as _);
        let vec_ref: &mut Vec<u8> = &mut *(buffer_vec as *mut Vec<u8>);
        vec_ref.write_all(buffer).unwrap();
        buf_size
    }
}

/// This Function is used to split audio and video.
/// The result is Ok((audio_buffer, video_buffer)).
pub fn demuxer(file_stream: Vec<u8>) -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
    unsafe {
        let mut in_fmt_ctx = null_mut();
        let mut tmp = tempfile::NamedTempFile::new()?;
        tmp.write_all(&file_stream)?;
        drop(file_stream);
        let url = std::ffi::CString::new(tmp.path().to_str().unwrap()).unwrap();
        handle(ffi::avformat_open_input(&mut in_fmt_ctx, url.as_ptr(), null_mut(), null_mut()))?;
        match in_fmt_ctx.is_null() {
            true => return Err(anyhow::anyhow!("Failed to alloc format context")),
            false => {}
        }
        handle(ffi::avformat_find_stream_info(in_fmt_ctx, null_mut()))?;
        let streams = std::slice::from_raw_parts((*in_fmt_ctx).streams, (*in_fmt_ctx).nb_streams as usize);
        let mut video_stream_index: i32 = -1;
        let mut video_format = "";
        let mut audio_stream_index: i32 = -1;
        let mut audio_format = "";
        let mut i = 0;
        while i < streams.len() {
            if (*(*streams[i]).codecpar).codec_type == 0 {
                video_stream_index = i as _;
                let decoder = ffi::avcodec_find_decoder((*(*streams[i]).codecpar).codec_id);
                match decoder.is_null() {
                    true => return Err(anyhow::anyhow!("Failed to get stream decoder")),
                    false => video_format = std::ffi::CStr::from_ptr((*decoder).name).to_str().unwrap(),
                }
            } else if (*(*streams[i]).codecpar).codec_type == 1 {
                audio_stream_index = i as _;
                let decoder = ffi::avcodec_find_decoder((*(*streams[i]).codecpar).codec_id);
                match decoder.is_null() {
                    true => return Err(anyhow::anyhow!("Failed to get stream decoder")),
                    false => {
                        audio_format = match std::ffi::CStr::from_ptr((*decoder).name).to_str().unwrap() {
                            "aac" => "adts",
                            x => x,
                        }
                    }
                }
            }
            i += 1;
        }
        if video_stream_index == -1 && audio_stream_index == -1 {
            return Err(anyhow::anyhow!("Failed to get stream index"));
        }
        let mut out_video_fmt_ctx = null_mut();
        let o_v_file_type = std::ffi::CString::new(video_format).unwrap();
        ffi::avformat_alloc_output_context2(&mut out_video_fmt_ctx, null(), o_v_file_type.as_ptr(), null());
        match out_video_fmt_ctx.is_null() {
            true => return Err(anyhow::anyhow!("Failed to alloc format context")),
            false => {}
        }
        let out_video_stream = ffi::avformat_new_stream(out_video_fmt_ctx, null_mut());
        match out_video_stream.is_null() {
            true => return Err(anyhow::anyhow!("Failed to alloc format context")),
            false => {}
        }
        handle(ffi::avcodec_parameters_copy((*out_video_stream).codecpar, (*streams[video_stream_index as usize]).codecpar))?;
        (*(*out_video_stream).codecpar).codec_tag = 0;
        let buffer_video = ffi::av_malloc(32768);
        let mut out_video_buffer: Vec<u8> = vec![];
        let out_video_avio_ctx = ffi::avio_alloc_context(
            buffer_video as _,
            32768,
            1,
            &mut out_video_buffer as *mut Vec<u8> as *mut ::std::os::raw::c_void,
            None,
            Some(write_packet),
            None,
        );
        (*out_video_fmt_ctx).pb = out_video_avio_ctx;
        let mut out_audio_fmt_ctx = null_mut();
        let o_a_file_type = std::ffi::CString::new(audio_format).unwrap();
        ffi::avformat_alloc_output_context2(&mut out_audio_fmt_ctx, null(), o_a_file_type.as_ptr(), null());
        match out_audio_fmt_ctx.is_null() {
            true => return Err(anyhow::anyhow!("Failed to alloc format context")),
            false => {}
        }
        let out_audio_stream = ffi::avformat_new_stream(out_audio_fmt_ctx, null_mut());
        match out_audio_stream.is_null() {
            true => return Err(anyhow::anyhow!("Failed to alloc format context")),
            false => {}
        }
        handle(ffi::avcodec_parameters_copy((*out_audio_stream).codecpar, (*streams[audio_stream_index as usize]).codecpar))?;
        (*(*out_audio_stream).codecpar).codec_tag = 0;
        let buffer_audio = ffi::av_malloc(32768);
        let mut out_audio_buffer: Vec<u8> = vec![];
        let out_video_avio_ctx = ffi::avio_alloc_context(
            buffer_audio as _,
            32768,
            1,
            &mut out_audio_buffer as *mut Vec<u8> as *mut ::std::os::raw::c_void,
            None,
            Some(write_packet),
            None,
        );
        (*out_audio_fmt_ctx).pb = out_video_avio_ctx;
        ffi::avformat_write_header(out_video_fmt_ctx, &mut null_mut());
        ffi::avformat_write_header(out_audio_fmt_ctx, &mut null_mut());

        let packet = ffi::av_packet_alloc();
        while ffi::av_read_frame(in_fmt_ctx, packet) == 0 {
            if (*packet).stream_index == video_stream_index {
                (*packet).stream_index = 0;
                ffi::av_interleaved_write_frame(out_video_fmt_ctx, packet);
            } else if (*packet).stream_index == audio_stream_index {
                (*packet).stream_index = 0;
                ffi::av_interleaved_write_frame(out_audio_fmt_ctx, packet);
            }
            ffi::av_packet_unref(packet);
        }
        ffi::av_write_trailer(out_video_fmt_ctx);
        ffi::avformat_free_context(out_video_fmt_ctx);
        ffi::av_write_trailer(out_audio_fmt_ctx);
        ffi::avformat_free_context(out_audio_fmt_ctx);
        Ok((out_audio_buffer, out_video_buffer))
    }
}
