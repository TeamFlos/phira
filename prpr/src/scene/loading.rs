use super::{draw_background, draw_illustration, ending::RecordUpdateState, game::GameMode, GameScene, NextScene, Scene};
use crate::{
    config::Config,
    ext::{draw_parallelogram, draw_text_aligned, poll_future, screen_aspect, LocalTask, SafeTexture, BLACK_TEXTURE},
    fs::FileSystem,
    info::ChartInfo,
    task::Task,
    time::TimeManager,
    ui::Ui,
};
use ::rand::{seq::SliceRandom, thread_rng};
use anyhow::{Context, Result};
use macroquad::prelude::*;
use regex::Regex;
use std::{rc::Rc, sync::Arc};

const BEFORE_TIME: f32 = 1.;
const TRANSITION_TIME: f32 = 1.4;
const WAIT_TIME: f32 = 0.4;

pub type UploadFn = Arc<dyn Fn(Vec<u8>) -> Task<Result<RecordUpdateState>>>;

pub struct BasicPlayer {
    pub avatar: Option<SafeTexture>,
    pub id: i32,
    pub rks: f32,
}

pub struct LoadingScene {
    info: ChartInfo,
    background: SafeTexture,
    illustration: SafeTexture,
    load_task: LocalTask<Result<GameScene>>,
    next_scene: Option<NextScene>,
    finish_time: f32,
    target: Option<RenderTarget>,
    charter: String,
}

impl LoadingScene {
    pub const TOTAL_TIME: f32 = BEFORE_TIME + TRANSITION_TIME + WAIT_TIME;

    pub async fn new(
        mode: GameMode,
        mut info: ChartInfo,
        config: Config,
        mut fs: Box<dyn FileSystem>,
        player: Option<BasicPlayer>,
        get_size_fn: Option<Rc<dyn Fn() -> (u32, u32)>>,
        upload_fn: Option<UploadFn>,
    ) -> Result<Self> {
        async fn load(fs: &mut Box<dyn FileSystem>, path: &str) -> Result<(Texture2D, Texture2D)> {
            let image = image::load_from_memory(&fs.load_file(path).await?).context("Failed to decode image")?;
            let (w, h) = (image.width(), image.height());
            let size = w as usize * h as usize;

            let mut blurred_rgb = image.to_rgb8();
            let mut vec = unsafe { Vec::from_raw_parts(std::mem::transmute(blurred_rgb.as_mut_ptr()), size, size) };
            fastblur::gaussian_blur(&mut vec, w as _, h as _, 50.);
            std::mem::forget(vec);
            let mut blurred = Vec::with_capacity(size * 4);
            for input in blurred_rgb.chunks_exact(3) {
                blurred.extend_from_slice(input);
                blurred.push(255);
            }
            Ok((
                Texture2D::from_rgba8(w as _, h as _, &image.into_rgba8()),
                Texture2D::from_image(&Image {
                    width: w as _,
                    height: h as _,
                    bytes: blurred,
                }),
            ))
        }

        let background = match load(&mut fs, &info.illustration).await {
            Ok((ill, bg)) => Some((ill, bg)),
            Err(err) => {
                warn!("Failed to load background: {:?}", err);
                None
            }
        };
        let (illustration, background): (SafeTexture, SafeTexture) = background
            .map(|(ill, back)| (ill.into(), back.into()))
            .unwrap_or_else(|| (BLACK_TEXTURE.clone(), BLACK_TEXTURE.clone()));
        let get_size_fn = get_size_fn.unwrap_or_else(|| Rc::new(|| (screen_width() as u32, screen_height() as u32)));
        if info.tip.is_none() {
            info.tip = Some(crate::config::TIPS.choose(&mut thread_rng()).unwrap().to_owned());
        }
        let future =
            Box::pin(GameScene::new(mode, info.clone(), config, fs, player, background.clone(), illustration.clone(), get_size_fn, upload_fn));
        let charter = Regex::new(r"\[!:[0-9]+:([^:]*)\]").unwrap().replace_all(&info.charter, "$1").to_string();
        Ok(Self {
            info,
            background,
            illustration,
            load_task: Some(future),
            next_scene: None,
            finish_time: f32::INFINITY,
            target: None,
            charter,
        })
    }
}

impl Scene for LoadingScene {
    fn enter(&mut self, _tm: &mut TimeManager, target: Option<RenderTarget>) -> Result<()> {
        self.target = target;
        Ok(())
    }

