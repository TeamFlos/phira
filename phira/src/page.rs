mod event;
pub use event::EventPage;

mod home;
pub use home::HomePage;

mod library;
pub use library::LibraryPage;

mod message;
pub use message::MessagePage;

mod offset;
pub use offset::OffsetPage;

mod respack;
pub use respack::{ResPackItem, ResPackPage};

mod settings;
pub use settings::SettingsPage;
use tokio::sync::Notify;

use crate::{
    client::File,
    data::BriefChartInfo,
    dir, get_data,
    images::Images,
    scene::{fs_from_path, ChartOrder},
};
use anyhow::Result;
use image::DynamicImage;
use macroquad::prelude::*;
use prpr::{
    core::{Resource, BOLD_FONT},
    ext::{semi_black, semi_white, SafeTexture, ScaleType, BLACK_TEXTURE},
    fs,
    scene::{NextScene, Scene},
    task::Task,
    time::TimeManager,
    ui::{FontArc, IntoShading, Shading, TextPainter, Ui},
};
use std::{
    any::Any,
    borrow::Cow,
    ops::DerefMut,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tracing::warn;

pub fn thumbnail_path(path: &str) -> Result<PathBuf> {
    Ok(format!("{}/{}", dir::cache_image_local()?, path.replace('/', "_")).into())
}

pub fn illustration_task(notify: Arc<Notify>, path: String, full: bool) -> Task<Result<(DynamicImage, Option<DynamicImage>)>> {
    Task::new(async move {
        notify.notified().await;
        let mut fs = fs_from_path(&path)?;
        let info = fs::load_info(fs.deref_mut()).await?;
        let mut img = None;
        let thumbnail = Images::local_or_else(thumbnail_path(&path)?, async {
            let image = image::load_from_memory(&fs.load_file(&info.illustration).await?)?;
            let thumbnail = Images::thumbnail(&image);
            img = Some(image);
            Ok(thumbnail)
        })
        .await?;
        if full {
            if img.is_none() {
                img = Some(image::load_from_memory(&fs.load_file(&info.illustration).await?)?);
            }
        } else {
            img = None;
        }
        Ok((thumbnail, img))
    })
}

pub fn local_illustration(path: String, def: SafeTexture, full: bool) -> Illustration {
    let notify = Arc::new(Notify::new());
    Illustration {
        texture: (def.clone(), def),
        notify: Arc::clone(&notify),
        task: Some(illustration_task(notify, path, full)),
        loaded: Arc::default(),
        load_time: f32::NAN,
    }
}

pub fn load_local(order: &(ChartOrder, bool)) -> Vec<ChartItem> {
    let tex = BLACK_TEXTURE.clone();
    let mut res: Vec<_> = get_data()
        .charts
        .iter()
        .map(|it| ChartItem {
            info: it.info.clone(),
            local_path: Some(it.local_path.clone()),
            illu: local_illustration(it.local_path.clone(), tex.clone(), false),
        })
        .collect();
    order.0.apply(&mut res);
    if order.1 {
        res.reverse();
    }
    res
}

#[derive(Clone)]
pub struct Illustration {
    pub texture: (SafeTexture, SafeTexture),
    pub notify: Arc<Notify>,
    pub task: Option<Task<Result<(DynamicImage, Option<DynamicImage>)>>>,
    pub loaded: Arc<Mutex<Option<(SafeTexture, SafeTexture)>>>,
    pub load_time: f32,
}

impl Illustration {
    const TIME: f32 = 0.4;

    pub fn from_file(file: File) -> Self {
        let notify = Arc::default();
        Self {
            texture: (BLACK_TEXTURE.clone(), BLACK_TEXTURE.clone()),
            notify: Arc::clone(&notify),
            task: Some(Task::new(async move {
                notify.notified().await;
                Ok((file.load_image().await?, None))
            })),
            loaded: Arc::default(),
            load_time: f32::NAN,
        }
    }

    pub fn notify(&self) {
        self.notify.notify_one();
    }

    pub fn settle(&mut self, t: f32) {
        if let Some(task) = &mut self.task {
            if let Some(illu) = task.take() {
                match illu {
                    Err(err) => {
                        warn!("failed to load illustration: {:?}", err);
                    }
                    Ok(illu) => {
                        self.texture = Images::into_texture(illu);
                    }
                };
                *self.loaded.lock().unwrap() = Some(self.texture.clone());
                self.task = None;
                self.load_time = t;
            } else if let Some(loaded) = self.loaded.lock().unwrap().clone() {
                self.texture = loaded;
                self.load_time = t;
                self.task = None;
            }
        }
    }

    pub fn alpha(&self, t: f32) -> f32 {
        if self.load_time.is_nan() {
            0.
        } else {
            ((t - self.load_time) / Self::TIME).min(1.)
        }
    }

    pub fn shading(&self, r: Rect, t: f32) -> impl Shading {
        (*self.texture.0, r, ScaleType::CropCenter, semi_white(self.alpha(t))).into_shading()
    }
}

#[derive(Clone)]
pub struct ChartItem {
    pub info: BriefChartInfo,
    pub local_path: Option<String>,
    pub illu: Illustration,
}

// srange name, isn't it?
pub struct Fader {
    pub distance: f32,
    start_time: f32,
    pub time: f32,
    index: usize,
    back: bool,
    pub sub: bool,
}

impl Fader {
    const DELTA: f32 = 0.04;

    pub fn new() -> Self {
        Self {
            distance: 0.2,
            start_time: f32::NAN,
            time: 0.7,
            index: 0,
            back: false,
            sub: false,
        }
    }

    #[inline]
    pub fn with_time(mut self, time: f32) -> Self {
        self.time = time;
        self
    }

    #[inline]
    pub fn with_distance(mut self, distance: f32) -> Self {
        self.distance = distance;
        self
    }

    #[inline]
    pub fn reset(&mut self) {
        self.index = 0;
    }

    #[inline]
    pub fn sub(&mut self, t: f32) {
        self.start_time = t;
        self.back = false;
    }

    #[inline]
    pub fn for_sub<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.sub = true;
        let res = f(self);
        self.sub = false;
        res
    }

    #[inline]
    pub fn back(&mut self, t: f32) {
        self.start_time = t;
        self.back = true;
    }

    pub fn progress_scaled(&self, t: f32, scale: f32) -> f32 {
        if self.start_time.is_nan() {
            0.
        } else {
            let p = ((t - self.start_time) / self.time * scale).clamp(0., 1.);
            let p = (1. - p).powi(3);
            let p = if self.back { p } else { 1. - p };
            if self.sub {
                1. - p
            } else {
                -p
            }
        }
    }

    pub fn progress(&self, t: f32) -> f32 {
        self.progress_scaled(t, 1.)
    }

    pub fn roll_back(&mut self) {
        self.index = self.index.saturating_sub(1);
    }

    pub fn render<R>(&mut self, ui: &mut Ui, t: f32, f: impl FnOnce(&mut Ui) -> R) -> R {
        let p = self.progress(t - self.index as f32 * Self::DELTA);
        let (dy, alpha) = (p * self.distance, 1. - p.abs());
        self.index += 1;
        ui.scope(|ui| {
            ui.dy(dy);
            ui.alpha(alpha, f)
        })
    }

    #[inline]
    pub fn transiting(&self) -> bool {
        !self.start_time.is_nan()
    }

    pub fn done(&mut self, t: f32) -> Option<bool> {
        if !self.start_time.is_nan() && t - self.start_time > self.time {
            self.start_time = f32::NAN;
            Some(self.back)
        } else {
            None
        }
    }

    pub fn render_title(&mut self, ui: &mut Ui, t: f32, s: &str) {
        let tp = ui.back_rect().center().y;
        let h = ui.text("L").size(1.2).no_baseline().measure_using(&BOLD_FONT).h;
        ui.scissor(Rect::new(-1., tp - h / 2., 2., h), |ui| {
            let p = self.progress_scaled(t, 1.6);
            let tp = tp + h * p - h / 2.;
            let mut x = -0.87;
            if s == "PHIRA" {
                x -= ui.back_rect().w;
            }
            for c in s.chars() {
                x += ui
                    .text(c.to_string())
                    .pos(x, tp)
                    .anchor(0., 0.)
                    .size(1.2)
                    .color(WHITE)
                    .draw_using(&BOLD_FONT)
                    .w
                    + 0.012;
            }
            if s == "PHIRA" {
                ui.text(concat!('v', env!("CARGO_PKG_VERSION")))
                    .pos(x + 0.01, tp + h - 0.027)
                    .anchor(0., 1.)
                    .color(semi_white(0.4))
                    .size(0.5)
                    .draw_using(&BOLD_FONT);
            }
        });
    }
}

