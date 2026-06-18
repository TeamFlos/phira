//! Scene management module.
#![allow(unused_macros)]

prpr_l10n::tl_file!("scene" ttl);

mod ending;
pub use ending::{EndingScene, RecordUpdateState};

mod game;
pub use game::{GameMode, GameScene, SimpleRecord};

mod loading;
pub use loading::{BasicPlayer, LoadingScene, SaveFn, UpdateFn, UploadFn};

use crate::{
    ext::{draw_image, screen_aspect, LocalTask, SafeTexture, ScaleType},
    judge::Judge,
    time::TimeManager,
    ui::{BillBoard, Dialog, Message, MessageHandle, MessageKind, TextPainter, Ui},
};
use anyhow::{Error, Result};
use cfg_if::cfg_if;
use inputbox::{
    backend::{default_backend, Backend},
    InputBox,
};
use macroquad::prelude::*;
use std::{
    any::Any,
    borrow::Cow,
    cell::RefCell,
    sync::{Arc, Mutex},
};
use tracing::warn;

#[derive(Default)]
pub enum NextScene {
    #[default]
    None,
    Pop,
    PopN(usize),
    PopWithResult(Box<dyn Any>),
    PopNWithResult(usize, Box<dyn Any>),
    Exit,
    Overlay(Box<dyn Scene>),
    Replace(Box<dyn Scene>),
}

thread_local! {
    pub static BILLBOARD: RefCell<(BillBoard, TimeManager)> = RefCell::new((BillBoard::new(), TimeManager::default()));
    pub static DIALOG: RefCell<Option<Dialog>> = const { RefCell::new(None) };
    pub static FULL_LOADING: RefCell<Option<FullLoadingView>> = const { RefCell::new(None) };
}

pub struct FullLoadingView {
    keep_alive: Arc<()>,
    text: Option<Cow<'static, str>>,
}

impl FullLoadingView {
    pub fn begin() -> Arc<()> {
        Self::begin_inner(None)
    }
    pub fn begin_text(text: Cow<'static, str>) -> Arc<()> {
        Self::begin_inner(Some(text))
    }
    fn begin_inner(text: Option<Cow<'static, str>>) -> Arc<()> {
        let arc = Arc::new(());
        let ret = arc.clone();
        FULL_LOADING.replace(Some(Self { keep_alive: arc, text }));
        ret
    }
}

#[inline]
pub fn show_error(error: Error) {
    warn!("show error: {error:?}");
    Dialog::error(error).show();
}

pub struct MessageBuilder {
    content: String,
    kind: MessageKind,
    duration: f32,
}

impl MessageBuilder {
    pub fn new(content: String) -> Self {
        Self {
            content,
            kind: MessageKind::Info,
            duration: 2.,
        }
    }

    #[inline]
    pub fn kind(mut self, kind: MessageKind) -> Self {
        self.kind = kind;
        self
    }

    #[inline]
    pub fn duration(mut self, t: f32) -> Self {
        self.duration = t;
        self
    }

    #[inline]
    pub fn ok(self) -> Self {
        self.kind(MessageKind::Ok)
    }

    #[inline]
    pub fn warn(self) -> Self {
        self.kind(MessageKind::Warn)
    }

    #[inline]
    pub fn error(self) -> Self {
        self.kind(MessageKind::Error)
    }

    fn show(&mut self) -> MessageHandle {
        BILLBOARD.with(|it| {
            let mut guard = it.borrow_mut();
            let (msg, handle) = Message::new(std::mem::take(&mut self.content), guard.1.now() as _, self.duration, self.kind.clone());
            guard.0.add(msg);
            handle
        })
    }

    #[inline]
    pub fn handle(mut self) -> MessageHandle {
        let handle = self.show();
        std::mem::forget(self);
        handle
    }
}

impl Drop for MessageBuilder {
    fn drop(&mut self) {
        self.show();
    }
}

#[inline]
pub fn show_message(msg: impl Into<String>) -> MessageBuilder {
    MessageBuilder::new(msg.into())
}

