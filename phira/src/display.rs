//! Display refresh rate detection and runtime vsync control.
//!
//! miniquad only reads the swap interval when the GL context is created, so a
//! settings switch would otherwise need a restart. On Windows the WGL swap
//! interval can be toggled at runtime; other platforms fall back to the value
//! chosen at window creation.

#[cfg(target_os = "windows")]
mod imp {
    use std::{ffi::c_void, sync::OnceLock};

    #[link(name = "opengl32")]
    extern "system" {
        fn wglGetProcAddress(name: *const i8) -> *const c_void;
    }

    #[link(name = "user32")]
    extern "system" {
        fn GetDC(hwnd: *mut c_void) -> *mut c_void;
        fn ReleaseDC(hwnd: *mut c_void, hdc: *mut c_void) -> i32;
    }

    #[link(name = "gdi32")]
    extern "system" {
        fn GetDeviceCaps(hdc: *mut c_void, index: i32) -> i32;
    }

    type SwapIntervalFn = unsafe extern "system" fn(i32) -> i32;

    const VREFRESH: i32 = 116;

    fn swap_interval_fn() -> Option<SwapIntervalFn> {
        static FN: OnceLock<Option<SwapIntervalFn>> = OnceLock::new();
        *FN.get_or_init(|| unsafe {
            let ptr = wglGetProcAddress(c"wglSwapIntervalEXT".as_ptr());
            if ptr.is_null() {
                None
            } else {
                Some(std::mem::transmute::<*const c_void, SwapIntervalFn>(ptr))
            }
        })
    }

    pub fn set_vsync(enabled: bool) -> bool {
        match swap_interval_fn() {
            Some(f) => {
                unsafe { f(if enabled { 1 } else { 0 }) };
                true
            }
            None => false,
        }
    }

    pub fn refresh_rate() -> u32 {
        unsafe {
            let hdc = GetDC(std::ptr::null_mut());
            if hdc.is_null() {
                return 60;
            }
            let rate = GetDeviceCaps(hdc, VREFRESH);
            ReleaseDC(std::ptr::null_mut(), hdc);
            if rate > 0 {
                rate as u32
            } else {
                60
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod imp {
    pub fn set_vsync(_enabled: bool) -> bool {
        false
    }

    pub fn refresh_rate() -> u32 {
        60
    }
}

pub use imp::{refresh_rate, set_vsync};

/// Declare the process DPI aware before the window is created.
///
/// miniquad asks for `PROCESS_DPI_UNAWARE` when `high_dpi` is false, which makes
/// Windows virtualize the client area: a 2560×1600 panel reports 1463×914 to
/// `GetSystemMetrics`, the framebuffer is created at that logical size and the
/// compositor upscales it, so the game never renders at the panel resolution.
///
/// Claiming system DPI awareness first makes that later call fail harmlessly;
/// miniquad then keeps its scale factors at 1.0 while the window is sized with
/// real pixels, so the framebuffer matches the panel 1:1.
#[cfg(target_os = "windows")]
pub fn ensure_dpi_aware() {
    #[link(name = "shcore")]
    extern "system" {
        fn SetProcessDpiAwareness(value: i32) -> i32;
    }

    // PROCESS_SYSTEM_DPI_AWARE = 1. Failure is fine: the process may already be
    // DPI aware (manifest, compatibility layer) and the result is the same.
    unsafe {
        SetProcessDpiAwareness(1);
    }
}

/// System DPI scale (96 DPI = 1.0). Used to keep the windowed size looking the
/// same once the process is DPI aware: miniquad sizes windows in physical
/// pixels in that case.
#[cfg(target_os = "windows")]
pub fn dpi_scale() -> f32 {
    use std::ffi::c_void;

    #[link(name = "user32")]
    extern "system" {
        fn GetDC(hwnd: *mut c_void) -> *mut c_void;
        fn ReleaseDC(hwnd: *mut c_void, hdc: *mut c_void) -> i32;
    }

    #[link(name = "gdi32")]
    extern "system" {
        fn GetDeviceCaps(hdc: *mut c_void, index: i32) -> i32;
    }

    const LOGPIXELSY: i32 = 90;

    unsafe {
        let hdc = GetDC(std::ptr::null_mut());
        if hdc.is_null() {
            return 1.;
        }
        let dpi = GetDeviceCaps(hdc, LOGPIXELSY);
        ReleaseDC(std::ptr::null_mut(), hdc);
        if dpi > 0 {
            dpi as f32 / 96.
        } else {
            1.
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn dpi_scale() -> f32 {
    1.
}

#[cfg(not(target_os = "windows"))]
pub fn ensure_dpi_aware() {}
