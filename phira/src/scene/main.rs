use super::{import_chart, itl, L10N_LOCAL};
use crate::{
    charts_view::NEED_UPDATE,
    data::LocalChart,
    dir, get_data, get_data_mut,
    mp::MPPanel,
    page::{HomePage, NextPage, Page, ResPackItem, SharedState},
    save_data,
    scene::{TEX_BACKGROUND, TEX_ICON_BACK},
};
use anyhow::{anyhow, Context, Result};
use macroquad::prelude::*;
use once_cell::sync::Lazy;
use prpr::{
    core::ResPackInfo,
    ext::{unzip_into, RectExt, SafeTexture},
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
    io::BufReader,
    sync::atomic::{AtomicBool, Ordering},
    thread_local,
    time::{Duration, Instant},
};
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
}

impl MainScene {
    // shall be call exactly once
    pub async fn new(fallback: FontArc) -> Result<Self> {
        Self::init().await?;

        #[cfg(feature = "closed")]
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
        #[cfg(not(feature = "closed"))]
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
            if MP_PANEL.with(|it| it.borrow_mut().as_mut().map_or(false, |it| it.touch(tm, touch))) {
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
        if self.pages.last_mut().unwrap().touch(touch, s)? {
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
                    self.import_task = Some(Task::new(import_chart(file)));
                }
                "_import_respack" => {
                    let root = dir::respacks()?;
                    let dir = prpr::dir::Dir::new(&root)?;
                    let mut dir_id = String::new();
                    let item: Result<ResPackItem> = (|| {
                        let mut uuid = Uuid::new_v4();
                        while dir.exists(uuid.to_string())? {
                            uuid = Uuid::new_v4();
                        }
                        dir_id = uuid.to_string();
                        dir.create_dir_all(&dir_id)?;
                        let dir = dir.open_dir(&dir_id)?;
                        unzip_into(BufReader::new(File::open(file)?), &dir, false).context("failed to unzip")?;
                        let config: ResPackInfo = serde_yaml::from_reader(dir.open("info.yml").context("missing yml")?)?;
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

        if self.mp_save_pos_at.map_or(false, |it| it < Instant::now()) {
            std::fs::write(position_file()?, format!("{},{}", self.mp_btn_pos.x, self.mp_btn_pos.y))?;
            self.mp_save_pos_at = None;
        }

        Ok(())
    }

    fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        set_camera(&ui.camera());

        STRIPE_MATERIAL.set_uniform("time", ((tm.real_time() as f64 * 0.025) % (std::f64::consts::PI * 2.)) as f32);
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

        self.pages.last_mut().unwrap().render_top(ui, s)?;

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

        if get_data().config.mp_enabled {
            let r = 0.06;
            self.mp_btn_pos.y = self.mp_btn_pos.y.clamp(-ui.top, ui.top);
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