pub static INPUT_TEXT: Mutex<(Option<String>, Option<String>)> = Mutex::new((None, None));
#[cfg(not(target_arch = "wasm32"))]
pub static CHOSEN_FILE: Mutex<(Option<String>, Option<String>)> = Mutex::new((None, None));

fn show_inputbox(config: InputBox, backend: &dyn Backend) {
    let result = config.show_with_async(backend, |result| match result {
        Ok(Some(text)) => {
            INPUT_TEXT.lock().unwrap().1 = Some(text);
        }
        Ok(None) => {}
        Err(err) => {
            warn!(?err, "failed to get input");
        }
    });
    if let Err(err) = result {
        warn!(?err, "failed to show input box");
    }
}

#[inline]
pub fn request_input(id: impl Into<String>, mut config: InputBox) {
    *INPUT_TEXT.lock().unwrap() = (Some(id.into()), None);
    if config.title.is_none() {
        config = config.title(ttl!("input"));
    }
    if config.prompt.is_none() {
        config = config.prompt(ttl!("input-msg"));
    }
    if config.cancel_label.is_none() {
        config = config.cancel_label(ttl!("cancel"));
    }
    if config.ok_label.is_none() {
        config = config.ok_label(ttl!("confirm"));
    }
    show_inputbox(config, &*default_backend());
}

pub fn take_input() -> Option<(String, String)> {
    let mut w = INPUT_TEXT.lock().unwrap();
    w.0.clone().zip(std::mem::take(&mut w.1))
}

pub fn return_input(id: String, text: String) {
    *INPUT_TEXT.lock().unwrap() = (Some(id), Some(text));
}

