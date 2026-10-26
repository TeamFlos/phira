prpr_l10n::tl_file!("common" ttl crate::);

#[rustfmt::skip]
#[cfg(closed)]
mod inner;

mod anim;
mod censor;
mod charts_view;
mod client;
mod crash;
mod data;
pub mod deeplink;
mod display;
mod icons;
mod images;
mod login;
mod mp;
mod page;
mod popup;
mod rate;
mod resource;
mod scene;
mod tabs;
mod tags;
mod threed;
mod uml;

use anyhow::Result;
use data::Data;
use macroquad::prelude::*;
use prpr::{
    build_conf,
    core::{init_assets, PGR_FONT},
    ext::SafeTexture,
    log,
    scene::show_error,
    time::TimeManager,
    ui::{cleanup_audio, FontArc, TextPainter},
    Main,
};
use prpr_l10n::set_prefered_locale;
#[cfg(not(feature = "hykb"))]
use prpr_l10n::{GLOBAL, LANGS};
use scene::MainScene;
use std::{
    collections::VecDeque,
    sync::{mpsc, Mutex},
};
use tracing::{error, info};

// Only used by the Android-only GTS anti-addiction callback below.
#[cfg(target_os = "android")]
use tracing::warn;

#[cfg(target_os = "android")]
use jni::{
    objects::{JClass, JString},
    sys::jint,
    EnvUnowned,
};

static MESSAGES_TX: Mutex<Option<mpsc::Sender<bool>>> = Mutex::new(None);
static DATA_PATH: Mutex<Option<String>> = Mutex::new(None);
static CACHE_DIR: Mutex<Option<String>> = Mutex::new(None);
pub static mut DATA: Option<Data> = None;

#[cfg(target_env = "ohos")]
use napi_derive_ohos::napi;

#[cfg(closed)]
pub async fn load_res(name: &str) -> Vec<u8> {
    let bytes = load_file(name).await.unwrap();
    inner::resolve_data(bytes)
}

#[allow(unused)]
pub async fn load_res_tex(name: &str) -> SafeTexture {
    #[cfg(closed)]
    {
        let bytes = load_res(name).await;
        let image = image::load_from_memory(&bytes).unwrap();
        image.into()
    }
    #[cfg(not(closed))]
    prpr::ext::BLACK_TEXTURE.clone()
}

pub fn sync_data() {
    if get_data().language.is_none() {
        #[cfg(feature = "hykb")]
        let default_lang = "zh-CN".to_owned();
        #[cfg(not(feature = "hykb"))]
        let default_lang = LANGS[GLOBAL.order.lock().unwrap()[0]].to_owned();
        get_data_mut().language = Some(default_lang);
    }
    set_prefered_locale(get_data().language.as_ref().and_then(|it| it.parse().ok()));
    let _ = client::set_access_token_sync(get_data().tokens.as_ref().map(|it| &*it.0));
}

pub fn set_data(data: Data) {
    unsafe {
        DATA = Some(data);
    }
}

#[allow(static_mut_refs)]
pub fn get_data() -> &'static Data {
    unsafe { DATA.as_ref().unwrap() }
}

#[allow(static_mut_refs)]
pub fn get_data_mut() -> &'static mut Data {
    unsafe { DATA.as_mut().unwrap() }
}

pub fn save_data() -> Result<()> {
    std::fs::write(format!("{}/data.json", dir::root()?), serde_json::to_string(get_data())?)?;
    Ok(())
}

mod dir {
    use anyhow::Result;

    use crate::{CACHE_DIR, DATA_PATH};

    fn ensure(s: &str) -> Result<String> {
        let s = format!("{}/{}", DATA_PATH.lock().unwrap().as_ref().map(|it| it.as_str()).unwrap_or("."), s);
        let path = std::path::Path::new(&s);
        if !path.exists() {
            std::fs::create_dir_all(path)?;
        }
        Ok(s)
    }

    pub fn cache() -> Result<String> {
        if let Some(cache) = &*CACHE_DIR.lock().unwrap() {
            ensure(cache)
        } else {
            ensure("cache")
        }
    }

    pub fn bold_font_path() -> Result<String> {
        Ok(format!("{}/bold.ttf", root()?))
    }

    pub fn cache_image_local() -> Result<String> {
        ensure(&format!("{}/image", cache()?))
    }

