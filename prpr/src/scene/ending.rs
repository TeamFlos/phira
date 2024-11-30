crate::tl_file!("ending");

use super::{draw_background, game::SimpleRecord, loading::UploadFn, NextScene, Scene};
use crate::{
    config::Config,
    core::{BOLD_FONT, PGR_FONT},
    ext::{create_audio_manger, rect_shadow, semi_black, semi_white, RectExt, SafeTexture, ScaleType},
    info::ChartInfo,
    judge::{icon_index, PlayResult},
    scene::show_message,
    task::Task,
    time::TimeManager,
    ui::{clip_sector, DRectButton, Dialog, MessageHandle, Ui},
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
    pub new_rks: Option<f32>,
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

    tr_start: f32,
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
        config: &Config,
        bgm: AudioClip,
        upload_fn: Option<UploadFn>,
        player_rks: Option<f32>,
        historic_best: u32,
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
                let (best, improvement) = if result.score > historic_best {
                    (true, result.score - historic_best)
                } else {
                    (false, 0)
                };
                Some(RecordUpdateState {
                    best,
                    improvement,
                    gain_exp: 0.,
                    new_rks: None,
                })
            },
            rated: upload_task.is_some(),

            info,
            result,
            player_name: config.player_name.clone(),
            player_rks,
            autoplay: config.autoplay(),
            speed: config.speed,
            next: 0,

            upload_fn,
            upload_task,
            record_data,
            record,

            btn_retry: DRectButton::new(),
            btn_proceed: DRectButton::new(),

            tr_start: f32::NAN,
        })
    }
}

thread_local! {
    static RE_UPLOAD: RefCell<bool> = RefCell::default();
}

impl Scene for EndingScene {
    fn enter(&mut self, tm: &mut TimeManager, target: Option<RenderTarget>) -> Result<()> {
        tm.reset();
        tm.seek_to(-0.4);
        self.target = target;
        Ok(())
    }

    fn pause(&mut self, tm: &mut TimeManager) -> Result<()> {
        self.bgm.pause()?;
        tm.pause();
        Ok(())
    }

    fn resume(&mut self, tm: &mut TimeManager) -> Result<()> {
        self.bgm.play()?;
        tm.resume();
        Ok(())
    }

    fn touch(&mut self, tm: &mut TimeManager, touch: &Touch) -> Result<bool> {
        let t = tm.now() as f32;
        if self.btn_retry.touch(touch, t) {
            if self.upload_task.is_some() {
                show_message(tl!("still-uploading"));
            } else {
                self.tr_start = t;
                self.next = 1;
            }
            return Ok(true);
        }
        if self.btn_proceed.touch(touch, t) {
            if self.upload_task.is_some() {
                show_message(tl!("still-uploading"));
            } else {
                self.tr_start = t;
                self.next = 2;
            }
            return Ok(true);
        }
        Ok(false)
    }

