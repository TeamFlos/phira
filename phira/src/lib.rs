prpr_l10n::tl_file!("common" ttl crate::);

#[rustfmt::skip]
#[cfg(feature = "closed")]
mod inner;

mod anim;
mod charts_view;
mod client;
mod data;
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
    scene::{show_error, show_message},
    time::TimeManager,
    ui::{FontArc, TextPainter},
    Main,
};
use prpr_l10n::{set_prefered_locale, GLOBAL, LANGS};
use scene::MainScene;
use std::sync::{mpsc, Mutex};
use tracing::{error, info};

static MESSAGES_TX: Mutex<Option<mpsc::Sender<bool>>> = Mutex::new(None);
static AA_TX: Mutex<Option<mpsc::Sender<i32>>> = Mutex::new(None);
static DATA_PATH: Mutex<Option<String>> = Mutex::new(None);
static CACHE_DIR: Mutex<Option<String>> = Mutex::new(None);
pub static mut DATA: Option<Data> = None;

#[cfg(feature = "closed")]
pub async fn load_res(name: &str) -> Vec<u8> {
    let bytes = load_file(name).await.unwrap();
    inner::resolve_data(bytes)
}

#[allow(unused)]
pub async fn load_res_tex(name: &str) -> SafeTexture {
    #[cfg(feature = "closed")]
    {
        let bytes = load_res(name).await;
        let image = image::load_from_memory(&bytes).unwrap();
        image.into()
    }
    #[cfg(not(feature = "closed"))]
    prpr::ext::BLACK_TEXTURE.clone()
}

pub fn sync_data() {
    set_prefered_locale(get_data().language.as_ref().and_then(|it| it.parse().ok()));
    if get_data().language.is_none() {
        get_data_mut().language = Some(LANGS[GLOBAL.order.lock().unwrap()[0]].to_owned());
    }
    let _ = client::set_access_token_sync(get_data().tokens.as_ref().map(|it| &*it.0));
}

pub fn set_data(data: Data) {
    unsafe {
        DATA = Some(data);
    }
}

pub fn get_data() -> &'static Data {
    unsafe { DATA.as_ref().unwrap() }
}

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
    log::register();

    init_assets();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap();
    let _guard = rt.enter();

    #[cfg(target_os = "ios")]
    unsafe {
        use prpr::objc::*;
        #[allow(improper_ctypes)]
        extern "C" {
            pub fn NSSearchPathForDirectoriesInDomains(
                directory: std::os::raw::c_ulong,
                domain_mask: std::os::raw::c_ulong,
                expand_tilde: bool,
            ) -> *mut NSArray<*mut NSString>;
        }
        let directories = NSSearchPathForDirectoriesInDomains(5, 1, true);
        let first: &mut NSString = msg_send![directories, firstObject];
        let path = first.as_str().to_owned();
        *DATA_PATH.lock().unwrap() = Some(path);
        *CACHE_DIR.lock().unwrap() = Some("Caches".to_owned());
    }

    let dir = dir::root()?;
    let mut data: Data = std::fs::read_to_string(format!("{dir}/data.json"))
        .map_err(anyhow::Error::new)
        .and_then(|s| Ok(serde_json::from_str(&s)?))
        .unwrap_or_default();
    data.init().await?;
    set_data(data);
    sync_data();

    let rx = {
        let (tx, rx) = mpsc::channel();
        *MESSAGES_TX.lock().unwrap() = Some(tx);
        rx
    };

    let aa_rx = {
        let (tx, rx) = mpsc::channel();
        *AA_TX.lock().unwrap() = Some(tx);
        rx
    };

    unsafe { get_internal_gl() }
        .quad_context
        .display_mut()
        .set_pause_resume_listener(on_pause_resume);

    if let Some(me) = &get_data().me {
        anti_addiction_action("startup", Some(format!("phira-{}", me.id)));
    }

    let pgr_font = FontArc::try_from_vec(load_file("phigros.ttf").await?)?;
    PGR_FONT.with(move |it| *it.borrow_mut() = Some(TextPainter::new(pgr_font, None)));

    let font = FontArc::try_from_vec(load_file("font.ttf").await?)?;
    let mut painter = TextPainter::new(font.clone(), None);

    let mut main = Main::new(Box::new(MainScene::new(font).await?), TimeManager::default(), None).await?;

    let tm = TimeManager::default();
    let mut fps_time = -1;

    let mut exit_time = f64::INFINITY;

    'app: loop {
        let frame_start = tm.real_time();
        let res = || -> Result<()> {
            main.update()?;
            main.render(&mut painter)?;
            if let Ok(paused) = rx.try_recv() {
                if paused {
                    main.pause()?;
                } else {
                    main.resume()?;
                }
            }
            Ok(())
        }();
        if let Err(err) = res {
            error!("uncaught error: {err:?}");
            show_error(err);
        }
        if main.should_exit() {
            break 'app;
        }

        if let Ok(code) = aa_rx.try_recv() {
            info!("anti addiction callback: {code}");
            match code {
                // login success
                500 => {
                    anti_addiction_action("enterGame", None);
                }
                // switch account
                1001 => {
                    anti_addiction_action("exit", None);
                    get_data_mut().me = None;
                    get_data_mut().tokens = None;
                    let _ = save_data();
                    sync_data();
                    use crate::login::L10N_LOCAL;
                    show_message(crate::login::tl!("logged-out")).ok();
                }
                // period restrict
                1030 => {
                    show_and_exit("你当前为未成年账号，已被纳入防沉迷系统。根据国家相关规定，周五、周六、周日及法定节假日 20 点 - 21 点之外为健康保护时段。当前时间段无法游玩，请合理安排时间。");
                    exit_time = frame_start;
                }
                // duration limit
                1050 => {
                    show_and_exit("你当前为未成年账号，已被纳入防沉迷系统。根据国家相关规定，周五、周六、周日及法定节假日 20 点 - 21 点之外为健康保护时段。你已达时间限制，无法继续游戏。");
                    exit_time = frame_start;
                }
                // stopped
                9002 => {
                    show_and_exit("必须实名认证方可进行游戏。");
                    exit_time = frame_start;
                }
                _ => {}
            }
        }

        let t = tm.real_time();

        if t > exit_time + 5. {
            break;
        }

        let fps_now = t as i32;
        if fps_now != fps_time {
            fps_time = fps_now;
            info!("FPS {}", (1. / (t - frame_start)) as u32);
        }

        #[cfg(target_os = "windows")]
        macroquad::window::set_fullscreen(get_data().config.fullscreen_mode);

        next_frame().await;
    }
    Ok(())
}

