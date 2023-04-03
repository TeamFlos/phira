use std::any::Any;

use crate::{
    page::{HomePage, NextPage, Page, SharedState},
    scene::{TEX_BACKGROUND, TEX_ICON_BACK},
};
use anyhow::Result;
use macroquad::prelude::*;
use prpr::{
    ext::{screen_aspect, SafeTexture},
    scene::{NextScene, Scene},
    time::TimeManager,
    ui::{button_hit, RectButton, Ui, UI_AUDIO},
};
use sasa::{AudioClip, Music, MusicParams};

const LOW_PASS: f32 = 0.95;

pub struct MainScene {
    state: SharedState,

    bgm: Option<Music>,

    background: SafeTexture,
    btn_back: RectButton,
    icon_back: SafeTexture,

    pages: Vec<Box<dyn Page>>,
}

impl MainScene {
    // shall be call exactly once
    pub async fn new() -> Result<Self> {
        Self::init().await?;

        let bgm_clip = AudioClip::new(load_file("ending.mp3").await?)?;
        let mut bgm = UI_AUDIO.with(|it| {
            it.borrow_mut().create_music(
                bgm_clip,
                MusicParams {
                    loop_: true,
                    ..Default::default()
                },
            )
        })?;
        // bgm.play()?;

        let mut sf = Self::new_inner(Some(bgm)).await?;
        sf.pages.push(Box::new(HomePage::new().await?));
        Ok(sf)
    }

    pub async fn new_with(page: impl Page + 'static) -> Result<Self> {
        let mut sf = Self::new_inner(None).await?;
        sf.pages.push(Box::new(page));
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

    async fn new_inner(bgm: Option<Music>) -> Result<Self> {
        let state = SharedState::new().await?;
        Ok(Self {
            state,

            bgm,

            background: TEX_BACKGROUND.with(|it| it.borrow().clone().unwrap()),
            btn_back: RectButton::new(),
            icon_back: TEX_ICON_BACK.with(|it| it.borrow().clone().unwrap()),

            pages: Vec::new(),
        })
    }
}

impl Scene for MainScene {
    fn on_result(&mut self, _tm: &mut TimeManager, result: Box<dyn Any>) -> Result<()> {
        self.pages.last_mut().unwrap().on_result(result, &mut self.state)
    }

    fn enter(&mut self, _tm: &mut TimeManager, _target: Option<RenderTarget>) -> Result<()> {
        self.pages.last_mut().unwrap().enter(&mut self.state)?;
        Ok(())
    }

    fn resume(&mut self, _tm: &mut TimeManager) -> Result<()> {
        self.pages.last_mut().unwrap().resume()?;
        Ok(())
    }

    fn pause(&mut self, _tm: &mut TimeManager) -> Result<()> {
        self.pages.last_mut().unwrap().pause()?;
        Ok(())
    }

    fn touch(&mut self, tm: &mut TimeManager, touch: &Touch) -> Result<bool> {
        if self.state.fader.transiting() {
            return Ok(false);
        }
        let s = &mut self.state;
        s.t = tm.now() as _;
        if self.btn_back.touch(touch) {
            button_hit();
            if self.pages.len() == 2 {
                if let Some(bgm) = &mut self.bgm {
                    bgm.set_low_pass(0.)?;
                }
            }
            s.fader.back(s.t);
            return Ok(true);
        }
        if self.pages.last_mut().unwrap().touch(touch, s)? {
            return Ok(true);
        }
        Ok(false)
    }

    fn update(&mut self, tm: &mut TimeManager) -> Result<()> {
        UI_AUDIO.with(|it| it.borrow_mut().recover_if_needed())?;
        let s = &mut self.state;
        s.t = tm.now() as _;
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
                    self.pages.push(sub);
                    s.fader.sub(s.t);
                }
                NextPage::Pop => {
                    s.fader.back(s.t);
                }
                NextPage::None => {}
            }
        } else if let Some(true) = s.fader.done(s.t) {
            self.pages.pop().unwrap().exit()?;
        }
        Ok(())
    }

    fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        set_camera(&Camera2D {
            zoom: vec2(1., -screen_aspect()),
            ..Default::default()
        });
        ui.fill_rect(ui.screen_rect(), (*self.background, ui.screen_rect()));
        let s = &mut self.state;
        s.t = tm.now() as _;

        // 1. title
        if s.fader.transiting() {
            let pos = self.pages.len() - 2;
            s.fader.reset();
            s.fader.render_title(ui, &mut s.painter, s.t, &self.pages[pos].label());
        }
        s.fader
            .for_sub(|f| f.render_title(ui, &mut s.painter, s.t, &self.pages.last().unwrap().label()));

        // 2. back
        if self.pages.len() >= 2 {
            let mut r = ui.back_rect();
            self.btn_back.set(ui, r);
            ui.scissor(Some(r));
            r.y += match self.pages.len() {
                1 => 1.,
                2 => s.fader.for_sub(|f| f.progress(s.t)),
                _ => 0.,
            } * r.h;
            ui.fill_rect(r, (*self.icon_back, r));
            ui.scissor(None);
        }

        // 3. page
        if s.fader.transiting() {
            let pos = self.pages.len() - 2;
            self.pages[pos].render(ui, s)?;
        }
        s.fader.sub = true;
        s.fader.reset();
        self.pages.last_mut().unwrap().render(ui, s)?;
        s.fader.sub = false;

        Ok(())
    }

    fn next_scene(&mut self, _tm: &mut TimeManager) -> NextScene {
        self.pages.last_mut().unwrap().next_scene(&mut self.state)
    }
}
