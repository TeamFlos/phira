prpr::tl_file!("tags");

use crate::{client::Permissions, page::Fader};
use macroquad::prelude::*;
use prpr::{
    core::BOLD_FONT,
    ext::{semi_black, RectExt},
    scene::{request_input, return_input, show_message, take_input},
    ui::{DRectButton, Scroll, Ui},
};
use smallvec::{smallvec, SmallVec};

const DIVISION_TAGS: &[&str] = &["regular", "troll", "plain", "visual"];

pub struct Tags {
    input_id: &'static str,
    tags: Vec<String>,
    btns: Vec<DRectButton>,
    add: DRectButton,
}

impl Tags {
    pub fn new(input_id: &'static str) -> Self {
        Self {
            input_id,
            tags: Vec::new(),
            btns: Vec::new(),
            add: DRectButton::new(),
        }
    }

    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    pub fn add(&mut self, s: String) {
        let s = s.trim().to_owned();
        if DIVISION_TAGS.contains(&s.as_str()) {
            return;
        }
        self.tags.push(s);
        self.btns.push(DRectButton::new());
    }

    pub fn set(&mut self, tags: Vec<String>) -> &'static str {
        let mut div = DIVISION_TAGS[0];
        let tags: Vec<_> = tags
            .into_iter()
            .map(|it| it.trim().to_owned())
            .filter(|it| {
                if let Some(division) = DIVISION_TAGS.iter().find(|div| *div == it) {
                    div = division;
                    false
                } else {
                    true
                }
            })
            .collect();
        self.btns = vec![DRectButton::new(); tags.len()];
        self.tags = tags;
        div
    }

    pub fn touch(&mut self, touch: &Touch, t: f32) -> bool {
        for (index, btn) in self.btns.iter_mut().enumerate() {
            if btn.touch(touch, t) {
                self.tags.remove(index);
                self.btns.remove(index);
                return true;
            }
        }
        if self.add.touch(touch, t) {
            request_input(self.input_id, "");
            return true;
        }
        false
    }

    pub fn render(&mut self, ui: &mut Ui, mw: f32, t: f32) -> f32 {
        let row_height = 0.1;
        let tmw = 0.3;
        let sz = 0.5;
        let margin = 0.03;
        let pad = 0.01;

        let mut h = 0.;
        let mut x = 0.;
        let mut draw = |btn: &mut DRectButton, text: &str| {
            let w = ui.text(text).size(sz).measure().w.clamp(0.08, tmw);
            if x + w + (margin + pad) * 2. > mw {
                x = 0.;
                h += row_height;
            }
            btn.render_text(ui, Rect::new(x, h, w + (margin + pad) * 2., row_height).feather(-pad), t, text, sz, true);
            x += w + (margin + pad) * 2.;
        };
        for (tag, btn) in self.tags.iter().zip(self.btns.iter_mut()) {
            draw(btn, tag);
        }
        draw(&mut self.add, "+");
        h + row_height
    }

    pub fn try_add(&mut self, s: &str) {
        if !s.chars().all(|it| it == '-' || it.is_alphanumeric()) {
            show_message(tl!("invalid-tag")).error();
            return;
        }
        if self.tags.iter().all(|it| it != s) {
            self.add(s.into());
        }
    }
}

pub struct TagsDialog {
    fader: Fader,
    show: bool,

    scroll: Scroll,
    pub tags: Tags,
    pub unwanted: Option<Tags>,

    pub division: &'static str,
    div_btns: Vec<DRectButton>,

    pub btn_me: DRectButton,
    pub show_me: bool,
    pub btn_unreviewed: DRectButton,
    pub show_unreviewed: bool,
    pub btn_stabilize: DRectButton,
    pub show_stabilize: bool,
    pub perms: Permissions,

    btn_cancel: DRectButton,
    btn_confirm: DRectButton,
    btn_rating: DRectButton,
    pub confirmed: Option<bool>,
    pub show_rating: bool,
}

impl TagsDialog {
    pub fn new(search_mode: bool) -> Self {
        Self {
            fader: Fader::new().with_distance(-0.4).with_time(0.5),
            show: false,

            scroll: Scroll::new(),
            tags: Tags::new("add_tag"),
            unwanted: if search_mode { Some(Tags::new("add_tag_unwanted")) } else { None },

            division: DIVISION_TAGS[0],
            div_btns: DIVISION_TAGS.iter().map(|_| DRectButton::new()).collect(),

            btn_me: DRectButton::new(),
            show_me: false,
            btn_unreviewed: DRectButton::new(),
            show_unreviewed: false,
            btn_stabilize: DRectButton::new(),
            show_stabilize: false,
            perms: Permissions::empty(),

            btn_cancel: DRectButton::new(),
            btn_confirm: DRectButton::new(),
            btn_rating: DRectButton::new(),
            confirmed: None,
            show_rating: false,
        }
    }

