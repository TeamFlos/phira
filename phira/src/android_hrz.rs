/// Android high refresh rate support.
///
/// On Android, the system's LTPO/power-saving strategy may cap the display
/// refresh rate at 60 Hz for apps that don't explicitly request a higher mode.
/// This module queries the display's supported modes at startup and sets
/// `preferredDisplayModeId` on the Activity's Window to the highest available
/// refresh rate, making Phira run at 120 Hz (or whatever the panel supports)
/// without requiring a global system override.

#[cfg(target_os = "android")]
pub fn request_high_refresh_rate() {
    use jni::{objects::JObject, vm::JavaVM};

    let vm = match JavaVM::singleton() {
        Ok(vm) => vm,
        Err(e) => {
            tracing::warn!("Failed to get JavaVM for high refresh rate: {e}");
            return;
        }
    };

    let result = vm.attach_current_thread(|env| -> jni::errors::Result<()> {
        // 1. Get the Activity (same pattern as call_activity_void in lib.rs)
        let ctx = unsafe { JObject::from_raw(env, ndk_context::android_context().context() as _) };

        // 2. activity.getWindow()
        let window = env
            .call_method(&ctx, "getWindow", "()Landroid/view/Window;", &[])?
            .l()?;

        // 3. window.getAttributes()  →  WindowManager.LayoutParams
        let attrs = env
            .call_method(
                &window,
                "getAttributes",
                "()Landroid/view/WindowManager$LayoutParams;",
                &[],
            )?
            .l()?;

        // 4. window.getWindowManager().getDefaultDisplay().getSupportedModes()
        let wm = env
            .call_method(&window, "getWindowManager", "()Landroid/view/WindowManager;", &[])?
            .l()?;

        let display = env
            .call_method(&wm, "getDefaultDisplay", "()Landroid/view/Display;", &[])?
            .l()?;

        let modes = env
            .call_method(
                &display,
                "getSupportedModes",
                "()[Landroid/view/Display$Mode;",
                &[],
            )?
            .l()?;

        // 5. Iterate modes to find the highest refresh rate
        let len = env.get_array_length(&modes).unwrap_or(0);
        let mut best_id: i32 = 0;
        let mut best_fps: f32 = 60.0;

        for i in 0..len {
            let mode = match env.get_object_array_element(&modes, i) {
                Ok(m) => m,
                Err(_) => continue,
            };
            let fps = env
                .call_method(&mode, "getRefreshRate", "()F", &[])
                .ok()
                .and_then(|v| v.f().ok())
                .unwrap_or(0.0);
            let id = env
                .call_method(&mode, "getModeId", "()I", &[])
                .ok()
                .and_then(|v| v.i().ok())
                .unwrap_or(0);
            if fps > best_fps {
                best_fps = fps;
                best_id = id;
            }
        }

        // 6. Apply: set preferredDisplayModeId on the LayoutParams
        if best_id > 0 {
            env.set_field(&attrs, "preferredDisplayModeId", "I", best_id.into())?;
            env.call_method(
                &window,
                "setAttributes",
                "(Landroid/view/WindowManager$LayoutParams;)V",
                &[(&attrs).into()],
            )?;
            tracing::info!(
                "Android: requested high refresh rate — preferredDisplayModeId={} ({} Hz)",
                best_id,
                best_fps
            );
        } else {
            tracing::info!("Android: no high refresh rate mode found beyond 60 Hz");
        }

        Ok(())
    });

    if let Err(e) = result {
        tracing::warn!("Failed to set high refresh rate on Android: {e}");
    }
}

#[cfg(not(target_os = "android"))]
pub fn request_high_refresh_rate() {}
