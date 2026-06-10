#![allow(clippy::type_complexity)]

use std::ffi::c_void;

pub const AV_CH_LAYOUT_STEREO: u64 = 3;
pub const AV_CHANNEL_ORDER_NATIVE: AVChannelOrder = 1;
pub const AV_CHANNEL_LAYOUT_STEREO: AVChannelLayout = AVChannelLayout {
    order: AV_CHANNEL_ORDER_NATIVE,
    nb_channels: 2,
    u: AVChannelLayout__bindgen_ty_1 { mask: AV_CH_LAYOUT_STEREO },
    opaque: std::ptr::null_mut(),
};

pub const AV_SAMPLE_FMT_FLT: AVSampleFormat = 3;

pub const AV_ROUND_UP: AVRounding = 0;

pub const AVSEEK_FLAG_BACKWARD: i32 = 1;

#[link(name = "avformat", kind = "static")]
extern "C" {
    pub fn avformat_alloc_context() -> *mut AVFormatContext;
    pub fn avformat_free_context(s: *mut AVFormatContext);
    pub fn avformat_open_input(
        ps: *mut *mut AVFormatContext,
        url: *const ::std::os::raw::c_char,
        fmt: *const c_void,
        options: *mut *mut c_void,
    ) -> ::std::os::raw::c_int;
    pub fn avformat_find_stream_info(ic: *mut AVFormatContext, options: *mut *mut c_void) -> ::std::os::raw::c_int;
    pub fn av_read_frame(s: *mut AVFormatContext, pkt: *mut AVPacket) -> ::std::os::raw::c_int;
    pub fn av_seek_frame(
        s: *mut AVFormatContext,
        stream_index: ::std::os::raw::c_int,
        timestamp: i64,
        flags: ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int;
}

#[link(name = "avutil", kind = "static")]
extern "C" {
    pub fn av_strerror(errnum: ::std::os::raw::c_int, errbuf: *mut ::std::os::raw::c_char, errbuf_size: usize) -> ::std::os::raw::c_int;
    pub fn av_frame_alloc() -> *mut AVFrame;
    pub fn av_frame_free(frame: *mut *mut AVFrame);
    pub fn av_frame_get_buffer(frame: *mut AVFrame, align: ::std::os::raw::c_int) -> ::std::os::raw::c_int;
    pub fn av_rescale_rnd(a: i64, b: i64, c: i64, r: AVRounding) -> i64;
}

#[link(name = "avcodec", kind = "static")]
extern "C" {
    pub fn avcodec_find_decoder(id: AVCodecID) -> *mut AVCodec;
    #[cfg(target_env = "ohos")]
    pub fn avcodec_find_decoder_by_name(name: *const ::std::os::raw::c_char) -> *mut AVCodec;
    pub fn avcodec_alloc_context3(codec: *const AVCodec) -> *mut AVCodecContext;
    pub fn avcodec_free_context(avctx: *mut *mut AVCodecContext);
    pub fn avcodec_parameters_to_context(codec: *mut AVCodecContext, par: *const AVCodecParameters) -> ::std::os::raw::c_int;
    pub fn avcodec_open2(avctx: *mut AVCodecContext, codec: *const AVCodec, options: *mut *mut c_void) -> ::std::os::raw::c_int;
    pub fn av_packet_alloc() -> *mut AVPacket;
    pub fn av_packet_free(pkt: *mut *mut AVPacket);
    pub fn avcodec_send_packet(avctx: *mut AVCodecContext, avpkt: *const AVPacket) -> ::std::os::raw::c_int;
    pub fn avcodec_receive_frame(avctx: *mut AVCodecContext, frame: *mut AVFrame) -> ::std::os::raw::c_int;
    pub fn avcodec_default_get_format(s: *mut AVCodecContext, fmt: *const AVPixelFormat) -> AVPixelFormat;
    pub fn avcodec_flush_buffers(avctx: *mut AVCodecContext);
}

#[link(name = "swscale", kind = "static")]
extern "C" {
    pub fn sws_getContext(
        srcW: ::std::os::raw::c_int,
        srcH: ::std::os::raw::c_int,
        srcFormat: AVPixelFormat,
        dstW: ::std::os::raw::c_int,
        dstH: ::std::os::raw::c_int,
        dstFormat: AVPixelFormat,
        flags: ::std::os::raw::c_int,
        srcFilter: *mut c_void,
        dstFilter: *mut c_void,
        param: *const f64,
    ) -> *mut SwsContext;
    pub fn sws_scale(
        c: *mut SwsContext,
        srcSlice: *const *const u8,
        srcStride: *const ::std::os::raw::c_int,
        srcSliceY: ::std::os::raw::c_int,
        srcSliceH: ::std::os::raw::c_int,
        dst: *const *mut u8,
        dstStride: *const ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int;
}

#[link(name = "swresample", kind = "static")]
extern "C" {
    pub fn swr_alloc_set_opts2(
        ps: *mut *mut SwrContext,
        out_ch_layout: *const AVChannelLayout,
        out_sample_fmt: AVSampleFormat,
        out_sample_rate: ::std::os::raw::c_int,
        in_ch_layout: *const AVChannelLayout,
        in_sample_fmt: AVSampleFormat,
        in_sample_rate: ::std::os::raw::c_int,
        log_offset: ::std::os::raw::c_int,
        log_ctx: *mut ::std::os::raw::c_void,
    ) -> ::std::os::raw::c_int;
    pub fn swr_init(s: *mut SwrContext) -> ::std::os::raw::c_int;
    pub fn swr_get_delay(s: *const SwrContext, base: ::std::os::raw::c_int) -> i64;
    pub fn swr_convert(
        s: *mut SwrContext,
        out: *mut *mut u8,
        out_count: ::std::os::raw::c_int,
        in_: *const *const u8,
        in_count: ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int;
    pub fn swr_free(s: *mut *mut SwrContext);
}

pub type AVChannelOrder = ::std::os::raw::c_uint;
pub type AVChromaLocation = ::std::os::raw::c_uint;
pub type AVCodecID = ::std::os::raw::c_uint;
pub type AVColorPrimaries = ::std::os::raw::c_uint;
pub type AVColorRange = ::std::os::raw::c_uint;
pub type AVColorSpace = ::std::os::raw::c_uint;
pub type AVColorTransferCharacteristic = ::std::os::raw::c_uint;
pub type AVDiscard = ::std::os::raw::c_int;
pub type AVDurationEstimationMethod = ::std::os::raw::c_uint;
pub type AVFieldOrder = ::std::os::raw::c_uint;
pub type AVMediaType = ::std::os::raw::c_int;
pub type AVPictureType = ::std::os::raw::c_uint;
pub type AVPixelFormat = ::std::os::raw::c_int;
pub type AVRounding = ::std::os::raw::c_uint;
pub type AVSampleFormat = ::std::os::raw::c_int;
pub type SwsContext = c_void;
pub type SwrContext = c_void;
pub type AVIODataMarkerType = ::std::os::raw::c_uint;
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct AVCodec {
    _unused: [u8; 0],
}
pub const AV_ERROR_MAX_STRING_SIZE: u32 = 64;
pub const SWS_BICUBIC: u32 = 4;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct AVRational {
    pub num: ::std::os::raw::c_int,
    pub den: ::std::os::raw::c_int,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct AVIOInterruptCB {
    pub callback: ::std::option::Option<unsafe extern "C" fn(arg1: *mut ::std::os::raw::c_void) -> ::std::os::raw::c_int>,
    pub opaque: *mut ::std::os::raw::c_void,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct AVBuffer {
    _unused: [u8; 0],
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct AVBufferRef {
    pub buffer: *mut AVBuffer,
    pub data: *mut u8,
    pub size: ::std::os::raw::c_int,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct AVPacket {
    pub buf: *mut AVBufferRef,
    pub pts: i64,
    pub dts: i64,
    pub data: *mut u8,
    pub size: ::std::os::raw::c_int,
    pub stream_index: ::std::os::raw::c_int,
    pub flags: ::std::os::raw::c_int,
    pub side_data: *mut c_void,
    pub side_data_elems: ::std::os::raw::c_int,
    pub duration: i64,
    pub pos: i64,
    pub opaque: *mut ::std::os::raw::c_void,
    pub opaque_ref: *mut AVBufferRef,
    pub time_base: AVRational,
}
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct AVIOContext {
    #[doc = " A class for private options.\n\n If this AVIOContext is created by avio_open2(), av_class is set and\n passes the options down to protocols.\n\n If this AVIOContext is manually allocated, then av_class may be set by\n the caller.\n\n warning -- this field can be NULL, be sure to not pass this AVIOContext\n to any av_opt_* functions in that case."]
    pub av_class: *const ::std::os::raw::c_void,
    #[doc = "< Start of the buffer."]
    pub buffer: *mut ::std::os::raw::c_uchar,
    #[doc = "< Maximum buffer size"]
    pub buffer_size: ::std::os::raw::c_int,
    #[doc = "< Current position in the buffer"]
    pub buf_ptr: *mut ::std::os::raw::c_uchar,
    #[doc = "< End of the data, may be less than\nbuffer+buffer_size if the read function returned\nless data than requested, e.g. for streams where\nno more data has been received yet."]
    pub buf_end: *mut ::std::os::raw::c_uchar,
    #[doc = "< A private pointer, passed to the read/write/seek/...\nfunctions."]
    pub opaque: *mut ::std::os::raw::c_void,
    pub read_packet: ::std::option::Option<
        unsafe extern "C" fn(opaque: *mut ::std::os::raw::c_void, buf: *mut u8, buf_size: ::std::os::raw::c_int) -> ::std::os::raw::c_int,
    >,
    pub write_packet: ::std::option::Option<
        unsafe extern "C" fn(opaque: *mut ::std::os::raw::c_void, buf: *mut u8, buf_size: ::std::os::raw::c_int) -> ::std::os::raw::c_int,
    >,
    pub seek: ::std::option::Option<unsafe extern "C" fn(opaque: *mut ::std::os::raw::c_void, offset: i64, whence: ::std::os::raw::c_int) -> i64>,
    pub pos: i64,
    pub eof_reached: ::std::os::raw::c_int,
    pub error: ::std::os::raw::c_int,
    pub write_flag: ::std::os::raw::c_int,
    pub max_packet_size: ::std::os::raw::c_int,
    pub min_packet_size: ::std::os::raw::c_int,
    pub checksum: ::std::os::raw::c_ulong,
    pub checksum_ptr: *mut ::std::os::raw::c_uchar,
    pub update_checksum: ::std::option::Option<
        unsafe extern "C" fn(checksum: ::std::os::raw::c_ulong, buf: *const u8, size: ::std::os::raw::c_uint) -> ::std::os::raw::c_ulong,
    >,
    pub read_pause:
        ::std::option::Option<unsafe extern "C" fn(opaque: *mut ::std::os::raw::c_void, pause: ::std::os::raw::c_int) -> ::std::os::raw::c_int>,
    pub read_seek: ::std::option::Option<
        unsafe extern "C" fn(
            opaque: *mut ::std::os::raw::c_void,
            stream_index: ::std::os::raw::c_int,
            timestamp: i64,
            flags: ::std::os::raw::c_int,
        ) -> i64,
    >,
    pub seekable: ::std::os::raw::c_int,
    pub direct: ::std::os::raw::c_int,
    pub protocol_whitelist: *const ::std::os::raw::c_char,
    pub protocol_blacklist: *const ::std::os::raw::c_char,
    pub write_data_type: ::std::option::Option<
        unsafe extern "C" fn(
            opaque: *mut ::std::os::raw::c_void,
            buf: *mut u8,
            buf_size: ::std::os::raw::c_int,
            type_: AVIODataMarkerType,
            time: i64,
        ) -> ::std::os::raw::c_int,
    >,
    #[doc = " If set, don't call write_data_type separately for AVIO_DATA_MARKER_BOUNDARY_POINT,\n but ignore them and treat them as AVIO_DATA_MARKER_UNKNOWN (to avoid needlessly\n small chunks of data returned from the callback)."]
    pub ignore_boundary_point: ::std::os::raw::c_int,
    #[doc = " Maximum reached position before a backward seek in the write buffer,\n used keeping track of already written data for a later flush."]
    pub buf_ptr_max: *mut ::std::os::raw::c_uchar,
    #[doc = " Read-only statistic of bytes read for this AVIOContext."]
    pub bytes_read: i64,
    #[doc = " Read-only statistic of bytes written for this AVIOContext."]
    pub bytes_written: i64,
}
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct AVFormatContext {
    pub av_class: *const c_void,
    pub iformat: *const c_void,
    pub oformat: *const c_void,
    pub priv_data: *mut ::std::os::raw::c_void,
    pub pb: *mut AVIOContext,
    pub ctx_flags: ::std::os::raw::c_int,
    pub nb_streams: ::std::os::raw::c_uint,
    pub streams: *mut *mut AVStream,
    pub url: *mut ::std::os::raw::c_char,
    pub start_time: i64,
    pub duration: i64,
    pub bit_rate: i64,
    pub packet_size: ::std::os::raw::c_uint,
    pub max_delay: ::std::os::raw::c_int,
    pub flags: ::std::os::raw::c_int,
    pub probesize: i64,
    pub max_analyze_duration: i64,
    pub key: *const u8,
    pub keylen: ::std::os::raw::c_int,
    pub nb_programs: ::std::os::raw::c_uint,
    pub programs: *mut *mut c_void,
    pub video_codec_id: AVCodecID,
    pub audio_codec_id: AVCodecID,
    pub subtitle_codec_id: AVCodecID,
    pub max_index_size: ::std::os::raw::c_uint,
    pub max_picture_buffer: ::std::os::raw::c_uint,
    pub nb_chapters: ::std::os::raw::c_uint,
    pub chapters: *mut *mut c_void,
    pub metadata: *mut c_void,
    pub start_time_realtime: i64,
    pub fps_probe_size: ::std::os::raw::c_int,
    pub error_recognition: ::std::os::raw::c_int,
    pub interrupt_callback: AVIOInterruptCB,
    pub debug: ::std::os::raw::c_int,
    pub max_interleave_delta: i64,
    pub strict_std_compliance: ::std::os::raw::c_int,
    pub event_flags: ::std::os::raw::c_int,
    pub max_ts_probe: ::std::os::raw::c_int,
    pub avoid_negative_ts: ::std::os::raw::c_int,
    pub ts_id: ::std::os::raw::c_int,
    pub audio_preload: ::std::os::raw::c_int,
    pub max_chunk_duration: ::std::os::raw::c_int,
    pub max_chunk_size: ::std::os::raw::c_int,
    pub use_wallclock_as_timestamps: ::std::os::raw::c_int,
    pub avio_flags: ::std::os::raw::c_int,
    pub duration_estimation_method: AVDurationEstimationMethod,
    pub skip_initial_bytes: i64,
    pub correct_ts_overflow: ::std::os::raw::c_uint,
    pub seek2any: ::std::os::raw::c_int,
    pub flush_packets: ::std::os::raw::c_int,
    pub probe_score: ::std::os::raw::c_int,
    pub format_probesize: ::std::os::raw::c_int,
    pub codec_whitelist: *mut ::std::os::raw::c_char,
    pub format_whitelist: *mut ::std::os::raw::c_char,
    pub io_repositioned: ::std::os::raw::c_int,
    pub video_codec: *const AVCodec,
    pub audio_codec: *const AVCodec,
    pub subtitle_codec: *const AVCodec,
    pub data_codec: *const AVCodec,
    pub metadata_header_padding: ::std::os::raw::c_int,
    pub opaque: *mut ::std::os::raw::c_void,
    pub control_message_cb: *mut c_void,
    pub output_ts_offset: i64,
    pub dump_separator: *mut u8,
    pub data_codec_id: AVCodecID,
    pub protocol_whitelist: *mut ::std::os::raw::c_char,
    pub io_open: *mut c_void,
    pub io_close: *mut c_void,
    pub protocol_blacklist: *mut ::std::os::raw::c_char,
    pub max_streams: ::std::os::raw::c_int,
    pub skip_estimate_duration_from_pts: ::std::os::raw::c_int,
    pub max_probe_packets: ::std::os::raw::c_int,
    pub io_close2: *mut c_void,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct AVCodecParameters {
    pub codec_type: AVMediaType,
    pub codec_id: AVCodecID,
    pub codec_tag: u32,
    pub extradata: *mut u8,
    pub extradata_size: ::std::os::raw::c_int,
    pub coded_side_data: *mut c_void,
    pub nb_coded_side_data: ::std::os::raw::c_int,
    pub format: ::std::os::raw::c_int,
    pub bit_rate: i64,
    pub bits_per_coded_sample: ::std::os::raw::c_int,
    pub bits_per_raw_sample: ::std::os::raw::c_int,
    pub profile: ::std::os::raw::c_int,
    pub level: ::std::os::raw::c_int,
    pub width: ::std::os::raw::c_int,
    pub height: ::std::os::raw::c_int,
    pub sample_aspect_ratio: AVRational,
    pub framerate: AVRational,
    pub field_order: AVFieldOrder,
    pub color_range: AVColorRange,
    pub color_primaries: AVColorPrimaries,
    pub color_trc: AVColorTransferCharacteristic,
    pub color_space: AVColorSpace,
    pub chroma_location: AVChromaLocation,
    pub video_delay: ::std::os::raw::c_int,
    pub ch_layout: AVChannelLayout,
    pub sample_rate: ::std::os::raw::c_int,
    pub block_align: ::std::os::raw::c_int,
    pub frame_size: ::std::os::raw::c_int,
    pub initial_padding: ::std::os::raw::c_int,
    pub trailing_padding: ::std::os::raw::c_int,
    pub seek_preroll: ::std::os::raw::c_int,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct AVChannelLayout {
    pub order: AVChannelOrder,
    pub nb_channels: ::std::os::raw::c_int,
    pub u: AVChannelLayout__bindgen_ty_1,
    pub opaque: *mut ::std::os::raw::c_void,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub union AVChannelLayout__bindgen_ty_1 {
    pub mask: u64,
    pub map: *mut c_void,
}

impl std::fmt::Debug for AVChannelLayout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mask = unsafe { self.u.mask };
        f.debug_struct("AVChannelLayout")
            .field("order", &self.order)
            .field("nb_channels", &self.nb_channels)
            .field("mask", &mask)
            .finish_non_exhaustive()
    }
}

#[allow(dead_code)]
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct AVProbeData {
    pub filename: *const ::std::os::raw::c_char,
    pub buf: *mut ::std::os::raw::c_uchar,
    pub buf_size: ::std::os::raw::c_int,
    pub mime_type: *const ::std::os::raw::c_char,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct AVStream {
    pub av_class: *const c_void,
    pub index: ::std::os::raw::c_int,
    pub id: ::std::os::raw::c_int,
    pub codecpar: *mut AVCodecParameters,
    pub priv_data: *mut ::std::os::raw::c_void,
    pub time_base: AVRational,
    pub start_time: i64,
    pub duration: i64,
    pub nb_frames: i64,
    pub disposition: ::std::os::raw::c_int,
    pub discard: AVDiscard,
    pub sample_aspect_ratio: AVRational,
    pub metadata: *mut c_void,
    pub avg_frame_rate: AVRational,
    pub attached_pic: AVPacket,
    pub event_flags: ::std::os::raw::c_int,
    pub r_frame_rate: AVRational,
    pub pts_wrap_bits: ::std::os::raw::c_int,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct AVCodecContext {
    pub av_class: *const c_void,
    pub log_level_offset: ::std::os::raw::c_int,
    pub codec_type: AVMediaType,
    pub codec: *const AVCodec,
    pub codec_id: AVCodecID,
    pub codec_tag: ::std::os::raw::c_uint,
    pub priv_data: *mut ::std::os::raw::c_void,
    pub internal: *mut c_void,
    pub opaque: *mut ::std::os::raw::c_void,
    pub bit_rate: i64,
    pub flags: ::std::os::raw::c_int,
    pub flags2: ::std::os::raw::c_int,
    pub extradata: *mut u8,
    pub extradata_size: ::std::os::raw::c_int,
    pub time_base: AVRational,
    pub pkt_timebase: AVRational,
    pub framerate: AVRational,
    pub delay: ::std::os::raw::c_int,
    pub width: ::std::os::raw::c_int,
    pub height: ::std::os::raw::c_int,
    pub coded_width: ::std::os::raw::c_int,
    pub coded_height: ::std::os::raw::c_int,
    pub sample_aspect_ratio: AVRational,
    pub pix_fmt: AVPixelFormat,
    pub sw_pix_fmt: AVPixelFormat,
    pub color_primaries: AVColorPrimaries,
    pub color_trc: AVColorTransferCharacteristic,
    pub colorspace: AVColorSpace,
    pub color_range: AVColorRange,
    pub chroma_sample_location: AVChromaLocation,
    pub field_order: AVFieldOrder,
    pub refs: ::std::os::raw::c_int,
    pub has_b_frames: ::std::os::raw::c_int,
    pub slice_flags: ::std::os::raw::c_int,
    pub draw_horiz_band: ::std::option::Option<
        unsafe extern "C" fn(
            s: *mut AVCodecContext,
            src: *const AVFrame,
            offset: *mut ::std::os::raw::c_int,
            y: ::std::os::raw::c_int,
            type_: ::std::os::raw::c_int,
            height: ::std::os::raw::c_int,
        ),
    >,
    pub get_format: ::std::option::Option<unsafe extern "C" fn(s: *mut AVCodecContext, fmt: *const AVPixelFormat) -> AVPixelFormat>,
    pub max_b_frames: ::std::os::raw::c_int,
    pub b_quant_factor: f32,
    pub b_quant_offset: f32,
    pub i_quant_factor: f32,
    pub i_quant_offset: f32,
    pub lumi_masking: f32,
    pub temporal_cplx_masking: f32,
    pub spatial_cplx_masking: f32,
    pub p_masking: f32,
    pub dark_masking: f32,
    pub nsse_weight: ::std::os::raw::c_int,
    pub me_cmp: ::std::os::raw::c_int,
    pub me_sub_cmp: ::std::os::raw::c_int,
    pub mb_cmp: ::std::os::raw::c_int,
    pub ildct_cmp: ::std::os::raw::c_int,
    pub dia_size: ::std::os::raw::c_int,
    pub last_predictor_count: ::std::os::raw::c_int,
    pub me_pre_cmp: ::std::os::raw::c_int,
    pub pre_dia_size: ::std::os::raw::c_int,
    pub me_subpel_quality: ::std::os::raw::c_int,
    pub me_range: ::std::os::raw::c_int,
    pub mb_decision: ::std::os::raw::c_int,
    pub intra_matrix: *mut u16,
    pub inter_matrix: *mut u16,
    pub chroma_intra_matrix: *mut u16,
    pub intra_dc_precision: ::std::os::raw::c_int,
    pub mb_lmin: ::std::os::raw::c_int,
    pub mb_lmax: ::std::os::raw::c_int,
    pub bidir_refine: ::std::os::raw::c_int,
    pub keyint_min: ::std::os::raw::c_int,
    pub gop_size: ::std::os::raw::c_int,
    pub mv0_threshold: ::std::os::raw::c_int,
    pub slices: ::std::os::raw::c_int,
    pub sample_rate: ::std::os::raw::c_int,
    pub sample_fmt: AVSampleFormat,
    pub ch_layout: AVChannelLayout,
    pub frame_size: ::std::os::raw::c_int,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct AVFrame {
    pub data: [*mut u8; 8usize],
    pub linesize: [::std::os::raw::c_int; 8usize],
    pub extended_data: *mut *mut u8,
    pub width: ::std::os::raw::c_int,
    pub height: ::std::os::raw::c_int,
    pub nb_samples: ::std::os::raw::c_int,
    pub format: ::std::os::raw::c_int,
    pub pict_type: AVPictureType,
    pub sample_aspect_ratio: AVRational,
    pub pts: i64,
    pub pkt_dts: i64,
    pub time_base: AVRational,
    pub quality: ::std::os::raw::c_int,
    pub opaque: *mut ::std::os::raw::c_void,
    pub repeat_pict: ::std::os::raw::c_int,
    pub sample_rate: ::std::os::raw::c_int,
    pub buf: [*mut AVBufferRef; 8usize],
    pub extended_buf: *mut *mut AVBufferRef,
    pub nb_extended_buf: ::std::os::raw::c_int,
    pub side_data: *mut *mut c_void,
    pub nb_side_data: ::std::os::raw::c_int,
    pub flags: ::std::os::raw::c_int,
    pub color_range: AVColorRange,
    pub color_primaries: AVColorPrimaries,
    pub color_trc: AVColorTransferCharacteristic,
    pub colorspace: AVColorSpace,
    pub chroma_location: AVChromaLocation,
    pub best_effort_timestamp: i64,
    pub metadata: *mut c_void,
    pub decode_error_flags: ::std::os::raw::c_int,
    pub hw_frames_ctx: *mut AVBufferRef,
    pub opaque_ref: *mut AVBufferRef,
    pub crop_top: usize,
    pub crop_bottom: usize,
    pub crop_left: usize,
    pub crop_right: usize,
    pub private_ref: *mut c_void,
    pub ch_layout: AVChannelLayout,
    pub duration: i64,
}