    fn update(&mut self, tm: &mut TimeManager) -> Result<()> {
        if let Some(future) = self.load_task.as_mut() {
            loop {
                match poll_future(future.as_mut()) {
                    None => {
                        if self.target.is_none() {
                            break;
                        }
                        std::thread::yield_now();
                    }
                    Some(game_scene) => {
                        self.load_task = None;
                        self.next_scene =
                            Some(game_scene.map_or_else(|e| NextScene::PopWithResult(Box::new(e)), |it| NextScene::Replace(Box::new(it))));
                        self.finish_time = tm.now() as f32 + BEFORE_TIME;
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        let asp = screen_aspect();
        let top = 1. / asp;
        let now = tm.now() as f32;
        let intern = unsafe { get_internal_gl() };
        let gl = intern.quad_gl;
        set_camera(&Camera2D {
            zoom: vec2(1., -asp),
            render_target: self.target,
            ..Default::default()
        });
        draw_background(*self.background);
        let dx = if now > self.finish_time {
            let p = ((now - self.finish_time) / TRANSITION_TIME).min(1.);
            p.powi(3) * 2.
        } else {
            0.
        };
        if dx != 0. {
            gl.push_model_matrix(Mat4::from_translation(vec3(dx, 0., 0.)));
        }
        let vo = -top / 10.;
        let r = draw_illustration(*self.illustration, 0.38, vo, 1., 1., WHITE);
        let h = r.h / 3.6;
        let main = Rect::new(-0.88, vo - h / 2. - top / 10., 0.78, h);
        draw_parallelogram(main, None, Color::new(0., 0., 0., 0.7), true);
        let p = (main.x + main.w * 0.09, main.y + main.h * 0.36);
        let mut text = ui.text(&self.info.name).pos(p.0, p.1).anchor(0., 0.5).size(0.7);
        if text.measure().w <= main.w * 0.6 {
            text.draw();
        } else {
            drop(text);
            ui.text(&self.info.name)
                .pos(p.0, p.1)
                .anchor(0., 0.5)
                .max_width(main.w * 0.6)
                .size(0.5)
                .draw();
        }
        draw_text_aligned(ui, &self.info.composer, main.x + main.w * 0.09, main.y + main.h * 0.73, (0., 0.5), 0.36, WHITE);

        let ext = 0.06;
        let sub = Rect::new(main.x + main.w * 0.71, main.y - main.h * ext, main.w * 0.26, main.h * (1. + ext * 2.));
        let mut ct = sub.center();
        ct.x += sub.w * 0.02;
        draw_parallelogram(sub, None, WHITE, true);
        draw_text_aligned(ui, &(self.info.difficulty as u32).to_string(), ct.x, ct.y + sub.h * 0.05, (0.5, 1.), 0.88, BLACK);
        draw_text_aligned(ui, self.info.level.split_whitespace().next().unwrap_or_default(), ct.x, ct.y + sub.h * 0.09, (0.5, 0.), 0.34, BLACK);
        let t = draw_text_aligned(ui, "Chart", main.x + main.w / 6., main.y + main.h * 1.2, (0., 0.), 0.3, WHITE);
        draw_text_aligned(ui, &self.charter, t.x, t.y + top / 20., (0., 0.), 0.47, WHITE);
        let w = 0.027;
        let t = draw_text_aligned(ui, "Illustration", t.x - w, t.y + w / 0.13 / 13. * 5., (0., 0.), 0.3, WHITE);
        draw_text_aligned(ui, &self.info.illustrator, t.x, t.y + top / 20., (0., 0.), 0.47, WHITE);

        draw_text_aligned(ui, self.info.tip.as_ref().unwrap(), -0.91, top * 0.92, (0., 1.), 0.47, WHITE);
        let t = draw_text_aligned(ui, "Loading...", 0.87, top * 0.92, (1., 1.), 0.44, WHITE);
        let we = 0.2;
        let he = 0.5;
        let r = Rect::new(t.x - t.w * we, t.y - t.h * he, t.w * (1. + we * 2.), t.h * (1. + he * 2.));

        let p = 0.6;
        let s = 0.2;
        let t = ((now - 0.3).max(0.) % (p * 2. + s)) / p;
        let st = (t - 1.).clamp(0., 1.).powi(3);
        let en = 1. - (1. - t.min(1.)).powi(3);

        let mut r = Rect::new(r.x + r.w * st, r.y, r.w * (en - st), r.h);
        ui.fill_rect(r, WHITE);
        r.x += dx;
        ui.scissor(Some(r));
        draw_text_aligned(ui, "Loading...", 0.87, top * 0.92, (1., 1.), 0.44, BLACK);
        ui.scissor(None);

        if dx != 0. {
            gl.pop_model_matrix();
        }
        Ok(())
    }

    fn next_scene(&mut self, tm: &mut TimeManager) -> NextScene {
        if matches!(self.next_scene, Some(NextScene::PopWithResult(_))) {
            return self.next_scene.take().unwrap();
        }
        if tm.now() as f32 > self.finish_time + TRANSITION_TIME + WAIT_TIME {
            if let Some(scene) = self.next_scene.take() {
                return scene;
            }
        }
        NextScene::None
    }
}
