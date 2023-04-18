crate::tl_file!("ending");

use super::{draw_background, draw_illustration, game::SimpleRecord, loading::UploadFn, NextScene, Scene};
use crate::{
    config::Config,
    ext::{
        create_audio_manger, draw_parallelogram, draw_parallelogram_ex, draw_text_aligned, screen_aspect, SafeTexture, ScaleType, PARALLELOGRAM_SLOPE,
    },
    info::ChartInfo,
    judge::{icon_index, Judge, PlayResult},
    scene::show_message,
    task::Task,
    ui::{Dialog, MessageHandle, Ui},
};
use anyhow::Result;
use macroquad::prelude::*;
use sasa::{AudioClip, AudioManager, Music, MusicParams};
use serde::Deserialize;
use std::{cell::RefCell, ops::DerefMut};

#[derive(Deserialize)]
pub struct RecordUpdateState {
    pub best: bool,
    pub improvement: u32,
    pub gain_exp: f32,
    pub new_rks: f32,
}

pub struct EndingScene {
    background: SafeTexture,
    illustration: SafeTexture,
    player: SafeTexture,
    icons: [SafeTexture; 8],
    icon_retry: SafeTexture,
    icon_proceed: SafeTexture,
    target: Option<RenderTarget>,
    audio: AudioManager,
    bgm: Music,

    info: ChartInfo,
    result: PlayResult,
    player_name: String,
    player_rks: Option<f32>,
    challenge_texture: SafeTexture,
    challenge_rank: u32,
    autoplay: bool,
    speed: f32,
    next: u8, // 0 -> none, 1 -> pop, 2 -> exit
    update_state: Option<RecordUpdateState>,
    rated: bool,

    upload_fn: Option<UploadFn>,
    upload_task: Option<(Task<Result<RecordUpdateState>>, MessageHandle)>,
    record_data: Option<Vec<u8>>,
    record: Option<SimpleRecord>,
}

impl EndingScene {
    pub fn new(
        background: SafeTexture,
        illustration: SafeTexture,
        player: SafeTexture,
        icons: [SafeTexture; 8],
        icon_retry: SafeTexture,
        icon_proceed: SafeTexture,
        info: ChartInfo,
        result: PlayResult,
        challenge_texture: SafeTexture,
        config: &Config,
        bgm: AudioClip,
        upload_fn: Option<UploadFn>,
        player_rks: Option<f32>,
        record_data: Option<Vec<u8>>,
        record: Option<SimpleRecord>,
    ) -> Result<Self> {
        let mut audio = create_audio_manger(config)?;
        let bgm = audio.create_music(
            bgm,
            MusicParams {
                amplifier: config.volume_music,
                loop_mix_time: 0.,
                ..Default::default()
            },
        )?;
        let upload_task = upload_fn
            .as_ref()
            .and_then(|f| record_data.clone().map(|data| (f(data), show_message(tl!("uploading")).handle())));
        Ok(Self {
            background,
            illustration,
            player,
            icons,
            icon_retry,
            icon_proceed,
            target: None,
            audio,
            bgm,
            update_state: if upload_task.is_some() {
                None
            } else {
                Some(RecordUpdateState {
                    best: true,
                    improvement: result.score,
                    gain_exp: 0.,
                    new_rks: 0.,
                })
            },
            rated: upload_task.is_some(),

            info,
            result,
            player_name: config.player_name.clone(),
            player_rks,
            challenge_texture,
            challenge_rank: config.challenge_rank,
            autoplay: config.autoplay,
            speed: config.speed,
            next: 0,

            upload_fn,
            upload_task,
            record_data,
            record,
        })
    }
}

thread_local! {
    static RE_UPLOAD: RefCell<bool> = RefCell::default();
}

impl Scene for EndingScene {
    fn enter(&mut self, tm: &mut crate::time::TimeManager, target: Option<RenderTarget>) -> Result<()> {
        tm.reset();
        tm.seek_to(-0.4);
        self.target = target;
        Ok(())
    }

    fn pause(&mut self, tm: &mut crate::time::TimeManager) -> Result<()> {
        self.bgm.pause()?;
        tm.pause();
        Ok(())
    }

