prpr::tl_file!("rate");

use crate::page::Fader;
use macroquad::prelude::*;
use prpr::{
    ext::{semi_black, semi_white, RectExt, SafeTexture, ScaleType},
    ui::{DRectButton, Ui}, scene::show_message,
};

pub struct RateDialog {
    fader: Fader,
    show: bool,

    icon_star: SafeTexture,
    pub score: i16,

    btn_cancel: DRectButton,
    btn_confirm: DRectButton,
    pub confirmed: Option<bool>,

    touch_x: Option<f32>,
    touch_rect: Rect,
}

impl RateDialog {
    pub fn new(icon_star: SafeTexture) -> Self {
        Self {
            fader: Fader::new().with_distance(-0.4).with_time(0.5),
            show: false,

            icon_star,
            score: 0,

            btn_cancel: DRectButton::new(),
            btn_confirm: DRectButton::new(),
            confirmed: None,

            touch_x: None,
            touch_rect: Rect::default(),
        }
    }

    pub fn showing(&self) -> bool {
        self.show
    }

    pub fn enter(&mut self, t: f32) {
        self.fader.sub(t);
    }

    pub fn dismiss(&mut self, t: f32) {
        self.show = false;
        self.fader.back(t);
    }

    pub fn touch(&mut self, touch: &Touch, t: f32) -> bool {
        if self.fader.transiting() {
            return true;
        }
        if self.show {
            if !Ui::dialog_rect().contains(touch.position) && touch.phase == TouchPhase::Started {
                self.dismiss(t);
                return true;
            }
            if self.btn_cancel.touch(touch, t) {
                self.confirmed = Some(false);
                self.dismiss(t);
                return true;
            }
            if self.btn_confirm.touch(touch, t) {
                if self.score != 0 {
                    self.confirmed = Some(true);
                }
                return true;
            }
            if self.touch_x.is_some() || self.touch_rect.contains(touch.position) {
                if matches!(touch.phase, TouchPhase::Ended | TouchPhase::Cancelled) {
                    self.touch_x = None;
                } else {
                    self.touch_x = Some(touch.position.x);
                }
            }
            return true;
        }
        false
    }

    pub fn update(&mut self, t: f32) {
        if let Some(done) = self.fader.done(t) {
            self.show = !done;
        }
    }

    pub fn render(&mut self, ui: &mut Ui, t: f32) {
        self.fader.reset();
        if self.show || self.fader.transiting() {
            let p = if self.show { 1. } else { -self.fader.progress(t) };
            ui.fill_rect(ui.screen_rect(), semi_black(p * 0.7));
            self.fader.for_sub(|f| {
                f.render(ui, t, |ui, c| {
                    let wr = Ui::dialog_rect().nonuniform_feather(0., -0.1);
                    ui.fill_path(&wr.rounded(0.02), Color { a: c.a, ..ui.background() });
                    let r = ui.text(tl!("rate")).pos(wr.x + 0.04, wr.y + 0.033).size(0.9).color(c).draw();
                    let bh = 0.09;
                    ui.scope(|ui| {
                        ui.dx(wr.center().x);
                        ui.dy(r.bottom() + 0.07);
                        let s = 0.1;
                        let pad = 0.03;
                        let cc = semi_white(c.a * 0.5);
                        let tw = s * 2.5 + pad * 2.;
                        self.touch_rect = ui.rect_to_global(Rect::new(-tw, s / 2., tw * 2., s));
                        if let Some(x) = self.touch_x {
                            let rw = (x - self.touch_rect.x) / self.touch_rect.w * tw * 2. + pad;
                            let index = (rw / (pad + s)) as i16;
                            let rem = rw - index as f32 * (pad + s);
                            self.score = index * 2;
                            if rem > pad / 2. {
                                self.score += 1;
                                if rem > pad + s / 2. {
                                    self.score += 1;
                                }
                            }
                            self.score = self.score.clamp(1, 10);
                        }
                        for i in 0..5 {
                            let pos = (i as f32 - 2.) * (pad + s);
                            let r = Rect::new(pos, s / 2., 0., 0.).feather(s / 2.);
                            if self.score >= (i + 1) * 2 {
                                ui.fill_rect(r, (*self.icon_star, r, ScaleType::Fit, c));
                            } else {
                                ui.fill_rect(r, (*self.icon_star, r, ScaleType::Fit, cc));
                                if self.score == i * 2 + 1 {
                                    let hr = Rect { w: r.w / 2., ..r };
                                    ui.fill_rect(hr, (*self.icon_star, r, ScaleType::Fit, c));
                                }
                            }
                        }
                    });
                    let pad = 0.02;
                    let bw = (wr.w - pad * 3.) / 2.;
                    let mut r = Rect::new(wr.x + pad, wr.bottom() - 0.02 - bh, bw, bh);
                    self.btn_cancel.render_text(ui, r, t, c.a, tl!("cancel"), 0.5, true);
                    r.x += bw + pad;
                    self.btn_confirm.render_text(ui, r, t, c.a, tl!("confirm"), 0.5, true);
                });
            });
        }
    }
}