pub struct SFader {
    time: f32,
    next_scene: Option<NextScene>,
}

impl SFader {
    const TIME: f32 = 0.35;

    pub fn new() -> Self {
        Self {
            time: f32::NAN,
            next_scene: None,
        }
    }

    pub fn transiting(&self) -> bool {
        !self.time.is_nan()
    }

    pub fn goto(&mut self, t: f32, scene: impl Scene + 'static) {
        self.time = t;
        self.next_scene = Some(NextScene::Overlay(Box::new(scene)));
    }

    pub fn next(&mut self, t: f32, next: NextScene) {
        self.time = t;
        self.next_scene = Some(next);
    }

    pub fn enter(&mut self, t: f32) {
        self.time = t;
    }

    pub fn render(&mut self, ui: &mut Ui, t: f32) {
        if self.time.is_nan() {
            return;
        }
        let p = ((t - self.time) / Self::TIME).min(1.);
        if p >= 1. && self.next_scene.is_none() {
            self.time = f32::NAN;
        } else {
            ui.fill_rect(ui.screen_rect(), semi_black(if self.next_scene.is_some() { p } else { 1. - p }));
        }
    }

    pub fn next_scene(&mut self, t: f32) -> Option<NextScene> {
        if t >= self.time + Self::TIME {
            self.next_scene.take()
        } else {
            None
        }
    }
}