    fn resume(&mut self, tm: &mut crate::time::TimeManager) -> Result<()> {
        self.bgm.play()?;
        tm.resume();
        Ok(())
    }

    fn update(&mut self, tm: &mut crate::time::TimeManager) -> Result<()> {
        self.audio.recover_if_needed()?;
        if tm.now() >= 0. && self.target.is_none() && self.bgm.paused() {
            self.bgm.play()?;
        }
        if RE_UPLOAD.with(|it| std::mem::replace(it.borrow_mut().deref_mut(), false)) && self.upload_task.is_none() {
            self.upload_task = self
                .record_data
                .clone()
                .map(|data| ((self.upload_fn.as_ref().unwrap())(data), show_message(tl!("uploading")).handle()));
        }
        if let Some((task, handle)) = &mut self.upload_task {
            if let Some(result) = task.take() {
                handle.cancel();
                match result {
                    Err(err) => {
                        let error = format!("{:?}", err.context(tl!("upload-failed")));
                        Dialog::plain(tl!("upload-failed"), error)
                            .buttons(vec![tl!("upload-cancel").to_string(), tl!("upload-retry").to_string()])
                            .listener(move |pos| {
                                if pos == 1 {
                                    RE_UPLOAD.with(|it| *it.borrow_mut() = true);
                                }
                            })
                            .show();
                    }
                    Ok(state) => {
                        self.update_state = Some(state);
                        show_message(tl!("uploaded")).ok();
                    }
                }
                self.upload_task = None;
            }
        }
        Ok(())
    }

