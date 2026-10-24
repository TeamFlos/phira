//! Display refresh rate detection and runtime vsync control.
//!
//! miniquad only reads the swap interval when the GL context is created, so a
//! settings switch would otherwise need a restart. On Windows the WGL swap
//! interval can be toggled at runtime; other platforms fall back to the value
//! chosen at window creation.

#[cfg(target_os = "windows")]
mod imp {
    use std::{
        ffi::c_void,
        sync::OnceLock,
    };

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