pub struct SharedState {
    pub t: f32,
    pub rt: f32,
    pub fader: Fader,
    pub charts_local: Vec<ChartItem>,

    pub icons: [SafeTexture; 8],
}

impl SharedState {
    pub async fn new(fallback: FontArc) -> Result<Self> {
        let font = FontArc::try_from_vec(load_file("bold.ttf").await?)?;
        BOLD_FONT.with(move |it| *it.borrow_mut() = Some(TextPainter::new(font, Some(fallback))));
        Ok(Self {
            t: 0.,
            rt: 0.,
            fader: Fader::new(),
            charts_local: Vec::new(),

            icons: Resource::load_icons().await?,
        })
    }

    pub fn update(&mut self, tm: &mut TimeManager) {
        self.t = tm.now() as _;
        self.rt = tm.real_time() as _;
    }

    pub fn render_fader<R>(&mut self, ui: &mut Ui, f: impl FnOnce(&mut Ui) -> R) -> R {
        self.fader.render(ui, self.t, f)
    }

    pub fn reload_local_charts(&mut self) {
        self.charts_local = load_local(&(ChartOrder::Default, false));
    }
}

#[derive(Default)]
#[allow(dead_code)]
pub enum NextPage {
    #[default]
    None,
    Overlay(Box<dyn Page>),
    Pop,
}

pub trait Page {
    fn label(&self) -> Cow<'static, str>;

    fn can_play_bgm(&self) -> bool {
        true
    }
    fn on_result(&mut self, _result: Box<dyn Any>, _s: &mut SharedState) -> Result<()> {
        Ok(())
    }
    fn enter(&mut self, _s: &mut SharedState) -> Result<()> {
        Ok(())
    }
    fn update(&mut self, s: &mut SharedState) -> Result<()>;
    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool>;
    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()>;
    fn pause(&mut self) -> Result<()> {
        Ok(())
    }
    fn resume(&mut self) -> Result<()> {
        Ok(())
    }
    fn next_page(&mut self) -> NextPage {
        NextPage::None
    }
    fn next_scene(&mut self, _s: &mut SharedState) -> NextScene {
        NextScene::None
    }
    fn exit(&mut self) -> Result<()> {
        Ok(())
    }
    fn on_back_pressed(&mut self, _s: &mut SharedState) -> bool {
        false
    }
}