    pub fn set(&mut self, tags: Vec<String>) {
        self.division = self.tags.set(tags);
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

    fn dialog_rect(&self) -> Rect {
        if self.unwanted.is_some() {
            Ui::dialog_rect().nonuniform_feather(0.04, 0.05)
        } else {
            Ui::dialog_rect()
        }
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
            if self.scroll.touch(touch, t) {
                return true;
            }
            if self.tags.touch(touch, t) {
                self.scroll.y_scroller.halt();
                return true;
            }
            if let Some(unwanted) = &mut self.unwanted {
                if unwanted.touch(touch, t) {
                    self.scroll.y_scroller.halt();
                    return true;
                }
            }
            for (div, btn) in DIVISION_TAGS.iter().zip(&mut self.div_btns) {
                if btn.touch(touch, t) {
                    self.scroll.y_scroller.halt();
                    self.division = div;
                }
            }
            if self.btn_me.touch(touch, t) {
                self.show_me ^= true;
                return true;
            }
            if self.btn_unreviewed.touch(touch, t) {
                self.show_unreviewed ^= true;
                return true;
            }
            if self.btn_stabilize.touch(touch, t) {
                self.show_stabilize ^= true;
                return true;
            }
            if self.btn_cancel.touch(touch, t) {
                self.confirmed = Some(false);
                self.dismiss(t);
                return true;
            }
            if self.btn_confirm.touch(touch, t) {
                self.confirmed = Some(true);
                self.dismiss(t);
                return true;
            }
            if self.btn_rating.touch(touch, t) {
                self.show_rating = true;
                self.dismiss(t);
                return true;
            }
            return true;
        }
        false
    }

    pub fn update(&mut self, t: f32) {
        if let Some(done) = self.fader.done(t) {
            self.show = !done;
        }
        self.scroll.update(t);
        if let Some((id, text)) = take_input() {
            match id.as_str() {
                "add_tag" => {
                    self.tags.try_add(text.trim());
                }
                "add_tag_unwanted" => {
                    self.unwanted.as_mut().unwrap().try_add(text.trim());
                }
                _ => {
                    return_input(id, text);
                }
            }
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
                        .text(if self.unwanted.is_some() { tl!("filter") } else { tl!("edit") })
                        .pos(wr.x + 0.04, wr.y + 0.033)
                        .size(0.9)
                        .draw_using(&BOLD_FONT);
                    let mw = wr.w - 0.08;
                    let bh = 0.09;
                    ui.scope(|ui| {
                        ui.dx(r.x);
                        ui.dy(r.bottom() + 0.02);
                        self.scroll.size((mw, wr.bottom() - r.bottom() - 0.06 - bh));
                        self.scroll.render(ui, |ui| {
                            let pad = 0.015;
                            let bw = mw / DIVISION_TAGS.len() as f32;
                            let mut r = Rect::new(pad / 2., 0., bw, bh).nonuniform_feather(-0.01, -0.004);
                            for (div, btn) in DIVISION_TAGS.iter().zip(&mut self.div_btns) {
                                btn.render_text(ui, r, t, tl!(*div), 0.5, self.division == *div);
                                r.x += bw;
                            }
                            let mut h = bh + 0.01;
                            ui.dy(h);
                            if self.unwanted.is_some() {
                                let mut row: SmallVec<[_; 3]> = smallvec![(&mut self.btn_me, "filter-me", self.show_me)];
                                if self.perms.contains(Permissions::SEE_UNREVIEWED) {
                                    row.push((&mut self.btn_unreviewed, "filter-unreviewed", self.show_unreviewed));
                                }
                                if self.perms.contains(Permissions::SEE_STABLE_REQ) {
                                    row.push((&mut self.btn_stabilize, "filter-stabilize", self.show_stabilize));
                                }
                                let bw = mw / row.len() as f32;
                                let mut r = Rect::new(pad / 2., 0., bw, bh).nonuniform_feather(-0.01, -0.004);
                                for (btn, text, on) in row.into_iter() {
                                    btn.render_text(ui, r, t, tl!(text), 0.5, on);
                                    r.x += bw;
                                }
                                let dh = bh + 0.01;
                                h += dh;
                                ui.dy(dh);
                            }
                            if self.unwanted.is_some() {
                                let th = ui.text(tl!("wanted")).size(0.5).draw().h + 0.01;
                                ui.dy(th);
                                h += th;
                            }
                            let th = self.tags.render(ui, mw, t);
                            ui.dy(th);
                            h += th;
                            if let Some(unwanted) = &mut self.unwanted {
                                ui.dy(0.02);
                                h += 0.02;
                                let th = ui.text(tl!("unwanted")).size(0.5).draw().h + 0.01;
                                ui.dy(th);
                                h += th;
                                h += unwanted.render(ui, mw, t);
                            }
                            (mw, h)
                        });
                    });
                    let pad = 0.02;
                    if self.unwanted.is_none() {
                        let bw = (wr.w - pad * 3.) / 2.;
                        let mut r = Rect::new(wr.x + pad, wr.bottom() - 0.02 - bh, bw, bh);
                        self.btn_cancel.render_text(ui, r, t, tl!("cancel"), 0.5, true);
                        r.x += bw + pad;
                        self.btn_confirm.render_text(ui, r, t, tl!("confirm"), 0.5, true);
                    } else {
                        let r = Rect::new(wr.x, wr.bottom() - 0.02 - bh, wr.w, bh).nonuniform_feather(-pad, 0.);
                        self.btn_rating.render_text(ui, r, t, tl!("filter-by-rating"), 0.5, true);
                    }
                });
            });
        }
    }
}
