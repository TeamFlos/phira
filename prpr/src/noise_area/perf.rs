//! Bounded, asynchronous diagnostics. No glFinish, readback, or file I/O on the render thread.
use std::time::{Duration, Instant};
const CHANNELS: usize = 12;
pub const LABELS: [&str; CHANNELS] = [
    "geometry",
    "masks",
    "subtract",
    "compose",
    "precise",
    "edge",
    "glow",
    "pack",
    "scene_copy",
    "scene_restore",
    "disabled",
    "active",
];
#[derive(Clone, Copy, Default)]
struct Stats {
    cpu_ns: u64,
    cpu_max_ns: u64,
    cpu_count: u64,
    gpu_ns: u64,
    gpu_count: u64,
    gpu_invalid_count: u64,
}
pub struct Token {
    started: Instant,
    channel: usize,
    query: Option<u32>,
}
pub struct Profiler {
    stats: [Stats; CHANNELS],
    last_log: Instant,
    frames: u64,
    sampled: bool,
    gpu: Option<gpu::Timer>,
    pending_log: [Stats; CHANNELS],
    log_cursor: usize,
}
impl Profiler {
    pub fn new() -> Self {
        let gpu = gpu::Timer::new();
        tracing::info!(target: "noise_perf", gpu_supported = gpu.is_some(), gpu_sampling_interval_frames = 60, "noise render profiler ready");
        Self {
            stats: [Stats::default(); CHANNELS],
            last_log: Instant::now(),
            frames: 0,
            sampled: false,
            gpu,
            pending_log: [Stats::default(); CHANNELS],
            log_cursor: CHANNELS,
        }
    }
    pub fn frame(&mut self, low: bool, precise: bool, w: u32, h: u32) {
        self.frames += 1;
        self.sampled = self.frames % 60 == 1;
        if let Some(gpu) = &mut self.gpu {
            gpu.poll(&mut self.stats);
        }
        if self.last_log.elapsed() >= Duration::from_secs(2) {
            self.pending_log = self.stats;
            self.stats = [Stats::default(); CHANNELS];
            self.log_cursor = 0;
            self.last_log = Instant::now();
        }
        // Spread the snapshot over subsequent frames instead of formatting
        // all channels in one frame on slower phones.
        while self.log_cursor < CHANNELS {
            let i = self.log_cursor;
            self.log_cursor += 1;
            let s = self.pending_log[i];
            if s.cpu_count == 0 {
                continue;
            }
            tracing::info!(target: "noise_perf", channel = LABELS[i], calls = s.cpu_count,
                cpu_submit_avg_ms = s.cpu_ns as f64 / s.cpu_count as f64 / 1e6,
                cpu_submit_max_ms = s.cpu_max_ns as f64 / 1e6,
                gpu_samples = s.gpu_count, gpu_invalid_samples = s.gpu_invalid_count,
                gpu_avg_ms = if s.gpu_count == 0 { -1. } else { s.gpu_ns as f64 / s.gpu_count as f64 / 1e6 },
                gpu_supported = self.gpu.is_some(), low_performance = low, precise_edges = precise, width = w, height = h,
                "noise render channel timings");
            break;
        }
    }