    fn render(&mut self, tm: &mut crate::time::TimeManager, ui: &mut Ui) -> Result<()> {
        let asp = screen_aspect();
        let top = 1. / asp;
        let now = tm.now() as f32;
        let gl = unsafe { get_internal_gl() }.quad_gl;
        let res = &self.result;
        set_camera(&Camera2D {
            zoom: vec2(1., -asp),
            render_target: self.target,
            ..Default::default()
        });
        draw_background(*self.background);

        fn ran(t: f32, l: f32, r: f32) -> f32 {
            ((t - l) / (r - l)).clamp(0., 1.)
        }
        fn tran(gl: &mut QuadGl, x: f32) {
            gl.push_model_matrix(Mat4::from_translation(vec3(x * 2., 0., 0.)));
        }

        tran(gl, (1. - ran(now, 0.1, 1.3)).powi(3));
        let r = draw_illustration(*self.illustration, -0.38, 0., 1., 1.2, WHITE);
        let slope = PARALLELOGRAM_SLOPE;
        let ratio = 0.2;
        draw_parallelogram_ex(
            Rect::new(r.x, r.y + r.h * (1. - ratio), r.w - r.h * (1. - ratio) * slope, r.h * ratio),
            None,
            Color::default(),
            Color::new(0., 0., 0., 0.7),
            false,
        );
        let rr = draw_text_aligned(ui, &self.info.level, r.right() - r.h / 7. * 13. * 0.13 - 0.01, r.bottom() - top / 20., (1., 1.), 0.46, WHITE);
        let p = (r.x + 0.04, r.bottom() - top / 20.);
        let mw = rr.x - 0.02 - p.0;
        let mut text = ui.text(&self.info.name).pos(p.0, p.1).anchor(0., 1.).size(0.7);
        if text.measure().w <= mw {
            text.draw();
        } else {
            drop(text);
            ui.text(&self.info.name).pos(p.0, p.1).anchor(0., 1.).size(0.5).max_width(mw).draw();
        }
        gl.pop_model_matrix();

        let dx = 0.06;
        let c = Color::new(0., 0., 0., 0.6);

        tran(gl, (1. - ran(now, 0.2, 1.3)).powi(3));
        let main = Rect::new(r.right() - 0.05, r.y, r.w * 0.84, r.h / 2.);
        draw_parallelogram(main, None, c, true);
        {
            let spd = if (self.speed - 1.).abs() <= 1e-4 {
                String::new()
            } else {
                format!(" {:.2}x", self.speed)
            };
            let text = if self.autoplay {
                format!("PHIRA[AUTOPLAY] {spd}")
            } else if !self.rated {
                format!("PHIRA[UNRATED] {spd}")
            } else if let Some(state) = &self.update_state {
                format!(
                    "PHIRA {spd}  {}",
                    if state.best {
                        format!("NEW BEST +{:07}", state.improvement)
                    } else {
                        String::new()
                    }
                )
            } else {
                "Uploading…".to_owned()
            };
            let r = draw_text_aligned(ui, &text, main.x + dx, main.bottom() - 0.035, (0., 1.), 0.34, WHITE);
            let r = draw_text_aligned(ui, &format!("{:07}", res.score), r.x, r.y - 0.023, (0., 1.), 1., WHITE);
            let icon = icon_index(res.score, res.num_of_notes == res.max_combo);
            let p = ran(now, 1.4, 1.9).powi(2);
            let s = main.h * 0.67;
            let ct = (main.right() - main.h * slope - s / 2., r.bottom() + 0.02 - s / 2.);
            let s = s + s * (1. - p) * 0.3;
            draw_texture_ex(
                *self.icons[icon],
                ct.0 - s / 2.,
                ct.1 - s / 2.,
                Color::new(1., 1., 1., p),
                DrawTextureParams {
                    dest_size: Some(vec2(s, s)),
                    ..Default::default()
                },
            );
        }
        gl.pop_model_matrix();

        tran(gl, (1. - ran(now, 0.4, 1.5)).powi(3));
        let d = r.h / 16.;
        let s1 = Rect::new(main.x - d * 4. * slope, main.bottom() + d, main.w - d * 5. * slope, d * 3.);
        draw_parallelogram(s1, None, c, true);
        {
            let dy = 0.025;
            let r = draw_text_aligned(ui, "Max Combo", s1.x + dx, s1.bottom() - dy, (0., 1.), 0.34, WHITE);
            draw_text_aligned(ui, &res.max_combo.to_string(), r.x, r.y - 0.01, (0., 1.), 0.7, WHITE);
            let r = draw_text_aligned(ui, "Accuracy", s1.right() - dx, s1.bottom() - dy, (1., 1.), 0.34, WHITE);
            draw_text_aligned(ui, &format!("{:.2}%", res.accuracy * 100.), r.right(), r.y - 0.01, (1., 1.), 0.7, WHITE);
        }
        gl.pop_model_matrix();

        tran(gl, (1. - ran(now, 0.5, 1.7)).powi(3));
        let s2 = Rect::new(s1.x - d * 4. * slope, s1.bottom() + d, s1.w, s1.h);
        draw_parallelogram(s2, None, c, true);
        {
            let dy = 0.025;
            let dy2 = 0.015;
            let bg = 0.57;
            let sm = 0.26;
            let draw_count = |ui: &mut Ui, ratio: f32, name: &str, count: u32| {
                let r = draw_text_aligned(ui, name, s2.x + s2.w * ratio, s2.bottom() - dy, (0.5, 1.), sm, WHITE);
                draw_text_aligned(ui, &count.to_string(), r.center().x, r.y - dy2, (0.5, 1.), bg, WHITE);
            };
            draw_count(ui, 0.14, "Perfect", res.counts[0]);
            draw_count(ui, 0.33, "Good", res.counts[1]);
            draw_count(ui, 0.46, "Bad", res.counts[2]);
            draw_count(ui, 0.59, "Miss", res.counts[3]);

            let sm = 0.3;
            let l = s2.x + s2.w * 0.72;
            let rt = s2.x + s2.w * 0.94;
            let cy = s2.center().y;
            let r = draw_text_aligned(ui, "Early", l, cy - dy2 / 2., (0., 1.), sm, WHITE);
            draw_text_aligned(ui, &res.early.to_string(), rt, r.bottom(), (1., 1.), sm, WHITE);
            let r = draw_text_aligned(ui, "Late", l, cy + dy2 / 2., (0., 0.), 0.3, WHITE);
            draw_text_aligned(ui, &res.late.to_string(), rt, r.y, (1., 0.), sm, WHITE);
        }
        gl.pop_model_matrix();

        fn touched(rect: Rect) -> bool {
            Judge::get_touches()
                .iter()
                .any(|touch| touch.phase == TouchPhase::Ended && rect.contains(touch.position))
        }

        let dy = 0.006;
        let w = 0.17;
        let p = (1. - ran(now, 2., 2.7)).powi(2);
        let h = 0.1;
        let s = 0.05;
        let hs = h * 0.3;
        let params = DrawTextureParams {
            dest_size: Some(vec2(hs * 2., hs * 2.)),
            ..Default::default()
        };
        tran(gl, -p * 0.085);
        let r = Rect::new(-1. - h * slope, -top + dy, w, h);
        draw_parallelogram(r, None, c, true);
        draw_parallelogram(Rect::new(r.x + r.w * (1. - s), r.y, r.w * s, r.h), None, WHITE, false);
        let ct = r.center();
        draw_texture_ex(*self.icon_retry, ct.x - hs, ct.y - hs, WHITE, params.clone());
        gl.pop_model_matrix();
        if p <= 0. && touched(r) {
            if self.upload_task.is_some() {
                show_message(tl!("still-uploading"));
            }
            self.next = 1;
        }

        tran(gl, p * 0.085);
        let r = Rect::new(1. + h * slope - w, top - dy - h, w, h);
        draw_parallelogram(r, None, c, true);
        draw_parallelogram(Rect::new(r.x + r.w * s, r.y, r.w * s, r.h), None, WHITE, false);
        let ct = r.center();
        draw_texture_ex(*self.icon_proceed, ct.x - hs, ct.y - hs, WHITE, params);
        gl.pop_model_matrix();
        if p <= 0. && touched(r) {
            if self.upload_task.is_some() {
                show_message(tl!("still-uploading"));
            }
            self.next = 2;
        }

        let alpha = ran(now, 1.5, 1.9);
        let main = Rect::new(1. - 0.28, -top + dy * 2.5, 0.35, 0.1);
        draw_parallelogram(main, None, Color::new(0., 0., 0., c.a * alpha), false);
        let sub = Rect::new(1. - 0.13, main.center().y + 0.01, 0.12, 0.03);
        let color = Color::new(1., 1., 1., alpha);
        draw_parallelogram(sub, None, color, false);
        draw_text_aligned(
            ui,
            &if let Some(state) = &self.update_state {
                format!("{:.2}", state.new_rks)
            } else if let Some(rks) = &self.player_rks {
                format!("{rks:.2}")
            } else {
                "".to_owned()
            },
            sub.center().x,
            sub.center().y,
            (0.5, 0.5),
            0.37,
            Color::new(0., 0., 0., alpha),
        );
        let r = draw_illustration(*self.player, 1. - 0.21, main.center().y, 0.12 / (0.076 * 7.), 0.12 / (0.076 * 7.), color);
        let text = draw_text_aligned(ui, &self.player_name, r.x - 0.01, r.center().y, (1., 0.5), 0.54, color);
        draw_parallelogram(
            Rect::new(text.x - main.h * slope - 0.01, main.y, r.x - text.x + main.h * slope * 2. + 0.013, main.h),
            None,
            Color::new(0., 0., 0., c.a * alpha),
            false,
        );
        draw_text_aligned(ui, &self.player_name, r.x - 0.01, r.center().y, (1., 0.5), 0.54, color);

        let ct = (1. - 0.1 + 0.043, main.center().y - 0.034 + 0.02);
        let (w, h) = (0.09 * self.challenge_texture.width() / 78., 0.04 * self.challenge_texture.height() / 38.);
        let r = Rect::new(ct.0 - w / 2., ct.1 - h / 2., w, h);
        ui.fill_rect(r, (*self.challenge_texture, r, ScaleType::Fit, color));
        let ct = r.center();
        ui.text(self.challenge_rank.to_string())
            .pos(ct.x, ct.y)
            .anchor(0.5, 1.)
            .size(0.46)
            .color(color)
            .draw();

        Ok(())
    }

    fn next_scene(&mut self, _tm: &mut crate::time::TimeManager) -> NextScene {
        if self.next != 0 {
            let _ = self.bgm.pause();
        }
        match self.next {
            0 => NextScene::None,
            1 => NextScene::Pop,
            2 => {
                if let Some(rec) = &self.record {
                    NextScene::PopNWithResult(2, Box::new(rec.clone()))
                } else {
                    NextScene::PopN(2)
                }
            }
            _ => unreachable!(),
        }
    }
}
