use super::{draw_background, ending::RecordUpdateState, game::GameMode, GameScene, NextScene, Scene};
use crate::{
    config::Config,
    ext::{poll_future, screen_aspect, semi_black, semi_white, LocalTask, RectExt, SafeTexture, BLACK_TEXTURE},
    fs::FileSystem,
    info::ChartInfo,
    task::Task,
    time::TimeManager,
    ui::{rounded_rect, rounded_rect_shadow, LoadingParams, ShadowConfig, Ui},
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

    theme_color: Color,
    use_black: bool,
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
        async fn load(fs: &mut Box<dyn FileSystem>, path: &str) -> Result<(Texture2D, Texture2D, Color)> {
            let image = image::load_from_memory(&fs.load_file(path).await?).context("Failed to decode image")?;
            let (w, h) = (image.width(), image.height());
            let size = w as usize * h as usize;

            let mut blurred_rgb = image.to_rgb8();
            let color = color_thief::get_palette(&blurred_rgb, color_thief::ColorFormat::Rgb, 10, 2)?[0];
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
                Color::from_rgba(color.r, color.g, color.b, 255),
            ))
        }

        let (background, theme_color) = match load(&mut fs, &info.illustration).await {
            Ok((ill, bg, color)) => (Some((ill, bg)), color),
            Err(err) => {
                warn!("Failed to load background: {:?}", err);
                (None, WHITE)
            }
        };
        let use_black = (theme_color.r * 0.299 + theme_color.g * 0.587 + theme_color.b * 0.114) > 186. / 255.;
        let (illustration, background): (SafeTexture, SafeTexture) = background
            .map(|(ill, back)| (ill.into(), back.into()))
            .unwrap_or_else(|| (BLACK_TEXTURE.clone(), BLACK_TEXTURE.clone()));
        let get_size_fn = get_size_fn.unwrap_or_else(|| Rc::new(|| (screen_width() as u32, screen_height() as u32)));
        if info.tip.is_none() {
            info.tip = Some(crate::config::TIPS.choose(&mut thread_rng()).unwrap().to_owned());
        }
        let future =
            Box::pin(GameScene::new(mode, info.clone(), config, fs, player, background.clone(), illustration.clone(), get_size_fn, upload_fn, theme_color, use_black));
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

            theme_color,
            use_black,
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
        let t = tm.now() as f32;
        set_camera(&Camera2D {
            zoom: vec2(1., -asp),
            render_target: self.target,
            ..Default::default()
        });
        draw_background(*self.background);

        let dx = if t > self.finish_time {
            let p = ((t - self.finish_time) / TRANSITION_TIME).min(1.);
            p.powi(3) * 2.
        } else {
            0.
        };

        ui.dx(-dx);

        let r = Rect::default().nonuniform_feather(0.65, top * 0.7);
        let config = ShadowConfig {
            radius: 0.03,
            ..Default::default()
        };
        let bar_height = 0.16;
        let ir = Rect { h: r.h - bar_height, ..r };

        let (main, sub) = Ui::main_sub_colors(self.use_black, 1.);

        rounded_rect_shadow(ui, r, &config);
        rounded_rect(ui, r, config.radius, |ui| {
            ui.fill_rect(r, self.theme_color);
            ui.fill_rect(ir, (*self.illustration, ir));
            ui.fill_rect(ir, (semi_black(0.5), (ir.x, ir.bottom()), Color::default(), (ir.x, ir.y)));
        });

        let ct = ir.bottom() + bar_height / 2.;
        let lf = r.x + 0.04;
        let rt = r.x + r.w * 0.65;
        let mw = rt - lf - 0.02;
        ui.text(&self.info.name)
            .pos(lf, ct)
            .anchor(0., 1.)
            .size(0.7)
            .color(main)
            .max_width(mw)
            .draw();
        ui.text(&self.info.composer)
            .pos(lf, ct + 0.012)
            .anchor(0., 0.)
            .size(0.4)
            .color(sub)
            .max_width(mw)
            .draw();

        ui.fill_rect(Rect::new(rt, ct, 0., 0.).nonuniform_feather(0.001, bar_height * 0.4), sub);

        let lf = rt + 0.03;
        let dy = bar_height / 6.;
        let size = 0.45;
        ui.text("Chart")
            .pos(lf, ct - dy)
            .anchor(0., 0.5)
            .no_baseline()
            .size(size)
            .color(sub)
            .draw();
        ui.text("Cover")
            .pos(lf, ct + dy)
            .anchor(0., 0.5)
            .no_baseline()
            .size(size)
            .color(sub)
            .draw();

        let lf = lf + 0.12;
        let mw = r.right() - lf - 0.01;
        ui.text(&self.charter)
            .pos(lf, ct - dy)
            .anchor(0., 0.5)
            .no_baseline()
            .size(size)
            .color(main)
            .max_width(mw)
            .draw();
        ui.text(&self.info.illustrator)
            .pos(lf, ct + dy)
            .anchor(0., 0.5)
            .no_baseline()
            .size(size)
            .color(main)
            .max_width(mw)
            .draw();

        let r = 0.07;
        ui.loading(
            1. - r,
            top - r,
            t,
            if t > self.finish_time {
                let p = ((t - self.finish_time) / 0.4).min(1.);
                semi_white((1. - p).powi(3))
            } else {
                WHITE
            },
            LoadingParams {
                radius: 0.04,
                width: 0.01,
                ..Default::default()
            },
        );

        ui.text(self.info.tip.as_ref().unwrap())
            .pos(-0.95, top - 0.05)
            .anchor(0., 1.)
            .size(0.47)
            .color(WHITE)
            .draw();

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
