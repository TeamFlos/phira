use super::{import_chart, L10N_LOCAL};
use crate::{
    charts_view::NEED_UPDATE,
    data::LocalChart,
    dir, get_data, get_data_mut,
    mp::MPPanel,
    page::{ExportInfo, HomePage, NextPage, Page, ResPackItem, SharedState},
    save_data,
    scene::{confirm_dialog, import_chart_to, TEX_BACKGROUND, TEX_ICON_BACK},
};
use anyhow::{anyhow, bail, Context, Result};
use macroquad::prelude::*;
use once_cell::sync::Lazy;
use prpr::{
    core::ResPackInfo,
    ext::{unzip_into, RectExt, SafeTexture},
    info::ChartInfo,
    scene::{return_file, show_error, show_message, take_file, NextScene, Scene},
    task::Task,
    time::TimeManager,
    ui::{button_hit, FontArc, RectButton, Ui, UI_AUDIO},
};
use sasa::{AudioClip, Music};
use std::{
    any::Any,
    cell::RefCell,
    fs::File,
    io::{BufReader, Read, Seek, SeekFrom},
    mem,
    path::{Component, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread_local,
    time::{Duration, Instant},
};
use tempfile::tempfile;
use uuid::Uuid;

const LOW_PASS: f32 = 0.95;

pub static BGM_VOLUME_UPDATED: AtomicBool = AtomicBool::new(false);

thread_local! {
    static RESPACK_ITEM: RefCell<Option<ResPackItem>> = RefCell::default();
    pub static MP_PANEL: RefCell<Option<MPPanel>> = RefCell::default();
}

#[inline]
fn position_file() -> Result<String> {
    Ok(format!("{}/mp-pos", dir::root()?))
}

pub struct MainScene {
    state: SharedState,

    bgm: Option<Music>,

    background: SafeTexture,
    btn_back: RectButton,
    icon_back: SafeTexture,

    pages: Vec<Box<dyn Page>>,

    import_task: Option<Task<Result<LocalChart>>>,

    mp_btn: RectButton,
    mp_icon: SafeTexture,
    mp_btn_pos: Vec2,
    mp_move: Option<(u64, Vec2, Vec2)>,
    mp_moved: bool,
    mp_save_pos_at: Option<Instant>,

    // batch import
    batch_import_confirm: Arc<AtomicBool>,
    batch_import: Option<(String, ExportInfo)>,
    batch_import_task: Option<Task<Result<()>>>,
    batch_import_rx: Option<mpsc::Receiver<ImportChart>>,
    batch_imported_charts: Vec<ImportChart>,
    batch_import_total: usize,
}

enum ImportChart {
    Imported(Box<LocalChart>),
    Skipped(String),
}

impl MainScene {
    // shall be call exactly once
    pub async fn new(fallback: FontArc) -> Result<Self> {
        Self::init().await?;

        #[cfg(closed)]
        let bgm = {
            let bgm_clip = AudioClip::new(crate::load_res("res/bgm").await)?;
            Some(UI_AUDIO.with(|it| {
                it.borrow_mut().create_music(
                    bgm_clip,
                    sasa::MusicParams {
                        amplifier: get_data().config.volume_bgm,
                        loop_mix_time: 5.46,
                        command_buffer_size: 64,
                        ..Default::default()
                    },
                )
            })?)
        };
        #[cfg(not(closed))]
        let bgm = None;

        let mut sf = Self::new_inner(bgm, fallback).await?;
        sf.pages.push(Box::new(HomePage::new().await?));
        Ok(sf)
    }

    async fn init() -> Result<()> {
        // init button hitsound
        macro_rules! load_sfx {
            ($name:ident, $path:literal) => {{
                let clip = AudioClip::new(load_file($path).await?)?;
                let sound = UI_AUDIO.with(|it| it.borrow_mut().create_sfx(clip, None))?;
                prpr::ui::$name.with(|it| *it.borrow_mut() = Some(sound));
            }};
        }
        load_sfx!(UI_BTN_HITSOUND_LARGE, "button_large.ogg");
        load_sfx!(UI_BTN_HITSOUND, "button.ogg");
        load_sfx!(UI_SWITCH_SOUND, "switch.ogg");

        let background: SafeTexture = load_texture("background.jpg").await?.into();
        let icon_back: SafeTexture = load_texture("back.png").await?.into();

        TEX_BACKGROUND.with(|it| *it.borrow_mut() = Some(background));
        TEX_ICON_BACK.with(|it| *it.borrow_mut() = Some(icon_back));

        Ok(())
    }

    async fn new_inner(bgm: Option<Music>, fallback: FontArc) -> Result<Self> {
        let state = SharedState::new(fallback).await?;
        let icon_user = load_texture("user.png").await?;
        MP_PANEL.with(|it| *it.borrow_mut() = Some(MPPanel::new(icon_user.into())));
        Ok(Self {
            state,

            bgm,

            background: TEX_BACKGROUND.with(|it| it.borrow().clone().unwrap()),
            btn_back: RectButton::new(),
            icon_back: TEX_ICON_BACK.with(|it| it.borrow().clone().unwrap()),

            pages: Vec::new(),

            import_task: None,

            mp_btn: RectButton::new(),
            mp_icon: SafeTexture::from(load_texture("multiplayer.png").await?).with_mipmap(),
            mp_btn_pos: (|| -> Result<Vec2> {
                let s = std::fs::read_to_string(position_file()?)?;
                let (x, y) = s.split_once(',').ok_or_else(|| anyhow!("invalid"))?;
                Ok(vec2(x.parse()?, y.parse()?))
            })()
            .unwrap_or_default(),
            mp_move: None,
            mp_moved: false,
            mp_save_pos_at: None,

            batch_import_confirm: Arc::default(),
            batch_import: None,
            batch_import_task: None,
            batch_import_rx: None,
            batch_imported_charts: Vec::new(),
            batch_import_total: 0,
        })
    }

    fn pop(&mut self) {
        if !self.pages.last().unwrap().can_play_bgm() && self.pages[self.pages.len() - 2].can_play_bgm() {
            if let Some(bgm) = &mut self.bgm {
                let _ = bgm.fade_in(0.5);
            }
        }
        self.state.fader.back(self.state.t);
    }

    pub fn take_imported_respack() -> Option<ResPackItem> {
        RESPACK_ITEM.with(|it| it.borrow_mut().take())
    }
}

impl Scene for MainScene {
    fn on_result(&mut self, _tm: &mut TimeManager, result: Box<dyn Any>) -> Result<()> {
        self.pages.last_mut().unwrap().on_result(result, &mut self.state)
    }

    fn enter(&mut self, tm: &mut TimeManager, _target: Option<RenderTarget>) -> Result<()> {
        if let Some(bgm) = &mut self.bgm {
            let _ = bgm.fade_in(1.3);
        }
        self.state.update(tm);
        self.pages.last_mut().unwrap().enter(&mut self.state)?;
        MP_PANEL.with(|it| {
            if let Some(panel) = it.borrow_mut().as_mut() {
                panel.enter();
            }
        });
        Ok(())
    }

    fn resume(&mut self, tm: &mut TimeManager) -> Result<()> {
        if let Some(bgm) = &mut self.bgm {
            bgm.play()?;
        }
        self.state.update(tm);
        self.pages.last_mut().unwrap().resume()?;
        Ok(())
    }

    fn pause(&mut self, tm: &mut TimeManager) -> Result<()> {
        if let Some(bgm) = &mut self.bgm {
            bgm.pause()?;
        }
        self.state.update(tm);
        self.pages.last_mut().unwrap().pause()?;
        Ok(())
    }

    fn touch(&mut self, tm: &mut TimeManager, touch: &Touch) -> Result<bool> {
        if self.state.fader.transiting() {
            return Ok(false);
        }
        if self.import_task.is_some() {
            return Ok(true);
        }

        if get_data().config.mp_enabled {
            if MP_PANEL.with(|it| it.borrow_mut().as_mut().is_some_and(|it| it.touch(tm, touch))) {
                return Ok(true);
            }
            if self.mp_btn.touch(touch) && !self.mp_moved {
                MP_PANEL.with(|it| {
                    if let Some(panel) = it.borrow_mut().as_mut() {
                        panel.show(tm.real_time() as _);
                    }
                });
                self.mp_move = None;
                self.mp_moved = false;
                return Ok(true);
            }
            if let Some((id, pos, btn_pos)) = self.mp_move {
                if touch.id == id {
                    if matches!(touch.phase, TouchPhase::Cancelled | TouchPhase::Ended) {
                        self.mp_move = None;
                        self.mp_moved = false;
                        return Ok(true);
                    }
                    let new_pos = touch.position;
                    if !self.mp_moved && (new_pos - pos).length() > 0.03 {
                        self.mp_moved = true;
                    }
                    if self.mp_moved {
                        self.mp_btn_pos = new_pos - pos + btn_pos;
                        self.mp_save_pos_at = Some(Instant::now() + Duration::from_secs(1));
                    }
                }
                return Ok(true);
            } else if self.mp_btn.touching() {
                self.mp_move = Some((touch.id, touch.position, self.mp_btn_pos));
                return Ok(true);
            }
        }

        let s = &mut self.state;
        s.update(tm);
        if self.pages.last_mut().unwrap().touch(touch, s)? {
            return Ok(true);
        }
        if self.btn_back.touch(touch) && self.pages.len() > 1 {
            button_hit();
            if !self.pages.last_mut().unwrap().on_back_pressed(&mut self.state) {
                if self.pages.len() == 2 {
                    if let Some(bgm) = &mut self.bgm {
                        bgm.set_low_pass(0.)?;
                    }
                }
                self.pop();
            }
            return Ok(true);
        }
        Ok(false)
    }

    fn update(&mut self, tm: &mut TimeManager) -> Result<()> {
        UI_AUDIO.with(|it| it.borrow_mut().recover_if_needed())?;
        if get_data().config.mp_enabled {
            MP_PANEL.with(|it| {
                if let Some(panel) = it.borrow_mut().as_mut() {
                    panel.update(tm)
                } else {
                    Ok(())
                }
            })?;
        }
        let s = &mut self.state;
        s.update(tm);
        if s.fader.transiting() {
            let pos = self.pages.len() - 2;
            self.pages[pos].update(s)?;
        }
        self.pages.last_mut().unwrap().update(s)?;
        if !s.fader.transiting() {
            match self.pages.last_mut().unwrap().next_page() {
                NextPage::Overlay(mut sub) => {
                    if self.pages.len() == 1 {
                        if let Some(bgm) = &mut self.bgm {
                            bgm.set_low_pass(LOW_PASS)?;
                        }
                    }
                    sub.enter(s)?;
                    if !sub.can_play_bgm() {
                        if let Some(bgm) = &mut self.bgm {
                            let _ = bgm.fade_out(0.5);
                        }
                    }
                    self.pages.push(sub);
                    s.fader.sub(s.t);
                }
                NextPage::Pop => {
                    self.pop();
                }
                NextPage::None => {}
            }
        } else if let Some(true) = s.fader.done(s.t) {
            self.pages.pop().unwrap().exit()?;
            self.pages.last_mut().unwrap().enter(s)?;
        }
        if let Some(bgm) = &mut self.bgm {
            if BGM_VOLUME_UPDATED.fetch_and(false, Ordering::Relaxed) {
                bgm.set_amplifier(get_data().config.volume_bgm)?;
            }
        }
        if let Some(task) = &mut self.import_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        show_error(err.context(itl!("import-failed")));
                    }
                    Ok(chart) => {
                        show_message(itl!("import-success")).ok();
                        get_data_mut().charts.push(chart);
                        save_data()?;
                        self.state.reload_local_charts();
                        NEED_UPDATE.store(true, Ordering::Relaxed);
                    }
                }
                self.import_task = None;
            }
        }
        if let Some((id, file)) = take_file() {
            match id.as_str() {
                "_import" => {
                    let export_info = (|| -> Result<Option<(ExportInfo, usize)>> {
                        let file = File::open(&file)?;
                        let mut archive = zip::ZipArchive::new(file)?;
                        let export_info = match archive.by_name("export.json") {
                            Err(zip::result::ZipError::FileNotFound) => {
                                return Ok(None);
                            }
                            Err(err) => {
                                return Err(err.into());
                            }
                            Ok(file) => serde_json::from_reader(file)?,
                        };
                        let mut count = 0;
                        for i in 0..archive.len() {
                            let file = archive.by_index(i)?;
                            if file.enclosed_name().is_some_and(|it| it.extension().is_some_and(|ext| ext == "zip")) {
                                count += 1;
                            }
                        }
                        Ok(Some((export_info, count)))
                    })();
                    match export_info {
                        Err(err) => {
                            show_error(err.context(itl!("import-failed")));
                        }
                        Ok(None) => {
                            self.import_task = Some(Task::new(async move {
                                let file = File::open(&file).context("cannot open file")?;
                                import_chart(file).await
                            }));
                        }
                        Ok(Some((info, count))) => {
                            self.batch_import = Some((file, info));
                            self.batch_import_total = count;
                            confirm_dialog(itl!("batch-import"), itl!("batch-import-confirm", "count" => count), self.batch_import_confirm.clone());
                        }
                    };
                }
                "_import_respack" => {
                    let root = dir::respacks()?;
                    let dir = prpr::dir::Dir::new(&root)?;
                    let mut dir_id = String::new();
                    let item: Result<ResPackItem> = (|| {
                        let config = {
                            let mut zip = zip::ZipArchive::new(BufReader::new(File::open(&file)?))?;
                            let config: ResPackInfo =
                                serde_yaml::from_reader(zip.by_name("info.yml").context("missing info.yml")?).context("invalid info.yml")?;
                            if config.name.is_empty() {
                                bail!("empty name");
                            }
                            if config.name.len() > 100 {
                                bail!("name too long");
                            }
                            if config.description.len() > 1000 {
                                bail!("description too long");
                            }
                            let mut buffer = Vec::new();
                            for file in [
                                "click.png",
                                "click_mh.png",
                                "drag.png",
                                "drag_mh.png",
                                "flick.png",
                                "flick_mh.png",
                                "hold.png",
                                "hold_mh.png",
                                "hit_fx.png",
                            ] {
                                let mut entry = zip.by_name(file).with_context(|| format!("missing file: {file}"))?;
                                buffer.clear();
                                entry.read_to_end(&mut buffer)?;
                                image::load_from_memory(&buffer).with_context(|| format!("failed to load image: {file}"))?;
                            }

                            for audio in ["click.ogg", "drag.ogg", "flick.ogg", "ending.ogg"] {
                                let mut entry = match zip.by_name(audio) {
                                    Err(zip::result::ZipError::FileNotFound) => continue,
                                    Err(err) => return Err(err.into()),
                                    Ok(file) => file,
                                };
                                buffer.clear();
                                entry.read_to_end(&mut buffer)?;
                                AudioClip::new(mem::take(&mut buffer)).with_context(|| format!("failed to load audio: {audio}"))?;
                            }
                            config
                        };

                        let mut uuid = Uuid::new_v4();
                        while dir.exists(uuid.to_string())? {
                            uuid = Uuid::new_v4();
                        }
                        dir_id = uuid.to_string();
                        dir.create_dir_all(&dir_id)?;
                        let dir = dir.open_dir(&dir_id)?;
                        unzip_into(BufReader::new(File::open(file)?), &dir, false).context("failed to unzip")?;
                        get_data_mut().respacks.push(dir_id.clone());
                        save_data()?;
                        Ok(ResPackItem::new(Some(format!("{root}/{dir_id}").into()), config.name))
                    })();
                    match item {
                        Err(err) => {
                            dir.remove_dir_all(&dir_id)?;
                            show_error(err.context(itl!("import-respack-failed")));
                        }
                        Ok(item) => {
                            RESPACK_ITEM.with(|it| *it.borrow_mut() = Some(item));
                            show_message(itl!("import-respack-success"));
                        }
                    }
                }
                _ => return_file(id, file),
            }
        }
        if self.batch_import_confirm.swap(false, Ordering::Relaxed) {
            if let Some((file, _info)) = self.batch_import.take() {
                let (tx, rx) = mpsc::channel();
                self.batch_import_rx = Some(rx);
                self.batch_imported_charts.clear();
                self.batch_import_task = Some(Task::new(async move {
                    let mut archive = zip::ZipArchive::new(BufReader::new(File::open(&file)?))?;
                    let charts_dir = dir::charts()?;
                    for i in 0..archive.len() {
                        let mut file = archive.by_index(i)?;
                        let Some(name) = file.enclosed_name() else {
                            continue;
                        };
                        if name.extension().is_none_or(|it| it != "zip") {
                            continue;
                        }
                        let [Component::Normal(dir), Component::Normal(name)] = name.components().collect::<Vec<_>>()[..] else {
                            continue;
                        };
                        let mut to_tempfile = || -> std::io::Result<_> {
                            let mut tf = tempfile()?;
                            std::io::copy(&mut file, &mut tf)?;
                            tf.seek(SeekFrom::Start(0))?;
                            Ok(tf)
                        };
                        match dir.to_str() {
                            Some("custom") => {
                                let tf = to_tempfile()?;
                                let chart = import_chart(tf)
                                    .await
                                    .with_context(|| itl!("batch-import-failed-chart", "chart" => name.display().to_string()))?;
                                let _ = tx.send(ImportChart::Imported(Box::new(chart))).ok();
                            }
                            Some("download") => {
                                let Some(id) = name.to_str().and_then(|it| it.strip_suffix(".zip")).and_then(|it| it.parse::<i32>().ok()) else {
                                    warn!("invalid batch import download id: {:?}", name);
                                    continue;
                                };
                                let local_path = format!("download/{id}");
                                let path = PathBuf::from(format!("{charts_dir}/{local_path}"));
                                if std::fs::exists(&path)? {
                                    let info: ChartInfo = serde_yaml::from_reader(File::open(path.join("info.yml"))?)?;
                                    let _ = tx.send(ImportChart::Skipped(info.name));
                                    continue;
                                }
                                std::fs::create_dir(&path)?;
                                let tf = to_tempfile()?;
                                let chart = import_chart_to(&path, local_path, tf)
                                    .await
                                    .with_context(|| itl!("batch-import-failed-chart", "chart" => name.display().to_string()))?;
                                let _ = tx.send(ImportChart::Imported(Box::new(chart))).ok();
                            }
                            _ => {
                                warn!("invalid batch import dir: {:?}", dir);
                            }
                        }
                    }
                    Ok(())
                }));
            }
        }

        if let Some(rx) = &mut self.batch_import_rx {
            match rx.try_recv() {
                Ok(chart) => {
                    self.batch_imported_charts.push(chart);
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    warn!("import thread panicked");
                    self.batch_import_rx = None;
                }
            }
        }

        if let Some(task) = &mut self.batch_import_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        let charts = dir::charts()?;
                        for chart in self.batch_imported_charts.drain(..) {
                            if let ImportChart::Imported(chart) = chart {
                                let path = format!("{charts}{}", chart.local_path);
                                let _ = std::fs::remove_dir_all(path);
                            }
                        }
                        show_error(err.context(itl!("batch-import-failed")));
                    }
                    Ok(()) => {
                        let data = get_data_mut();
                        let mut count = 0;
                        let mut skipped = String::new();
                        for chart in self.batch_imported_charts.drain(..) {
                            match chart {
                                ImportChart::Imported(chart) => {
                                    data.charts.push(*chart);
                                    count += 1;
                                }
                                ImportChart::Skipped(name) => {
                                    if !skipped.is_empty() {
                                        skipped.push_str(", ");
                                    }
                                    skipped.push_str(&name);
                                }
                            }
                        }
                        save_data()?;
                        self.state.reload_local_charts();
                        NEED_UPDATE.store(true, Ordering::Relaxed);

                        let mut message = itl!("batch-import-success", "count" => count);
                        if !skipped.is_empty() {
                            message.push('\n');
                            message += &itl!("batch-import-downloaded-skipped", "charts" => skipped);
                        }
                        show_message(message);
                    }
                }
                self.batch_import_task = None;
                self.batch_import_rx = None;
            }
        }

        if self.mp_save_pos_at.is_some_and(|it| it < Instant::now()) {
            std::fs::write(position_file()?, format!("{},{}", self.mp_btn_pos.x, self.mp_btn_pos.y))?;
            self.mp_save_pos_at = None;
        }

        Ok(())
    }

    fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        set_camera(&ui.camera());

        STRIPE_MATERIAL.set_uniform("time", ((tm.real_time() * 0.025) % (std::f64::consts::PI * 2.)) as f32);
        gl_use_material(*STRIPE_MATERIAL);
        ui.fill_rect(ui.screen_rect(), (*self.background, ui.screen_rect()));
        gl_use_default_material();

        let s = &mut self.state;
        s.update(tm);

        // 1. page
        if s.fader.transiting() {
            let pos = self.pages.len() - 2;
            let old = s.fader.distance;
            s.fader.distance *= -0.6;
            self.pages[pos].render(ui, s)?;
            s.fader.distance = old;
        }
        s.fader.sub = true;
        s.fader.reset();
        self.pages.last_mut().unwrap().render(ui, s)?;
        s.fader.sub = false;

        // 2. title
        if s.fader.transiting() {
            let pos = self.pages.len() - 2;
            s.fader.reset();
            s.fader.render_title(ui, s.t, &self.pages[pos].label());
        }
        s.fader.for_sub(|f| f.render_title(ui, s.t, &self.pages.last().unwrap().label()));

        // 3. back
        if self.pages.len() >= 2 {
            let mut r = ui.back_rect();
            self.btn_back.set(ui, r);
            ui.scissor(r, |ui| {
                r.y += match self.pages.len() {
                    1 => 1.,
                    2 => s.fader.for_sub(|f| f.progress(s.t)),
                    _ => 0.,
                } * r.h;
                ui.fill_rect(r, (*self.icon_back, r));
            });
        }

        self.pages.last_mut().unwrap().render_top(ui, s)?;

        if get_data().config.mp_enabled {
            let r = 0.06;
            self.mp_btn_pos.y = self.mp_btn_pos.y.clamp(-ui.top, ui.top);
            self.mp_btn_pos.x = self.mp_btn_pos.x.clamp(-1., 1.);
            ui.fill_circle(self.mp_btn_pos.x, self.mp_btn_pos.y, r, ui.background());
            let r = Rect::new(self.mp_btn_pos.x, self.mp_btn_pos.y, 0., 0.).feather(r);
            self.mp_btn.set(ui, r);
            let r = r.feather(-0.02);
            ui.fill_rect(r, (*self.mp_icon, r));

            MP_PANEL.with(|it| {
                if let Some(panel) = it.borrow_mut().as_mut() {
                    panel.render(tm, ui);
                }
            });
        }

        if self.import_task.is_some() {
            ui.full_loading(itl!("importing"), s.t);
        }
        if self.batch_import_task.is_some() {
            let current = self.batch_imported_charts.len();
            let total = self.batch_import_total;
            ui.full_loading(itl!("batch-importing", "current" => current, "total" => total), s.t);
        }

        Ok(())
    }

    fn next_scene(&mut self, _tm: &mut TimeManager) -> NextScene {
        let res = MP_PANEL
            .with(|it| it.borrow_mut().as_mut().and_then(|it| it.next_scene()))
            .unwrap_or(self.pages.last_mut().unwrap().next_scene(&mut self.state));
        if !matches!(res, NextScene::None) {
            if let Some(bgm) = &mut self.bgm {
                let _ = bgm.fade_out(0.5);
            }
        }
        res
    }
}

static STRIPE_MATERIAL: Lazy<Material> = Lazy::new(|| {
    load_material(
        shader::VERTEX,
        shader::FRAGMENT,
        MaterialParams {
            uniforms: vec![("time".to_owned(), UniformType::Float1)],
            ..Default::default()
        },
    )
    .unwrap()
});

mod shader {
    pub const VERTEX: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;

varying lowp vec4 color;
varying lowp vec2 pos0;
varying lowp vec2 uv;

uniform mat4 Model;
uniform mat4 Projection;

void main() {
    gl_Position = Projection * Model * vec4(position, 1);
    color = color0 / 255.0;
    pos0 = position.xy;
    uv = texcoord;
}"#;

    pub const FRAGMENT: &str = r#"#version 100
precision highp float;

varying lowp vec4 color;
varying lowp vec2 pos0;
varying lowp vec2 uv;

uniform sampler2D Texture;
uniform float time;

void main() {
    float angle = 0.66;
    float w = sin(angle) * pos0.y + cos(angle) * pos0.x - time;
    float t = mod(w, 0.02);
    float p = step(t, 0.012) * 0.07;
    gl_FragColor = texture2D(Texture, uv);
    gl_FragColor += (vec4(1.0) - gl_FragColor) * p;
}"#;
}