    pub fn root() -> Result<String> {
        ensure("data")
    }

    pub fn charts() -> Result<String> {
        ensure("data/charts")
    }

    pub fn collections() -> Result<String> {
        ensure("data/collections")
    }

    pub fn custom_charts() -> Result<String> {
        ensure("data/charts/custom")
    }

    pub fn downloaded_charts() -> Result<String> {
        ensure("data/charts/download")
    }

    pub fn respacks() -> Result<String> {
        ensure("data/respack")
    }
}

async fn the_main() -> Result<()> {
    #[cfg(target_env = "ohos")]
    {
        *DATA_PATH.lock().unwrap() = Some("/data/storage/el2/base".to_owned());
        *CACHE_DIR.lock().unwrap() = Some("/data/storage/el2/base/cache".to_owned());
        prpr::core::DPI_VALUE.store(250, std::sync::atomic::Ordering::Relaxed);
    };

    init_assets();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap();
    let _guard = rt.enter();

    #[cfg(target_os = "ios")]
    {
        use objc2_foundation::{NSSearchPathDirectory, NSSearchPathDomainMask, NSSearchPathForDirectoriesInDomains};

        let directories = NSSearchPathForDirectoriesInDomains(NSSearchPathDirectory::LibraryDirectory, NSSearchPathDomainMask::UserDomainMask, true);
        let path = directories.firstObject().unwrap().to_string();
        *DATA_PATH.lock().unwrap() = Some(path);
        *CACHE_DIR.lock().unwrap() = Some("Caches".to_owned());
    }

    let dir = dir::root()?;
    log::register_with_dir(std::path::Path::new(&dir).join("logs"));
    info!("window {}x{}", screen_width() as u32, screen_height() as u32);
    info!("display refresh rate: {:?} Hz", display::refresh_rate());
    let mut data: Data = std::fs::read_to_string(format!("{dir}/data.json"))
        .map_err(anyhow::Error::new)
        .and_then(|s| Ok(serde_json::from_str(&s)?))
        .unwrap_or_default();
    data.init().await?;
    set_data(data);
    sync_data();
    save_data()?;

    // Warm up the offline banned-word automaton so local edits can check
    // synchronously. No-op without the `aa` feature.
    tokio::spawn(censor::preload());

    let rx = {
        let (tx, rx) = mpsc::channel();
        *MESSAGES_TX.lock().unwrap() = Some(tx);
        rx
    };

    unsafe { get_internal_gl() }
        .quad_context
        .display_mut()
        .set_pause_resume_listener(on_pause_resume);
    once_cell::sync::Lazy::force(&prpr::ext::BLACK_TEXTURE);

    let pgr_font = FontArc::try_from_vec(load_file("phigros.ttf").await?)?;
    PGR_FONT.with(move |it| *it.borrow_mut() = Some(TextPainter::new(pgr_font, None)));

    let font = FontArc::try_from_vec(load_file("font.ttf").await?)?;
    let mut painter = TextPainter::new(font.clone(), None);

    let mut main = Main::new(Box::new(MainScene::new(font).await?), TimeManager::default(), None).await?;

    // Apply the persisted presentation mode; runtime switching is handled by
    // the settings page through `display::set_vsync`.
    display::set_vsync(get_data().config.frame_rate_limit.needs_vsync());

    let tm = TimeManager::default();
    let mut fps_time = -1;

    const FPS_BUF_SIZE: usize = 60;
    let mut fps_times = VecDeque::<f32>::with_capacity(FPS_BUF_SIZE);
    let mut last_frame_start = f32::NAN;
    let mut fps_time_sum = 0.;

    let mut crash_scene = false;
    'app: loop {
        if main.paused() {
            match rx.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(false) => {
                    main.resume()?;
                }
                Ok(true) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break 'app,
            }
        }
        // Poll for a crash record only once we are about to render: while
        // paused the rendering surface may already be gone (Android), so the
        // switch to the crash scene has to wait for the resume above.
        if !crash_scene {
            if let Some(record) = log::crash_record() {
                main.replace_scene(Box::new(crash::CrashScene::new(record)));
                crash_scene = true;
            }
        }

        let frame_begin = std::time::Instant::now();
        let frame_start = tm.real_time();
        if !last_frame_start.is_nan() {
            if fps_times.len() == FPS_BUF_SIZE {
                fps_time_sum -= fps_times.pop_front().unwrap();
            }
            let frame_time = frame_start as f32 - last_frame_start;
            fps_times.push_back(frame_time);
            fps_time_sum += frame_time;
        }
        last_frame_start = frame_start as f32;
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<()> {
            main.update()?;
            main.render(&mut painter)?;
            if let Ok(paused) = rx.try_recv() {
                if paused {
                    main.pause()?;
                } else if main.paused() {
                    main.resume()?;
                }
            }
            prpr::ext::flush_pending_texture_deletions();
            Ok(())
        }));
        match res {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                error!("uncaught error: {err:?}");
                // Transient update/render failures stay dismissible dialogs;
                // only panics escalate to the crash scene.
                show_error(err);
            }
            Err(_) => {
                let record = log::crash_record().unwrap_or_else(|| {
                    let record = log::CrashRecord {
                        summary: "unknown panic".to_owned(),
                        details: "The main loop panicked without a panic record.".to_owned(),
                    };
                    log::set_crash_record(record.clone());
                    record
                });
                if !crash_scene {
                    main.replace_scene(Box::new(crash::CrashScene::new(record)));
                    crash_scene = true;
                } else {
                    break 'app;
                }
            }
        }
        if main.should_exit() {
            break 'app;
        }

        let t = tm.real_time();

        let fps_now = t as i32;
        if fps_now != fps_time {
            fps_time = fps_now;
            if fps_times.len() == FPS_BUF_SIZE {
                let actual_fps = 1. / (fps_time_sum / FPS_BUF_SIZE as f32);
                let current_fps = 1. / (t - frame_start);
                info!("FPS {} (capped at {})", current_fps as u32, actual_fps as u32);
            }
        }

        // While backgrounded the scene is paused; the timed receive above
        // keeps this thread responsive to background-thread crash records.
        next_frame().await;

        // Pace fixed frame rate caps *after* presenting the frame, so the
        // buffer swap counts towards the target interval. Vertical sync and
        // "unlimited" are left to the presentation backend.
        if let Some(target) = get_data().config.frame_rate_limit.target_duration() {
            let elapsed = frame_begin.elapsed();
            if elapsed < target {
                // `sleep` alone overshoots by ~1-2ms on Windows; sleep most of
                // the way and busy-wait the rest for accurate pacing.
                let remain = target - elapsed;
                let spin = std::time::Duration::from_millis(2);
                if remain > spin {
                    std::thread::sleep(remain - spin);
                }
                while frame_begin.elapsed() < target {
                    std::hint::spin_loop();
                }
            }
        }
    }
    Ok(())
}