    fn update(&mut self, tm: &mut TimeManager) -> Result<()> {
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
                            .listener(move |_dialog, pos| {
                                if pos == 1 {
                                    RE_UPLOAD.with(|it| *it.borrow_mut() = true);
                                }
                                false
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

    fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        let mut cam = ui.camera();
        let asp = -cam.zoom.y;
        let top = 1. / asp;
        let t = tm.now() as f32;
        cam.render_target = self.target;
        let sr = ui.screen_rect();
        set_camera(&cam);
        draw_background(*self.background);

        fn ran(t: f32, l: f32, r: f32) -> f32 {
            ((t - l) / (r - l)).clamp(0., 1.)
        }

        let ct = vec2(-0.55, 1.2);
        let start = vec2(1.25, 0.9) - ct;
        let end = vec2(-0.15, -0.7) - ct;
        let angle_start = start.y.atan2(start.x) * 0.4;
        let angle_end = end.y.atan2(end.x);
        let center_angle = 1.8;

        let p = ran(t, 0.1, 1.8);
        let p = 1. - (1. - p).powi(3);
        let sector_start = p * (angle_end - angle_start - center_angle) + angle_start;
        let project_y = ct.y + (1. - ct.x) * (sector_start + center_angle).sin();

        let pf = ran(t, 2., 2.4);

        if project_y < top {
            let c = ui.background();
            let y = -top + 0.12;
            let br = Rect::new(-1., y, 2., 0.34);
            ui.fill_rect(br, (c, (-1., y), Color { a: 0.1, ..c }, (1., y + 0.3)));

            let res = &self.result;

            let y = y - 0.07;
            ui.fill_rect(Rect::new(-1., y, 2., 0.07), Color { a: 0.3, ..c });
            let r = ui
                .text(&self.info.name)
                .pos(-0.53 + (1.2 - y) / 1.9 * 0.4, y + 0.012)
                .color(semi_white(0.6))
                .max_width(0.8)
                .size(0.56)
                .draw();
            ui.text(&self.info.level)
                .pos(0.97, r.y)
                .anchor(1., 0.)
                .size(0.56)
                .color(semi_white(0.7))
                .draw();

            let icon = &self.icons[icon_index(res.score, res.max_combo == res.num_of_notes)];
            let p = ran(t, 1.7, 2.4).powi(2);
            let r = Rect::new(0.75, br.center().y, 0., 0.).feather(0.13 + (1. - p) * 0.05);
            ui.fill_rect(r, (**icon, r, ScaleType::Fit, semi_white(p)));

            let y = y + 0.16;
            let lf = -0.48 + (1.2 - y) / 1.9 * 0.4;
            let mut x = lf;
            let p = ran(t, 0.9, 2.6);
            let mut digits = Vec::with_capacity(7);
            let mut s = res.score;
            for _ in 0..7 {
                digits.push(s % 10);
                s /= 10;
            }
            digits.reverse();
            let s = 1.5;
            let sr = ui.text("0").size(s).measure_using(&PGR_FONT);
            let h = sr.h;
            ui.scissor(Rect::new(-1., y, 2., h + 0.01), |ui| {
                for (i, d) in digits.into_iter().enumerate() {
                    let p = (p * (1. + (0.16 * (6 - i) as f32).powi(2))).min(1.);
                    let p = 1. - (1. - p).powi(3);
                    let mut p = d as f32 + (1. - p) * 7.;
                    if p > 10. {
                        p -= 10.;
                    }
                    let up = p as u32;
                    let dw = (up + 1) % 10;
                    let o = -h * (p - up as f32);
                    ui.text(up.to_string())
                        .pos(x + sr.w / 2., y + o)
                        .anchor(0.5, 0.)
                        .size(s)
                        .draw_using(&PGR_FONT);
                    ui.text(dw.to_string())
                        .pos(x + sr.w / 2., y + h + o)
                        .anchor(0.5, 0.)
                        .size(s)
                        .draw_using(&PGR_FONT);
                    x += sr.w;
                }
            });

            if let Some(s) = &self.update_state {
                if s.best {
                    ui.text(format!("{}  {:+07}", tl!("new-best"), s.improvement))
                        .pos(x - 0.01, y - 0.016)
                        .anchor(1., 1.)
                        .color(semi_white(pf))
                        .size(0.5)
                        .draw_using(&BOLD_FONT);
                }
            }

            let cl = semi_white(0.6);
            let ct = semi_white(0.8);
            let cs = semi_white(0.4);
            let s = 0.5;

            let r = ui
                .text(tl!("accuracy"))
                .pos(lf - 0.017, y + h + 0.03)
                .color(cl)
                .size(s)
                .draw_using(&BOLD_FONT);
            let r = ui
                .text(format!("{:.2}%", res.accuracy * 100.))
                .pos(r.right() + 0.02, r.y)
                .color(ct)
                .size(s)
                .draw_using(&BOLD_FONT);

            let r = ui.text("|").pos(r.right() + 0.03, r.y).color(cs).size(s).draw();

            let r = ui.text(tl!("error")).pos(r.right() + 0.03, r.y).color(cl).size(s).draw_using(&BOLD_FONT);
            ui.text(format!("±{}ms", (res.std * 1000.).round() as i32))
                .pos(r.right() + 0.02, r.y)
                .size(s)
                .color(ct)
                .draw_using(&BOLD_FONT);

            let mut y = -top + 0.4 + ui.top * 0.3;
            let tp = y;
            let mut x = -0.26 + (1.2 - y) / 1.9 * 0.4;
            let lf = x;
            let s = 0.64;
            for (title, num) in ["PERFECT", "GOOD", "BAD", "MISS"].into_iter().zip(res.counts) {
                ui.text(title)
                    .pos(x, y)
                    .anchor(1., 0.)
                    .color(semi_white(0.6))
                    .size(s)
                    .draw_using(&BOLD_FONT);
                let r = ui.text(num.to_string()).pos(x + 0.06, y).size(s).draw_using(&BOLD_FONT);
                let dy = r.h + 0.03;
                y += dy;
                x -= dy / 1.9 * 0.4;
            }

            let p = ran(t, 0.8, 1.8);
            let p = 1. - (1. - p).powi(3);
            let mut y = tp;
            let mut x = lf + 0.42;
            let r = ui
                .text(tl!("max-combo"))
                .pos(x, y)
                .anchor(1., 0.)
                .color(semi_white(0.6))
                .size(s)
                .draw_using(&BOLD_FONT);
            let mut r = Rect::new(r.right() + 0.03, r.y + 0.004, 0.45, r.h);
            let draw_par = |ui: &mut Ui, r: Rect, p: f32, c: Color| {
                let sl = 1.9 / 0.4;
                let w = p * r.w;
                let d = r.h / sl;
                let mut b = ui.builder(c);
                b.add(r.x, r.bottom());
                if w < d {
                    b.add(r.x + w, r.bottom());
                    b.add(r.x + w, r.bottom() - w * sl);
                    b.triangle(0, 1, 2);
                } else {
                    b.add(r.x + d, r.y);
                    b.add(r.x + w, r.y);
                    b.add(r.x + w.min(r.w - d), r.bottom());
                    b.triangle(0, 1, 2);
                    b.triangle(0, 2, 3);
                    if w + d > r.right() {
                        b.add(r.x + w, r.y + (r.w - w) * sl);
                        b.triangle(2, 3, 4);
                    }
                }
                b.commit();
            };
            draw_par(ui, r, 1., semi_black(0.4));
            let ct = r.center();
            let combo = (res.max_combo as f32 * p).round() as u32;
            let text = format!("{combo} / {}", res.num_of_notes);
            ui.text(&text)
                .pos(ct.x, ct.y)
                .anchor(0.5, 0.5)
                .no_baseline()
                .size(0.4)
                .draw_using(&BOLD_FONT);
            let p = combo as f32 / res.num_of_notes as f32;
            draw_par(ui, r, p, WHITE);
            r.w *= p;
            ui.scissor(r, |ui| {
                ui.text(text)
                    .pos(ct.x, ct.y)
                    .anchor(0.5, 0.5)
                    .no_baseline()
                    .size(0.4)
                    .color(BLACK)
                    .draw_using(&BOLD_FONT);
            });

            let dy = r.h + 0.03;
            y += dy;
            x -= dy / 1.9 * 0.4;

            let r = ui
                .text(tl!("rks-delta"))
                .pos(x, y)
                .anchor(1., 0.)
                .color(semi_white(0.6))
                .size(s)
                .draw_using(&BOLD_FONT);
            let text = if let Some((new_rks, now)) = self.update_state.as_ref().and_then(|it| it.new_rks).zip(self.player_rks) {
                let delta = new_rks - now;
                if delta.abs() > 1e-5 {
                    format!("{:+.2}", delta)
                } else {
                    "-".to_owned()
                }
            } else {
                "-".to_owned()
            };
            ui.text(text).pos(r.right() + 0.03, y).size(s).draw_using(&BOLD_FONT);

            let mut r = Rect::new(0.96, ui.top - 0.04, 0.25, 0.1);
            r.x -= r.w;
            r.y -= r.h;
            self.btn_proceed.render_shadow(ui, r, t, |ui, path| {
                ui.fill_path(&path, Color::from_hex(0x3f51b5));
                let ir = Rect::new(r.x + 0.05, r.center().y, 0., 0.).feather(0.03);
                ui.fill_rect(ir, (*self.icon_proceed, ir));
                ui.text(tl!("proceed"))
                    .pos((ir.right() + r.right() - 0.01) / 2., r.center().y)
                    .anchor(0.5, 0.5)
                    .no_baseline()
                    .size(0.44)
                    .draw_using(&BOLD_FONT);
            });

            r.x -= r.w + 0.02;
            self.btn_retry.render_shadow(ui, r, t, |ui, path| {
                ui.fill_path(&path, Color::from_hex(0x78909c));
                let ir = Rect::new(r.x + 0.05, r.center().y, 0., 0.).feather(0.03);
                ui.fill_rect(ir, (*self.icon_retry, ir));
                ui.text(tl!("retry"))
                    .pos((ir.right() + r.right() - 0.01) / 2., r.center().y)
                    .anchor(0.5, 0.5)
                    .no_baseline()
                    .size(0.44)
                    .draw_using(&BOLD_FONT);
            });

            let spd = if (self.speed - 1.).abs() <= 1e-4 {
                String::new()
            } else {
                format!("{:.2}x", self.speed)
            };
            let text = if self.autoplay {
                format!("AUTOPLAY {spd}")
            } else if !self.rated {
                format!("UNRATED {spd}")
            } else {
                spd
            };
            let text = text.trim();
            if !text.is_empty() {
                let ty = br.bottom();
                let x = -0.55 + (1.2 - ty) / 1.9 * 0.4;
                let h = 0.04;
                let mut text = ui
                    .text(text)
                    .pos(x + 0.02, ty - h / 2.)
                    .anchor(0., 0.5)
                    .no_baseline()
                    .color(semi_black(0.6))
                    .size(0.5);
                let tr = text.measure_using(&BOLD_FONT);
                let r = Rect::new(-1., tr.y, tr.right() + 1.03, tr.h);
                let mut b = text.ui.builder(WHITE);
                b.add(-1., tr.y);
                b.add(r.right(), tr.y);
                b.add(r.right() - tr.h / 1.9 * 0.4, tr.bottom());
                b.add(-1., tr.bottom());
                b.triangle(0, 1, 2);
                b.triangle(0, 2, 3);
                b.commit();

                text.draw_using(&BOLD_FONT);
            }
        }
        clip_sector(ui, ct, sector_start, sector_start + center_angle, |ui| {
            ui.fill_rect(sr, (*self.illustration, sr));
        });
        let sector_start = (p * 1.4 - 0.3).max(0.) * (angle_end - angle_start - center_angle) + angle_start;
        clip_sector(ui, ct, sector_start, sector_start + center_angle * 0.5, |ui| {
            ui.fill_rect(sr, (*self.illustration, sr.feather(0.15)));
        });

        ui.alpha(pf, |ui| {
            let s = 0.05;
            let pad = 0.02;
            let mw = 0.4;
            let w = s * 2. + pad + ui.text(&self.player_name).size(0.6).measure().w.min(mw) + 0.02;
            let r = Rect::new(-0.96, -top + 0.04, w, s * 2.);
            ui.fill_path(&r.feather(0.01).rounded(s + 0.01), semi_black(0.6));
            ui.fill_rect(Rect::new(r.x, r.y + s + 0.003, r.w + 0.01, 0.).nonuniform_feather(-0.01, 0.002), WHITE);
            ui.avatar(r.x + s, r.y + s, s, t, Ok(Some(self.player.clone())));
            let lf = r.x + s * 2. + pad;
            ui.text(&self.player_name)
                .pos(lf, r.y + s - 0.007)
                .anchor(0., 1.)
                .max_width(mw)
                .size(0.6)
                .draw();
            ui.text(if let Some(new_rks) = self.update_state.as_ref().and_then(|it| it.new_rks) {
                format!("{new_rks:.2}")
            } else if let Some(rks) = &self.player_rks {
                format!("{rks:.2}")
            } else {
                String::new()
            })
            .pos(lf, r.y + s + 0.008)
            .size(0.4)
            .color(semi_white(0.6))
            .draw();
        });

        if !self.tr_start.is_nan() {
            let p = ((t - self.tr_start) / 0.5).min(1.);
            if p >= 1. {
                self.tr_start = f32::NAN;
            }
            let p = 1. - (1. - p).powi(3);
            let mut r = sr;
            r.y -= r.h * (1. - p);
            rect_shadow(r, 0.01, 0.5);
            let (tex, alpha) = if self.next == 1 {
                (&self.background, 0.3)
            } else {
                (&self.illustration, 0.55)
            };
            ui.fill_rect(r, (**tex, r));
            ui.fill_rect(r, semi_black(alpha));
        }

        Ok(())
    }

    fn next_scene(&mut self, _tm: &mut TimeManager) -> NextScene {
        if !self.tr_start.is_nan() {
            return NextScene::None;
        }
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