#[cfg(not(target_arch = "wasm32"))]
pub fn request_file(id: impl Into<String>) {
    let id: String = id.into();
    #[cfg(target_env = "ohos")]
    let is_photo = id == "avatar";
    *CHOSEN_FILE.lock().unwrap() = (Some(id), None);
    cfg_if! {
        if #[cfg(target_os = "android")] {
            unsafe {
                let env = miniquad::native::attach_jni_env();
                let ctx = ndk_context::android_context().context();
                let class = (**env).GetObjectClass.unwrap()(env, ctx);
                let method = (**env).GetMethodID.unwrap()(env, class, c"chooseFile".as_ptr() as _, c"()V".as_ptr() as _);
                (**env).CallVoidMethod.unwrap()(env, ctx, method);
            }
        } else if #[cfg(target_os = "ios")] {
            use objc2::{available, define_class, rc::Retained, runtime::ProtocolObject, MainThreadMarker, MainThreadOnly};
            use objc2_foundation::{NSArray, NSObject, NSObjectProtocol, NSString, NSURL};
            use objc2_ui_kit::{UIDocumentPickerDelegate, UIDocumentPickerViewController};

            thread_local! {
                static DELEGATE: RefCell<Option<Retained<PickerDelegate>>> = const { RefCell::new(None) };
            }

            define_class! {
                // SAFETY:
                // - The superclass NSObject does not have any subclassing requirements.
                // - `Delegate` does not implement `Drop`.
                #[unsafe(super = NSObject)]
                #[thread_kind = MainThreadOnly]
                struct PickerDelegate;

                // SAFETY: `NSObjectProtocol` has no safety requirements.
                unsafe impl NSObjectProtocol for PickerDelegate {}

                // SAFETY: `UIDocumentPickerDelegate` has no safety requirements.
                unsafe impl UIDocumentPickerDelegate for PickerDelegate {
                    // SAFETY: The signature is correct.
                    #[unsafe(method(documentPicker:didPickDocumentsAtURLs:))]
                    fn did_pick_documents_at_urls(&self, controller: &UIDocumentPickerViewController, urls: &NSArray<NSURL>) {
                        use objc2_foundation::{NSData, NSDataReadingOptions, NSTemporaryDirectory};

                        let url = urls.firstObject().unwrap();
                        let need_close = unsafe { url.startAccessingSecurityScopedResource() };

                        let data = match NSData::dataWithContentsOfURL_options_error(&url, NSDataReadingOptions::Uncached) {
                            Ok(data) => data,
                            Err(err) => {
                                let message = err.localizedDescription().to_string();
                                show_error(Error::msg(message).context(ttl!("read-file-failed")));
                                return;
                            }
                        };
                        if need_close {
                            unsafe { url.stopAccessingSecurityScopedResource() };
                        }

                        let dir = NSTemporaryDirectory();
                        let path = format!("{}{}", dir, uuid::Uuid::new_v4());
                        data.writeToFile_atomically(&NSString::from_str(&path), true);
                        CHOSEN_FILE.lock().unwrap().1 = Some(path);
                    }
                }
            }

            impl PickerDelegate {
                fn new(mtm: MainThreadMarker) -> Retained<Self> {
                    let this = Self::alloc(mtm).set_ivars(());
                    unsafe { objc2::msg_send![super(this), init] }
                }
            }

            let mtm = MainThreadMarker::new().unwrap();

            let picker = UIDocumentPickerViewController::alloc(mtm);
            let picker = if available!(ios = 14.0.0) {
                use objc2_uniform_type_identifiers::UTType;

                let ext = |e: &str| UTType::typeWithFilenameExtension(&NSString::from_str(e)).unwrap();
                let types = NSArray::from_retained_slice(&[
                    ext("zip"),
                    ext("pez"),
                    ext("jpg"),
                    ext("png"),
                    ext("jpeg"),
                    ext("json"),
                    ext("mp3"),
                    ext("ogg"),
                ]);
                UIDocumentPickerViewController::initForOpeningContentTypes(picker, &types)
            } else {
                #[allow(deprecated)]
                {
                    use objc2_ui_kit::UIDocumentPickerMode;

                    let ext = NSString::from_str;
                    let types = NSArray::from_retained_slice(&[ext("public.image"), ext("public.archive")]);
                    UIDocumentPickerViewController::initWithDocumentTypes_inMode(picker, &types, UIDocumentPickerMode::Import)
                }
            };
            let dlg_obj = PickerDelegate::new(mtm);
            picker.setDelegate(Some(ProtocolObject::from_ref(&*dlg_obj)));
            DELEGATE.with(|it| *it.borrow_mut() = Some(dlg_obj));

            inputbox::backend::IOS::get_top_view_controller(mtm)
                .unwrap()
                .presentViewController_animated_completion(&picker, true, None);
        } else if #[cfg(target_env = "ohos")] {
            miniquad::native::call_request_callback(format!(r#"{{"action": "chooseFile", "isPhoto": {}}}"#, is_photo));
        } else { // desktop
            CHOSEN_FILE.lock().unwrap().1 = rfd::FileDialog::new().pick_file().map(|it| it.display().to_string());
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn take_file() -> Option<(String, String)> {
    let mut w = CHOSEN_FILE.lock().unwrap();
    w.0.clone().zip(std::mem::take(&mut w.1))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn return_file(id: String, file: String) {
    *CHOSEN_FILE.lock().unwrap() = (Some(id), Some(file));
}

pub trait Scene {
    fn enter(&mut self, _tm: &mut TimeManager, _target: Option<RenderTarget>) -> Result<()> {
        Ok(())
    }
    fn pause(&mut self, _tm: &mut TimeManager) -> Result<()> {
        Ok(())
    }
    fn resume(&mut self, _tm: &mut TimeManager) -> Result<()> {
        Ok(())
    }
    fn on_result(&mut self, _tm: &mut TimeManager, _result: Box<dyn Any>) -> Result<()> {
        Ok(())
    }
    fn touch(&mut self, _tm: &mut TimeManager, _touch: &Touch) -> Result<bool> {
        Ok(false)
    }
    fn update(&mut self, tm: &mut TimeManager) -> Result<()>;
    fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()>;
    fn next_scene(&mut self, _tm: &mut TimeManager) -> NextScene {
        NextScene::None
    }
}

pub trait RenderTargetChooser {
    fn choose(&mut self) -> Option<RenderTarget>;
}
impl RenderTargetChooser for Option<RenderTarget> {
    fn choose(&mut self) -> Option<RenderTarget> {
        *self
    }
}
impl<F: FnMut() -> Option<RenderTarget>> RenderTargetChooser for F {
    fn choose(&mut self) -> Option<RenderTarget> {
        self()
    }
}

pub struct Main {
    pub scenes: Vec<Box<dyn Scene>>,
    times: Vec<f64>,
    target_chooser: Box<dyn RenderTargetChooser>,
    tm: TimeManager,
    paused: bool,
    last_update_time: f64,
    should_exit: bool,
    pub top_level: bool,
    touches: Option<Vec<Touch>>,
    pub viewport: Option<(i32, i32, i32, i32)>,
}

impl Main {
    pub async fn new(mut scene: Box<dyn Scene>, mut tm: TimeManager, mut target_chooser: impl RenderTargetChooser + 'static) -> Result<Self> {
        simulate_mouse_with_touch(false);
        scene.enter(&mut tm, target_chooser.choose())?;
        let last_update_time = tm.now();
        macro_rules! load_tex {
            ($path:literal) => {
                SafeTexture::from(Texture2D::from_image(&load_image($path).await?))
            };
        }
        let icons = [load_tex!("info.png"), load_tex!("warn.png"), load_tex!("ok.png"), load_tex!("error.png")];
        BILLBOARD.with(|it| it.borrow_mut().0.set_icons(icons));
        Ok(Self {
            scenes: vec![scene],
            times: Vec::new(),
            target_chooser: Box::new(target_chooser),
            tm,
            paused: false,
            last_update_time,
            should_exit: false,
            top_level: true,
            touches: None,
            viewport: None,
        })
    }

    pub fn update(&mut self) -> Result<()> {
        self.update_with_mutate(|_| {})
    }

    pub fn update_with_mutate(&mut self, f: impl Fn(&mut Touch)) -> Result<()> {
        if self.paused {
            return Ok(());
        }
        match self.scenes.last_mut().unwrap().next_scene(&mut self.tm) {
            NextScene::None => {}
            NextScene::Pop => {
                self.scenes.pop();
                self.tm.seek_to(self.times.pop().unwrap());
                self.scenes.last_mut().unwrap().enter(&mut self.tm, self.target_chooser.choose())?;
            }
            NextScene::PopN(num) => {
                for _ in 0..num {
                    self.scenes.pop();
                    self.tm.seek_to(self.times.pop().unwrap());
                }
                self.scenes.last_mut().unwrap().enter(&mut self.tm, self.target_chooser.choose())?;
            }
            NextScene::PopWithResult(result) => {
                self.scenes.pop();
                self.tm.seek_to(self.times.pop().unwrap());
                self.scenes.last_mut().unwrap().on_result(&mut self.tm, result)?;
                self.scenes.last_mut().unwrap().enter(&mut self.tm, self.target_chooser.choose())?;
            }
            NextScene::PopNWithResult(num, result) => {
                for _ in 0..num {
                    self.scenes.pop();
                    self.tm.seek_to(self.times.pop().unwrap());
                }
                self.scenes.last_mut().unwrap().on_result(&mut self.tm, result)?;
                self.scenes.last_mut().unwrap().enter(&mut self.tm, self.target_chooser.choose())?;
            }
            NextScene::Exit => {
                self.should_exit = true;
            }
            NextScene::Overlay(mut scene) => {
                self.times.push(self.tm.now());
                scene.enter(&mut self.tm, self.target_chooser.choose())?;
                self.scenes.push(scene);
            }
            NextScene::Replace(mut scene) => {
                scene.enter(&mut self.tm, self.target_chooser.choose())?;
                *self.scenes.last_mut().unwrap() = scene;
            }
        }
        Judge::on_new_frame();
        let mut touches = Judge::get_touches();
        touches.iter_mut().for_each(f);
        if !(touches.is_empty() || FULL_LOADING.with(|it| it.borrow().is_some())) {
            let now = self.tm.now();
            let delta = (now - self.last_update_time) / touches.len() as f64;
            let start_time = self.tm.start_time;
            let mut last_err = None;
            DIALOG.with(|it| -> Result<()> {
                let mut index = 1;
                touches.retain_mut(|touch| {
                    let t = self.last_update_time + (index + 1) as f64 * delta;
                    index += 1;
                    let mut guard = it.borrow_mut();
                    if let Some(dialog) = guard.as_mut() {
                        if !dialog.touch(touch, t as _) {
                            drop(guard);
                            *it.borrow_mut() = None;
                        }
                        false
                    } else {
                        drop(guard);
                        self.tm.seek_to(t);
                        match self.scenes.last_mut().unwrap().touch(&mut self.tm, touch) {
                            Ok(val) => !val,
                            Err(err) => {
                                warn!(?err, "failed to handle touch");
                                last_err = Some(err);
                                false
                            }
                        }
                    }
                });
                Ok(())
            })?;
            if let Some(err) = last_err {
                return Err(err);
            }
            self.tm.start_time = start_time;
        }
        self.touches = Some(touches);
        self.last_update_time = self.tm.now();
        DIALOG.with(|it| {
            if let Some(dialog) = it.borrow_mut().as_mut() {
                dialog.update(self.last_update_time as _);
            }
        });
        self.scenes.last_mut().unwrap().update(&mut self.tm)?;
        Ok(())
    }

    pub fn render(&mut self, painter: &mut TextPainter) -> Result<()> {
        if self.paused {
            return Ok(());
        }
        let mut ui = Ui::new(painter, self.viewport);
        ui.set_touches(self.touches.take().unwrap());
        ui.scope(|ui| self.scenes.last_mut().unwrap().render(&mut self.tm, ui))?;
        if self.top_level {
            push_camera_state();
            set_camera(&ui.camera());
            let mut gl = unsafe { get_internal_gl() };
            gl.flush();
            // gl.quad_gl.render_pass(None);
            // gl.quad_gl.viewport(None);
            BILLBOARD.with(|it| {
                let mut guard = it.borrow_mut();
                let t = guard.1.now() as f32;
                guard.0.render(&mut ui, t);
            });
            DIALOG.with(|it| {
                if let Some(dialog) = it.borrow_mut().as_mut() {
                    dialog.render(&mut ui, self.tm.now() as _);
                }
            });
            let remove = FULL_LOADING.with(|it| {
                if let Some(loading) = it.borrow_mut().as_mut() {
                    if Arc::strong_count(&loading.keep_alive) > 1 {
                        if let Some(text) = loading.text.as_ref() {
                            ui.full_loading(text.clone(), self.tm.now() as _);
                        } else {
                            ui.full_loading_simple(self.tm.now() as _);
                        }
                        return false;
                    } else {
                        return true;
                    }
                }
                false
            });
            if remove {
                FULL_LOADING.take();
            }
            pop_camera_state();
        }
        Ok(())
    }

    pub fn pause(&mut self) -> Result<()> {
        self.paused = true;
        self.scenes.last_mut().unwrap().pause(&mut self.tm)
    }

    pub fn resume(&mut self) -> Result<()> {
        self.paused = false;
        self.scenes.last_mut().unwrap().resume(&mut self.tm)
    }

    pub fn should_exit(&self) -> bool {
        self.should_exit
    }
}

fn draw_background(tex: Texture2D) {
    let asp = screen_aspect();
    let top = 1. / asp;
    draw_image(tex, Rect::new(-1., -top, 2., top * 2.), ScaleType::CropCenter);
    draw_rectangle(-1., -top, 2., top * 2., Color::new(0., 0., 0., 0.3));
}

pub type LocalSceneTask = LocalTask<Result<NextScene>>;