fn build_global_window_conf() -> Conf {
    let mut conf = build_conf();
    conf.window_title = "Phira".to_owned();
    conf.icon = Some(miniquad::conf::Icon {
        small: *include_bytes!("../icon/small"),
        medium: *include_bytes!("../icon/medium"),
        big: *include_bytes!("../icon/big"),
    });

    // Read the persisted config once: it decides the presentation mode and,
    // on Windows, whether the window starts fullscreen.
    let saved = dir::root()
        .ok()
        .and_then(|r| std::fs::read_to_string(std::path::Path::new(&r).join("data.json")).ok())
        .and_then(|s| serde_json::from_str::<Data>(&s).ok());

    #[cfg(target_os = "windows")]
    {
        conf.fullscreen = saved.as_ref().is_some_and(|d| d.config.fullscreen_mode);
        // The process is DPI aware now, so miniquad sizes windows in physical
        // pixels; scale the default window size so windowed mode keeps the same
        // apparent size as before.
        let scale = display::dpi_scale();
        conf.window_width = (conf.window_width as f32 * scale) as i32;
        conf.window_height = (conf.window_height as f32 * scale) as i32;
    }

    // Vertical sync can only be chosen when the GL context is created, so the
    // frame rate limit is applied here; fixed caps are paced by the main loop.
    #[cfg(not(any(target_os = "android", target_os = "ios", target_env = "ohos")))]
    {
        let limit = saved.as_ref().map(|d| d.config.frame_rate_limit).unwrap_or_default();
        conf.platform.swap_interval = Some(if limit.needs_vsync() { 1 } else { 0 });
    }

    conf
}

