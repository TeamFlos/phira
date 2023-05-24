crate::tl_file!("ending");

use super::{draw_background, game::SimpleRecord, loading::UploadFn, NextScene, Scene};
use crate::{
    config::Config,
    ext::{create_audio_manger, screen_aspect, semi_black, semi_white, RectExt, SafeTexture, ScaleType},
    info::ChartInfo,
    judge::PlayResult,
    scene::show_message,
    task::Task,
    ui::{rounded_rect, rounded_rect_shadow, DRectButton, Dialog, MessageHandle, ShadowConfig, Ui},
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

    btn_retry: DRectButton,
    btn_proceed: DRectButton,

    theme_color: Color,
    use_black: bool,
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
        theme_color: Color,
        use_black: bool,
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
            autoplay: config.autoplay(),
            speed: config.speed,
            next: 0,

            upload_fn,
            upload_task,
            record_data,
            record,

            btn_retry: DRectButton::new(),
            btn_proceed: DRectButton::new(),

            theme_color,
            use_black,
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

    fn touch(&mut self, tm: &mut crate::time::TimeManager, touch: &Touch) -> Result<bool> {
        let t = tm.now() as f32;
        if self.btn_retry.touch(touch, t) {
            if self.upload_task.is_some() {
                show_message(tl!("still-uploading"));
            } else {
                self.next = 1;
            }
            return Ok(true);
        }
        if self.btn_proceed.touch(touch, t) {
            if self.upload_task.is_some() {
                show_message(tl!("still-uploading"));
            } else {
                self.next = 2;
            }
            return Ok(true);
        }
        Ok(false)
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
        let t = tm.now() as f32;
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

        let radius = 0.03;

        let ep = 1. - (1. - ran(t, 0.9, 1.9)).powi(3);

        let mut r = Rect::default().nonuniform_feather(0.45 + 0.2 * ep, top * 0.7);
        let p = 1. - (1. - ran(t, 0.1, 0.7)).powi(3);
        r.y += 0.3 * (1. - p);
        let gr = r;
        rounded_rect_shadow(
            ui,
            r,
            &ShadowConfig {
                radius,
                base: p,
                ..Default::default()
            },
        );

        let ir = Rect {
            x: r.x + r.w * 0.2,
            w: r.w * 0.8,
            ..r
        };
        rounded_rect(ui, r, radius, |ui| {
            ui.fill_rect(r, (*self.illustration, r, ScaleType::CropCenter, semi_white(p)));
            ui.fill_rect(r, semi_black(0.3 * ep));
            ui.fill_rect(ir, Color { a: ep, ..self.theme_color });
        });

        let (main, sub) = Ui::main_sub_colors(self.use_black, ep);

        ui.text(&self.info.level)
            .pos(r.x + 0.02, r.bottom() - 0.02)
            .anchor(0., 1.)
            .size(0.5)
            .color(semi_white(ep))
            .draw();

        let lf = ir.x + 0.04;
        ui.text(&self.info.name)
            .pos(lf, r.y + 0.09)
            .anchor(0., 1.)
            .size(0.7)
            .color(main)
            .max_width(ir.right() - lf - 0.02)
            .draw();
        let r = ui
            .text(format!("{:07}", res.score))
            .pos(lf + 0.02, r.y + 0.12)
            .size(1.2)
            .color(main)
            .draw();
        let sr = ui
            .text(format!("{:.2}%", res.accuracy * 100.))
            .pos(r.right() + 0.03, r.bottom())
            .anchor(0., 1.)
            .size(0.8)
            .color(sub)
            .draw();
        ui.scissor(Some(gr));
        ui.text(format!("(±{}ms)", res.std as i32))
            .pos(sr.right() + 0.02, sr.bottom())
            .anchor(0., 1.)
            .size(0.4)
            .color(sub)
            .draw();
        ui.scissor(None);

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
        ui.text(text).pos(r.x, r.bottom() + 0.03).size(0.4).color(main).draw();

        let mut y = r.y + 0.21;
        let lf = r.x + 0.036;
        for (num, text) in res.counts.iter().zip(["Perfect", "Good", "Bad", "Miss"]) {
            ui.text(text).pos(lf, y).no_baseline().size(0.4).color(sub).draw();
            y += 0.035;
            ui.text(num.to_string())
                .pos(lf + 0.02, y)
                .anchor(0., 0.)
                .no_baseline()
                .size(0.64)
                .color(main)
                .draw();
            y += 0.06;
        }

        let mut y = r.y + 0.21;
        let lf = r.x + 0.38;
        for (num, text) in [(res.max_combo, "Max Combo"), (res.early, "Early"), (res.late, "Late")] {
            ui.text(text).pos(lf, y).no_baseline().size(0.4).color(sub).draw();
            y += 0.035;
            ui.text(num.to_string())
                .pos(lf + 0.02, y)
                .anchor(0., 0.)
                .no_baseline()
                .size(0.64)
                .color(main)
                .draw();
            y += 0.06;
        }

        let ct = (0.91, -ui.top + 0.09);
        let rad = 0.05;
        ui.avatar(ct.0, ct.1, rad, semi_white(p), t, Ok(Some(self.player.clone())));
        let rt = ct.0 - rad - 0.02;
        ui.text(&self.player_name)
            .pos(rt, ct.1 + 0.002)
            .anchor(1., 1.)
            .size(0.6)
            .color(semi_white(p))
            .draw();
        ui.text(if let Some(state) = &self.update_state {
            format!("{:.2}", state.new_rks)
        } else if let Some(rks) = &self.player_rks {
            format!("{rks:.2}")
        } else {
            String::new()
        })
        .pos(rt, ct.1 + 0.008)
        .anchor(1., 0.)
        .size(0.4)
        .color(semi_white(p * 0.6))
        .draw();

        let s = 0.14;
        let c = Color { a: ep, ..main };
        let mut r = Rect::new(ir.right() - s - 0.04, ir.bottom() - s - 0.04, s, s);

        let (cr, _) = self.btn_proceed.render_shadow(ui, r, t, ep, |_| semi_white(0.3 * ep));
        let cr = cr.feather(-0.02);
        ui.fill_rect(cr, (*self.icon_proceed, cr, ScaleType::Fit, c));

        r.x -= r.w + 0.03;
        let (cr, _) = self.btn_retry.render_shadow(ui, r, t, ep, |_| semi_white(0.3 * ep));
        let cr = cr.feather(-0.02);
        ui.fill_rect(cr, (*self.icon_retry, cr, ScaleType::Fit, c));

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
