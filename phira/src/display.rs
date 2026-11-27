//! Display refresh rate detection and runtime vsync control.
//!
//! miniquad only reads the swap interval when the GL context is created, so a
//! settings switch would otherwise need a restart. On Windows the WGL swap
//! interval can be toggled at runtime; other platforms fall back to the value
//! chosen at window creation.
//!
//! The refresh rate is queried through each platform's native API and returned
//! as `Option<u32>`: `None` means it could not be determined, in which case
//! callers should not hide high refresh rate options.

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
mod windows_impl {
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

    pub fn refresh_rate() -> Option<u32> {
        unsafe {
            let hdc = GetDC(std::ptr::null_mut());
            if hdc.is_null() {
                return None;
            }
            let rate = GetDeviceCaps(hdc, VREFRESH);
            ReleaseDC(std::ptr::null_mut(), hdc);
            if rate > 0 {
                Some(rate as u32)
            } else {
                None
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Android: WindowManager#getDefaultDisplay().getRefreshRate()
// ---------------------------------------------------------------------------

#[cfg(target_os = "android")]
mod android_impl {
    pub fn refresh_rate() -> Option<u32> {
        unsafe {
            let env = miniquad::native::attach_jni_env();
            let ctx = ndk_context::android_context().context();
            if env.is_null() || ctx.is_null() {
                return None;
            }
            let activity = ctx as jni::sys::jobject;

            let activity_class = ((**env).GetObjectClass.unwrap())(env, activity);
            if activity_class.is_null() {
                return None;
            }
            let get_wm =
                ((**env).GetMethodID.unwrap())(env, activity_class, c"getWindowManager".as_ptr(), c"()Landroid/view/WindowManager;".as_ptr());
            if get_wm.is_null() {
                return None;
            }
            let wm = ((**env).CallObjectMethod.unwrap())(env, activity, get_wm);
            if wm.is_null() {
                return None;
            }

            let wm_class = ((**env).GetObjectClass.unwrap())(env, wm);
            let get_display = ((**env).GetMethodID.unwrap())(env, wm_class, c"getDefaultDisplay".as_ptr(), c"()Landroid/view/Display;".as_ptr());
            if get_display.is_null() {
                return None;
            }
            let display = ((**env).CallObjectMethod.unwrap())(env, wm, get_display);
            if display.is_null() {
                return None;
            }

            let display_class = ((**env).GetObjectClass.unwrap())(env, display);
            let get_rate = ((**env).GetMethodID.unwrap())(env, display_class, c"getRefreshRate".as_ptr(), c"()F".as_ptr());
            if get_rate.is_null() {
                return None;
            }
            let rate = ((**env).CallFloatMethod.unwrap())(env, display, get_rate);
            if rate.is_finite() && rate > 0. {
                Some(rate.round() as u32)
            } else {
                None
            }
        }
    }
}

// ---------------------------------------------------------------------------
// iOS: UIScreen.maximumFramesPerSecond
// ---------------------------------------------------------------------------

#[cfg(target_os = "ios")]
mod ios_impl {
    pub fn refresh_rate() -> Option<u32> {
        use objc2::MainThreadMarker;
        use objc2_ui_kit::UIScreen;

        // `mainScreen` is main-thread only; callers run on the main thread.
        let mtm = MainThreadMarker::new()?;
        let screen = UIScreen::mainScreen(mtm);
        let rate = screen.maximumFramesPerSecond();
        if rate > 0 {
            Some(rate as u32)
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Platform dispatch
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
pub use windows_impl::{refresh_rate, set_vsync};

#[cfg(target_os = "android")]
pub use android_impl::refresh_rate;

#[cfg(target_os = "ios")]
pub use ios_impl::refresh_rate;

/// Fallback for platforms without a native query (e.g. OpenHarmony): report
/// "unknown" so callers keep showing high refresh rate options.
#[cfg(not(any(target_os = "windows", target_os = "android", target_os = "ios")))]
pub fn refresh_rate() -> Option<u32> {
    None
}

#[cfg(not(target_os = "windows"))]
pub fn set_vsync(_enabled: bool) -> bool {
    false
}

// ---------------------------------------------------------------------------
// DPI awareness (Windows only)
// ---------------------------------------------------------------------------

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
pub fn ensure_dpi_aware() {}

#[cfg(not(target_os = "windows"))]
pub fn dpi_scale() -> f32 {
    1.
}