#[no_mangle]
pub extern "C" fn quad_main() {
    // Must run before the window is created so the framebuffer uses the real
    // panel resolution instead of a DPI-virtualized logical size.
    display::ensure_dpi_aware();
    macroquad::Window::from_config(build_global_window_conf(), async {
        if let Err(err) = the_main().await {
            error!(?err, "global error");
        }
    });
    cleanup_audio();
}

fn on_pause_resume(pause: bool) {
    if let Some(tx) = MESSAGES_TX.lock().unwrap().as_mut() {
        let _ = tx.send(pause);
    }
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_quad_1native_QuadNative_initializeEnvironment(env: EnvUnowned, _class: JClass) {
    unsafe {
        inputbox::backend::Android::initialize_raw(env.as_raw()).unwrap();
    }
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_quad_1native_QuadNative_prprActivityOnPause(_env: EnvUnowned, _class: JClass) {
    if let Some(tx) = MESSAGES_TX.lock().unwrap().as_mut() {
        let _ = tx.send(true);
    }
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_quad_1native_QuadNative_prprActivityOnResume(_env: EnvUnowned, _class: JClass) {
    if let Some(tx) = MESSAGES_TX.lock().unwrap().as_mut() {
        let _ = tx.send(false);
    }
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_quad_1native_QuadNative_prprActivityOnDestroy(_env: EnvUnowned, _class: JClass) {
    std::process::exit(0);
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_quad_1native_QuadNative_setDataPath(_env: EnvUnowned, _class: JClass, path: JString) {
    *DATA_PATH.lock().unwrap() = Some(path.to_string());
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_quad_1native_QuadNative_setTempDir(_env: EnvUnowned, _class: JClass, path: JString) {
    let path = path.to_string();
    std::env::set_var("TMPDIR", path.clone());
    *CACHE_DIR.lock().unwrap() = Some(path);
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_quad_1native_QuadNative_setDpi(_env: EnvUnowned, _class: JClass, dpi: jint) {
    prpr::core::DPI_VALUE.store(dpi as _, std::sync::atomic::Ordering::SeqCst);
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_quad_1native_QuadNative_setChosenFile(_env: EnvUnowned, _class: JClass, file: JString) {
    use prpr::scene::CHOSEN_FILE;
    CHOSEN_FILE.lock().unwrap().1 = Some(file.to_string());
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_quad_1native_QuadNative_setDeepLink(_env: EnvUnowned, _class: JClass, url: JString) {
    deeplink::set_deeplink(url.to_string());
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_quad_1native_QuadNative_markImport(_env: EnvUnowned, _class: JClass) {
    use prpr::scene::CHOSEN_FILE;

    CHOSEN_FILE.lock().unwrap().0 = Some("_import".to_owned());
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_quad_1native_QuadNative_markImportRespack(_env: EnvUnowned, _class: JClass) {
    use prpr::scene::CHOSEN_FILE;

    CHOSEN_FILE.lock().unwrap().0 = Some("_import_respack".to_owned());
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_quad_1native_QuadNative_setInputText(_env: EnvUnowned, _class: JClass, text: JString) {
    use prpr::scene::INPUT_TEXT;
    INPUT_TEXT.lock().unwrap().1 = Some(text.to_string());
}

/// Credentials obtained from the native HYKB (好游快爆) login SDK.
pub struct HykbCredential {
    /// SDK result code: 0 on success, otherwise an error / user cancellation.
    pub code: i32,
    pub uid: i64,
    pub nick: String,
    pub access_token: String,
}

impl HykbCredential {
    /// Map the SDK result code to an error, or yield the credential on success.
    /// Centralizes the code → user-facing message translation shared by every
    /// HYKB login/bind entry point.
    #[cfg(feature = "hykb")]
    pub fn ok_or_err(self) -> Result<Self> {
        if self.code == 0 {
            Ok(self)
        } else {
            // A non-zero code is any failure the HYKB SDK reports: 2001 auth
            // failed, 2002 login failed, 2003 cancelled, 2004 exception, 2005
            // developer-requested exit / account logout. A HYKB build mandates a
            // valid, matching HYKB session, so every one of these must tear the
            // in-game session down — otherwise cancelling the HYKB prompt during
            // a silent re-verify would leave the player signed in and bypass the
            // gate entirely.
            force_logout();
            anyhow::bail!("{}", crate::ttl!("hykb-login-cancelled"))
        }
    }
}

/// Slot for the pending HYKB login result. The native callback fulfills it.
static HYKB_TX: Mutex<Option<tokio::sync::oneshot::Sender<HykbCredential>>> = Mutex::new(None);

/// Call a no-arg `void` method on the Android host activity (the HYKB shell).
#[cfg(all(target_os = "android", feature = "hykb"))]
fn call_activity_void(method: &'static jni::strings::JNIStr) {
    use jni::{jni_sig, objects::JObject, vm::JavaVM};

    JavaVM::singleton()
        .unwrap()
        .attach_current_thread(|env| -> jni::errors::Result<()> {
            let ctx = unsafe { JObject::from_raw(env, ndk_context::android_context().context() as _) };
            env.call_method(ctx, method, jni_sig!("()V"), &[])?;
            Ok(())
        })
        .unwrap();
}

/// Ask the Android shell to pop the HYKB account picker (`MainActivity.hykbSwitchAccount`).
/// Used by the explicit login / switch-account flow.
#[cfg(all(target_os = "android", feature = "hykb"))]
fn request_hykb_login() {
    call_activity_void(jni::jni_str!("hykbSwitchAccount"));
}

#[cfg(not(all(target_os = "android", feature = "hykb")))]
fn request_hykb_login() {}

/// Ask the Android shell to sign in using the cached HYKB account without
/// popping the picker (`MainActivity.hykbLogin`). The credentials the SDK
/// reports flow back through `HYKB_TX`, so the caller can verify them against
/// the restored Phira session. Used by the silent startup restore.
#[cfg(all(target_os = "android", feature = "hykb"))]
fn request_hykb_login_silent() {
    call_activity_void(jni::jni_str!("hykbLogin"));
}

#[cfg(not(all(target_os = "android", feature = "hykb")))]
fn request_hykb_login_silent() {}

/// Ask the Android shell to start the GTS anti-addiction for an email session
/// under the game's own server user id (`MainActivity.hykbStartFcm`). This
/// replaces the forced silent HYKB login that used to back the email path,
/// keeping that path independent of any HYKB account. Fire-and-forget: the
/// GTS listener exposes no "started" signal, so failures surface asynchronously
/// through `hykbFcmCallback` and tear the session down there.
#[cfg(all(target_os = "android", feature = "hykb"))]
pub fn request_hykb_fcm(uid: &str) {
    use jni::{jni_sig, objects::JObject, vm::JavaVM};

    JavaVM::singleton()
        .unwrap()
        .attach_current_thread(|env| -> jni::errors::Result<()> {
            let ctx = unsafe { JObject::from_raw(env, ndk_context::android_context().context() as _) };
            let uid = env.new_string(uid)?;
            env.call_method(ctx, jni::jni_str!("hykbStartFcm"), jni_sig!("(Ljava/lang/String;)V"), &[(&uid).into()])?;
            Ok(())
        })
        .unwrap();
}

#[cfg(not(all(target_os = "android", feature = "hykb")))]
pub fn request_hykb_fcm(_uid: &str) {}

/// Stop the GTS anti-addiction (`MainActivity.hykbStopFcm`). Called whenever a
/// HYKB session takes over (its own anti-addiction then applies) or the
/// session ends.
#[cfg(all(target_os = "android", feature = "hykb"))]
pub fn request_hykb_fcm_stop() {
    call_activity_void(jni::jni_str!("hykbStopFcm"));
}

#[cfg(not(all(target_os = "android", feature = "hykb")))]
pub fn request_hykb_fcm_stop() {}

/// Tell the native HYKB SDK to sign out (`MainActivity.hykbLogout`). Called when the
/// player logs out from their profile.
#[cfg(all(target_os = "android", feature = "hykb"))]
pub fn hykb_logout() {
    call_activity_void(jni::jni_str!("hykbLogout"));
}

#[cfg(not(all(target_os = "android", feature = "hykb")))]
pub fn hykb_logout() {}

/// Tear down the local session: sign out of the native HYKB SDK, clear the
/// stored account and tokens, then re-sync. Shared by every path that must
/// reject a login — a failed/cancelled HYKB verification, a uid mismatch, or
/// the player logging out from their profile.
pub fn force_logout() {
    hykb_logout();
    request_hykb_fcm_stop();
    client::clear_session();
    sync_data();
}

/// Trigger the native HYKB login and await its credentials.
#[allow(unused)]
pub async fn obtain_hykb_credential() -> Result<HykbCredential> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    *HYKB_TX.lock().unwrap() = Some(tx);
    request_hykb_login();
    let cred = rx.await.map_err(|_| anyhow::anyhow!("hykb login cancelled"))?;
    Ok(cred)
}

/// Silently restore the HYKB session from the cached account and await its
/// credentials. Unlike [`obtain_hykb_credential`], this does not pop the account
/// picker; used by the blocking startup check to verify the restored session.
#[allow(unused)]
pub async fn obtain_hykb_credential_silent() -> Result<HykbCredential> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    *HYKB_TX.lock().unwrap() = Some(tx);
    request_hykb_login_silent();
    let cred = rx.await.map_err(|_| anyhow::anyhow!("hykb login cancelled"))?;
    Ok(cred)
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_quad_1native_QuadNative_hykbLoginCallback(
    _env: EnvUnowned,
    _class: JClass,
    code: jint,
    uid: jni::sys::jlong,
    nick: JString,
    access_token: JString,
) {
    let nick = if nick.is_null() { String::new() } else { nick.to_string() };
    let access_token = if access_token.is_null() {
        String::new()
    } else {
        access_token.to_string()
    };
    if let Some(tx) = HYKB_TX.lock().unwrap().take() {
        let _ = tx.send(HykbCredential {
            code,
            uid,
            nick,
            access_token,
        });
    } else if code == 2005 {
        // No login is in flight, so this is the SDK's asynchronous
        // anti-addiction "exit game" action: the player hit a play-time limit
        // and chose to quit from the SDK's own dialog. Honor it by exiting.
        // Other async codes (e.g. 2008 "continue playing") are handled inside
        // the SDK and need no response here. A request-less success (code 0, the
        // SDK switching accounts on its own) is likewise ignored: any signed-in
        // HYKB account is accepted, so a switch no longer tears the session down.
    }
}

/// GTS anti-addiction events for the email session path (no login in flight).
/// Besides the SDK listener events (2005 exit, -1 switch account, other =
/// start failure), the Kotlin bridge sends -3 when GtsCore itself failed to
/// initialize while an email session was waiting for coverage — typically a
/// weak network at startup. Every non-2005 code tears the session back down,
/// which is how a GTS failure blocks the email path.
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_quad_1native_QuadNative_hykbFcmCallback(_env: EnvUnowned, _class: JClass, code: jint) {
    match code {
        2005 => {
            // The player hit a play-time limit and tapped "exit game" on the
            // GTS dialog: the same terminal action as the login SDK's 2005.
            std::process::exit(0);
        }
        -1 => {
            // "Switch account" on the GTS dialog: drop the email session so
            // the forced login panel comes back.
            force_logout();
        }
        -3 => {
            // GtsCore failed to initialize while a session was waiting for
            // coverage: fail closed — drop the session instead of leaving it
            // unguarded. The login panel returns; retrying the login re-kicks
            // the SDK init (HykbAuth.startFcm → initGts).
            warn!("GTS SDK init failed, tearing the email session down");
            force_logout();
        }
        code => {
            warn!("GTS anti-addiction failed with code {code}, tearing the email session down");
            force_logout();
        }
    }
}

#[cfg(target_env = "ohos")]
#[napi]
pub fn set_input_text(text: String) {
    use prpr::scene::INPUT_TEXT;
    INPUT_TEXT.lock().unwrap().1 = Some(text);
}

#[cfg(target_env = "ohos")]
#[napi]
pub fn set_chosen_file(file: String) {
    use prpr::scene::CHOSEN_FILE;
    CHOSEN_FILE.lock().unwrap().1 = Some(file);
}

#[cfg(target_env = "ohos")]
#[napi]
pub fn mark_auto_import() {
    use prpr::scene::CHOSEN_FILE;
    CHOSEN_FILE.lock().unwrap().0 = Some("_import_auto".to_owned());
}

#[cfg(target_env = "ohos")]
#[napi]
pub fn on_foreground() {
    if let Some(tx) = MESSAGES_TX.lock().unwrap().as_mut() {
        let _ = tx.send(false);
    }
}

#[cfg(target_env = "ohos")]
#[napi]
pub fn on_background() {
    if let Some(tx) = MESSAGES_TX.lock().unwrap().as_mut() {
        let _ = tx.send(true);
    }
}