fn show_and_exit(msg: &str) {
    prpr::ui::Dialog::simple(msg)
        .buttons(vec!["确定".to_owned()])
        .listener(|_, _| std::process::exit(0))
        .show();
}

#[no_mangle]
pub extern "C" fn quad_main() {
    macroquad::Window::from_config(build_conf(), async {
        if let Err(err) = the_main().await {
            error!(?err, "global error");
        }
    });
}

fn on_pause_resume(pause: bool) {
    if let Some(tx) = MESSAGES_TX.lock().unwrap().as_mut() {
        let _ = tx.send(pause);
    }
}

#[cfg(target_os = "android")]
unsafe fn string_from_java(env: *mut ndk_sys::JNIEnv, s: ndk_sys::jstring) -> String {
    let get_string_utf_chars = (**env).GetStringUTFChars.unwrap();
    let release_string_utf_chars = (**env).ReleaseStringUTFChars.unwrap();

    let ptr = (get_string_utf_chars)(env, s, ::std::ptr::null::<ndk_sys::jboolean>() as _);
    let res = std::ffi::CStr::from_ptr(ptr).to_str().unwrap().to_owned();
    (release_string_utf_chars)(env, s, ptr);

    res
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_quad_1native_QuadNative_prprActivityOnPause(_: *mut std::ffi::c_void, _: *const std::ffi::c_void) {
    anti_addiction_action("leaveGame", None);
    if let Some(tx) = MESSAGES_TX.lock().unwrap().as_mut() {
        let _ = tx.send(true);
    }
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_quad_1native_QuadNative_prprActivityOnResume(_: *mut std::ffi::c_void, _: *const std::ffi::c_void) {
    anti_addiction_action("enterGame", None);
    if let Some(tx) = MESSAGES_TX.lock().unwrap().as_mut() {
        let _ = tx.send(false);
    }
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_quad_1native_QuadNative_prprActivityOnDestroy(_: *mut std::ffi::c_void, _: *const std::ffi::c_void) {
    // std::process::exit(0);
}

#[cfg(target_os = "android")]
#[no_mangle]
pub unsafe extern "C" fn Java_quad_1native_QuadNative_setDataPath(env: *mut ndk_sys::JNIEnv, _: *const std::ffi::c_void, path: ndk_sys::jstring) {
    *DATA_PATH.lock().unwrap() = Some(string_from_java(env, path));
}

#[cfg(target_os = "android")]
#[no_mangle]
pub unsafe extern "C" fn Java_quad_1native_QuadNative_setTempDir(env: *mut ndk_sys::JNIEnv, _: *const std::ffi::c_void, path: ndk_sys::jstring) {
    let path = string_from_java(env, path);
    std::env::set_var("TMPDIR", path.clone());
    *CACHE_DIR.lock().unwrap() = Some(path);
}

#[cfg(target_os = "android")]
#[no_mangle]
pub unsafe extern "C" fn Java_quad_1native_QuadNative_setDpi(_: *mut std::ffi::c_void, _: *const std::ffi::c_void, dpi: ndk_sys::jint) {
    prpr::core::DPI_VALUE.store(dpi as _, std::sync::atomic::Ordering::SeqCst);
}

#[cfg(target_os = "android")]
#[no_mangle]
pub unsafe extern "C" fn Java_quad_1native_QuadNative_setChosenFile(env: *mut ndk_sys::JNIEnv, _: *const std::ffi::c_void, file: ndk_sys::jstring) {
    use prpr::scene::CHOSEN_FILE;
    CHOSEN_FILE.lock().unwrap().1 = Some(string_from_java(env, file));
}

#[cfg(target_os = "android")]
#[no_mangle]
pub unsafe extern "C" fn Java_quad_1native_QuadNative_markImport(_: *mut std::ffi::c_void, _: *const std::ffi::c_void) {
    use prpr::scene::CHOSEN_FILE;

    CHOSEN_FILE.lock().unwrap().0 = Some("_import".to_owned());
}

#[cfg(target_os = "android")]
#[no_mangle]
pub unsafe extern "C" fn Java_quad_1native_QuadNative_markImportRespack(_: *mut std::ffi::c_void, _: *const std::ffi::c_void) {
    use prpr::scene::CHOSEN_FILE;

    CHOSEN_FILE.lock().unwrap().0 = Some("_import_respack".to_owned());
}

#[cfg(target_os = "android")]
#[no_mangle]
pub unsafe extern "C" fn Java_quad_1native_QuadNative_setInputText(env: *mut ndk_sys::JNIEnv, _: *const std::ffi::c_void, text: ndk_sys::jstring) {
    use prpr::scene::INPUT_TEXT;
    INPUT_TEXT.lock().unwrap().1 = Some(string_from_java(env, text));
}

#[cfg(not(all(target_os = "android", feature = "aa")))]
pub fn anti_addiction_action(_action: &str, _arg: Option<String>) {}

#[cfg(all(target_os = "android", feature = "aa"))]
pub fn anti_addiction_action(action: &str, arg: Option<String>) {
    unsafe {
        let env = miniquad::native::attach_jni_env();
        let ctx = ndk_context::android_context().context();
        let class = (**env).GetObjectClass.unwrap()(env, ctx);
        let method =
            (**env).GetMethodID.unwrap()(env, class, b"antiAddiction\0".as_ptr() as _, b"(Ljava/lang/String;Ljava/lang/String;)V\0".as_ptr() as _);
        let action = std::ffi::CString::new(action.to_owned()).unwrap();
        let arg = arg.map(|it| std::ffi::CString::new(it).unwrap());
        (**env).CallVoidMethod.unwrap()(
            env,
            ctx,
            method,
            (**env).NewStringUTF.unwrap()(env, action.as_ptr()),
            arg.map(|it| (**env).NewStringUTF.unwrap()(env, it.as_ptr()))
                .unwrap_or_else(|| std::ptr::null_mut()),
        );
    }
}

#[cfg(target_os = "android")]
#[no_mangle]
pub unsafe extern "C" fn Java_quad_1native_QuadNative_antiAddictionCallback(
    _: *mut std::ffi::c_void,
    _: *const std::ffi::c_void,
    #[allow(dead_code)] code: ndk_sys::jint,
) {
    if cfg!(feature = "aa") {
        if let Some(tx) = AA_TX.lock().unwrap().as_mut() {
            let _ = tx.send(code);
        }
    }
}