    pub fn begin(&mut self, channel: usize) -> Token {
        let query = if self.sampled && channel != 0 {
            self.gpu.as_mut().and_then(|gpu| gpu.begin(channel))
        } else {
            None
        };
        Token {
            started: Instant::now(),
            channel,
            query,
        }
    }
    pub fn end(&mut self, token: Token) {
        let ns = token.started.elapsed().as_nanos().min(u64::MAX as u128) as u64;
        let stats = &mut self.stats[token.channel];
        stats.cpu_ns = stats.cpu_ns.saturating_add(ns);
        stats.cpu_max_ns = stats.cpu_max_ns.max(ns);
        stats.cpu_count += 1;
        if let Some(id) = token.query {
            if let Some(gpu) = &mut self.gpu {
                gpu.end(id, token.channel);
            }
        }
    }
}
#[cfg(not(target_os = "android"))]
mod gpu {
    use super::Stats;
    pub struct Timer;
    impl Timer {
        pub fn new() -> Option<Self> {
            None
        }
        pub fn poll(&mut self, _: &mut [Stats]) {}
        pub fn begin(&mut self, _: usize) -> Option<u32> {
            None
        }
        pub fn end(&mut self, _: u32, _: usize) {}
    }
}
#[cfg(target_os = "android")]
mod gpu {
    use super::Stats;
    use std::time::Instant;
    use std::{
        collections::VecDeque,
        ffi::{c_void, CStr},
    };
    const ELAPSED: u32 = 0x88BF;
    const AVAILABLE: u32 = 0x8867;
    const RESULT: u32 = 0x8866;
    const DISJOINT: u32 = 0x8FBB;
    const MAX_QUERIES: usize = 96;
    #[link(name = "EGL")]
    extern "C" {
        fn eglGetProcAddress(name: *const i8) -> *const c_void;
    }
    type GetString = unsafe extern "C" fn(u32) -> *const u8;
    type GetInt = unsafe extern "C" fn(u32, *mut i32);
    type Gen = unsafe extern "C" fn(i32, *mut u32);
    type Delete = unsafe extern "C" fn(i32, *const u32);
    type Begin = unsafe extern "C" fn(u32, u32);
    type End = unsafe extern "C" fn(u32);
    type GetAvailable = unsafe extern "C" fn(u32, u32, *mut u32);
    type GetResult = unsafe extern "C" fn(u32, u32, *mut u64);
    type GetQuery = unsafe extern "C" fn(u32, u32, *mut i32);
    pub struct Timer {
        ids: [u32; MAX_QUERIES],
        free: Vec<u32>,
        pending: VecDeque<(u32, usize, Instant)>,
        active_started: Instant,
        get_int: GetInt,
        begin: Begin,
        end: End,
        available: GetAvailable,
        result: GetResult,
        delete: Delete,
        discard_pending: bool,
    }
    impl Timer {
        pub fn new() -> Option<Self> {
            unsafe {
                macro_rules! load {
                    ($name:literal, $ty:ty) => {{
                        let p = eglGetProcAddress(concat!($name, "\0").as_ptr().cast());
                        if p.is_null() {
                            return None;
                        }
                        std::mem::transmute::<*const c_void, $ty>(p)
                    }};
                }
                let string = load!("glGetString", GetString);
                let extensions = string(0x1F03);
                if extensions.is_null()
                    || !CStr::from_ptr(extensions.cast())
                        .to_string_lossy()
                        .split_whitespace()
                        .any(|s| s == "GL_EXT_disjoint_timer_query")
                {
                    return None;
                }
                let query = load!("glGetQueryivEXT", GetQuery);
                let mut bits = 0;
                query(ELAPSED, 0x8864, &mut bits);
                if bits == 0 {
                    return None;
                }
                let gen = load!("glGenQueriesEXT", Gen);
                let mut timer = Self {
                    ids: [0; MAX_QUERIES],
                    free: Vec::with_capacity(MAX_QUERIES),
                    pending: VecDeque::with_capacity(MAX_QUERIES),
                    active_started: Instant::now(),
                    get_int: load!("glGetIntegerv", GetInt),
                    begin: load!("glBeginQueryEXT", Begin),
                    end: load!("glEndQueryEXT", End),
                    available: load!("glGetQueryObjectuivEXT", GetAvailable),
                    result: load!("glGetQueryObjectui64vEXT", GetResult),
                    delete: load!("glDeleteQueriesEXT", Delete),
                    discard_pending: false,
                };
                gen(MAX_QUERIES as i32, timer.ids.as_mut_ptr());
                timer.free.extend(timer.ids.iter().copied().filter(|id| *id != 0));
                Some(timer)
            }
        }
        pub fn begin(&mut self, _: usize) -> Option<u32> {
            if self.discard_pending {
                return None;
            }
            let id = self.free.pop()?;
            self.active_started = Instant::now();
            unsafe {
                (self.begin)(ELAPSED, id);
            }
            Some(id)
        }
        pub fn end(&mut self, id: u32, channel: usize) {
            unsafe {
                (self.end)(ELAPSED);
            }
            self.pending.push_back((id, channel, self.active_started));
        }
        pub fn poll(&mut self, stats: &mut [Stats]) {
            unsafe {
                let mut disjoint = 0;
                (self.get_int)(DISJOINT, &mut disjoint);
                if disjoint != 0 {
                    self.discard_pending = true;
                    for s in stats.iter_mut() {
                        s.gpu_ns = 0;
                        s.gpu_count = 0;
                    }
                }
                while let Some(&(id, channel, started)) = self.pending.front() {
                    let mut ready = 0;
                    (self.available)(id, AVAILABLE, &mut ready);
                    if ready == 0 {
                        break;
                    }
                    if !self.discard_pending {
                        let mut ns = 0;
                        (self.result)(id, RESULT, &mut ns);
                        // A GPU interval cannot exceed the CPU wall interval from
                        // issuing its query to observing the result. Some drivers
                        // return an uninitialized timestamp for the first query.
                        if ns as u128 <= started.elapsed().as_nanos() + 1_000_000 {
                            stats[channel].gpu_ns = stats[channel].gpu_ns.saturating_add(ns);
                            stats[channel].gpu_count += 1;
                        } else {
                            stats[channel].gpu_invalid_count += 1;
                        }
                    }
                    self.pending.pop_front();
                    self.free.push(id);
                }
                if self.pending.is_empty() {
                    self.discard_pending = false;
                }
            }
        }
    }
    impl Drop for Timer {
        fn drop(&mut self) {
            unsafe {
                (self.delete)(MAX_QUERIES as i32, self.ids.as_ptr());
            }
        }
    }
}
