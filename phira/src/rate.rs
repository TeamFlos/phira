prpr::tl_file!("rate");

use crate::page::Fader;
use macroquad::prelude::*;
use prpr::{
    core::BOLD_FONT,
    ext::{semi_black, semi_white, RectExt, SafeTexture, ScaleType},
    ui::{DRectButton, Ui},
};

pub struct Rate {
    pub score: i16,

    touch_x: Option<f32>,
    touch_rect: Rect,
}

impl Rate {
    pub fn new() -> Self {
        Self {
            score: 0,

            touch_x: None,
            touch_rect: Rect::default(),
        }
    }

    pub fn touch(&mut self, touch: &Touch) {
        if self.touch_x.is_some() || self.touch_rect.contains(touch.position) {
            if matches!(touch.phase, TouchPhase::Ended | TouchPhase::Cancelled) {
                self.touch_x = None;
            } else {
                self.touch_x = Some(touch.position.x);
            }
        }
    }

    pub fn render(&mut self, ui: &mut Ui, icon_star: &SafeTexture) -> Rect {
        let wr = Ui::dialog_rect();
        ui.scope(|ui| {
            ui.dx(wr.center().x);
            let s = 0.1;
            let pad = 0.03;
            let cc = semi_white(0.5);
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
                self.score = self.score.clamp(0, 10);
            }
            for i in 0..5 {
                let pos = (i as f32 - 2.) * (pad + s);
                let r = Rect::new(pos, s / 2., 0., 0.).feather(s / 2.);
                if self.score >= (i + 1) * 2 {
                    ui.fill_rect(r, (**icon_star, r, ScaleType::Fit));
                } else {
                    ui.fill_rect(r, (**icon_star, r, ScaleType::Fit, cc));
                    if self.score == i * 2 + 1 {
                        let hr = Rect { w: r.w / 2., ..r };
                        ui.fill_rect(hr, (**icon_star, r, ScaleType::Fit));
                    }
                }
            }
        });
        self.touch_rect
    }
}

pub struct RateDialog {
    fader: Fader,
    show: bool,

    icon_star: SafeTexture,

    btn_cancel: DRectButton,
    btn_confirm: DRectButton,
    btn_tags: DRectButton,
    pub confirmed: Option<bool>,
    pub show_tags: bool,

    pub rate: Rate,
    pub rate_upper: Option<Rate>,
}

impl RateDialog {
    pub fn new(icon_star: SafeTexture, range: bool) -> Self {
        Self {
            fader: Fader::new().with_distance(-0.4).with_time(0.5),
            show: false,

            icon_star,

            btn_cancel: DRectButton::new(),
            btn_confirm: DRectButton::new(),
            btn_tags: DRectButton::new(),
            confirmed: None,
            show_tags: false,

            rate: Rate::new(),
            rate_upper: if range { Some(Rate::new()) } else { None },
        }
    }

    pub fn showing(&self) -> bool {
        self.show
    }

    pub fn enter(&mut self, t: f32) {
        self.fader.sub(t);
    }

    fn dialog_rect(&self) -> Rect {
        Ui::dialog_rect().nonuniform_feather(0., if self.rate_upper.is_some() { -0.02 } else { -0.1 })
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
            if !self.dialog_rect().contains(touch.position) && touch.phase == TouchPhase::Started {
                self.dismiss(t);
                return true;
            }
            if self.btn_cancel.touch(touch, t) {
                self.confirmed = Some(false);
                self.dismiss(t);
                return true;
            }
            if self.btn_confirm.touch(touch, t) {
                if self.rate.score != 0 {
                    self.confirmed = Some(true);
                }
                return true;
            }
            if self.btn_tags.touch(touch, t) {
                self.show_tags = true;
                self.dismiss(t);
                return true;
            }
            self.rate.touch(touch);
            if let Some(upper) = &mut self.rate_upper {
                upper.touch(touch);
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
            let wr = self.dialog_rect();
            self.fader.for_sub(|f| {
                f.render(ui, t, |ui| {
                    ui.fill_path(&wr.rounded(0.02), ui.background());
                    let r = ui
                        .text(if self.rate_upper.is_some() { tl!("filter") } else { tl!("rate") })
                        .pos(wr.x + 0.04, wr.y + 0.033)
                        .size(0.9)
                        .draw_using(&BOLD_FONT);
                    let bh = 0.09;
                    ui.scope(|ui| {
                        ui.dy(r.bottom() + 0.04);
                        if self.rate_upper.is_some() {
                            let h = ui.text(tl!("lower-bound")).pos(wr.center().x, 0.).anchor(0.5, 0.).size(0.5).draw().h;
                            ui.dy(h + 0.02);
                        } else {
                            ui.dy(0.03);
                        }
                        let h = self.rate.render(ui, &self.icon_star).h;
                        if let Some(upper) = &mut self.rate_upper {
                            upper.score = upper.score.max(self.rate.score);
                        }
                        ui.dy(h + 0.03);
                        if let Some(upper) = &mut self.rate_upper {
                            let h = ui.text(tl!("upper-bound")).pos(wr.center().x, 0.).anchor(0.5, 0.).size(0.5).draw().h;
                            ui.dy(h + 0.02);
                            upper.render(ui, &self.icon_star);
                            self.rate.score = self.rate.score.min(upper.score);
                        }
                    });
                    let pad = 0.02;
                    if self.rate_upper.is_none() {
                        let bw = (wr.w - pad * 3.) / 2.;
                        let mut r = Rect::new(wr.x + pad, wr.bottom() - 0.02 - bh, bw, bh);
                        self.btn_cancel.render_text(ui, r, t, tl!("cancel"), 0.5, true);
                        r.x += bw + pad;
                        self.btn_confirm.render_text(ui, r, t, tl!("confirm"), 0.5, true);
                    } else {
                        let r = Rect::new(wr.x, wr.bottom() - 0.02 - bh, wr.w, bh).nonuniform_feather(-pad, 0.);
                        self.btn_tags.render_text(ui, r, t, tl!("filter-by-tags"), 0.5, true);
                    }
                });
            });
        }
        // TODO magical. removing this line will make the title disappear.
        ui.text("").draw_using(&BOLD_FONT);
    }
}
