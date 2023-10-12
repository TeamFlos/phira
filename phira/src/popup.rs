use crate::page::Fader;
use macroquad::prelude::*;
use prpr::{
    ext::{semi_black, semi_white, RectExt},
    ui::{button_hit, rounded_rect_shadow, DRectButton, RectButton, Scroll, ShadowConfig, Ui},
};

pub struct Popup {
    scroll: Scroll,
    rect: Rect,
    showing: bool,
    options: Vec<(String, RectButton)>,
    selected: usize,
    left: f32,
    size: f32,
    height: f32,
    fader: Fader,
    changed: bool,
}

impl Popup {
    pub fn new() -> Self {
        Self {
            scroll: Scroll::new(),
            rect: Rect::default(),
            showing: false,
            options: Vec::new(),
            selected: usize::MAX,
            left: 0.024,
            size: 0.6,
            height: 0.1,
            fader: Fader::new().with_time(0.4).with_distance(0.04),
            changed: false,
        }
    }

    #[inline]
    pub fn with_options(mut self, options: Vec<String>) -> Self {
        self.set_options(options);
        self
    }

    #[inline]
    pub fn selected(&self) -> usize {
        self.selected
    }

    #[inline]
    pub fn set_options(&mut self, options: Vec<String>) {
        self.options = options.into_iter().map(|it| (it, RectButton::new())).collect();
    }

    #[inline]
    pub fn set_selected(&mut self, selected: usize) {
        self.selected = selected;
    }

    pub fn set_bottom(&mut self, bottom: bool) {
        self.fader.distance = self.fader.distance.abs() * if bottom { 1. } else { -1. };
    }

    pub fn show(&mut self, ui: &mut Ui, t: f32, r: Rect) {
        self.rect = ui.rect_to_global(r);
        self.showing = true;
        self.fader.sub(t);
    }

    pub fn dismiss(&mut self, t: f32) {
        self.showing = false;
        self.fader.back(t);
    }

    pub fn render(&mut self, ui: &mut Ui, t: f32, alpha: f32) {
        if !self.fader.transiting() && !self.showing {
            return;
        }
        let r = self.rect;
        self.scroll.size((r.w, r.h));
        self.fader.reset();
        ui.abs_scope(|ui| {
            ui.dx(r.x);
            ui.dy(r.y);
            self.fader.for_sub(|f| {
                f.render(ui, t, |ui| {
                    let r = Rect::new(0., 0., r.w, r.h);
                    let mut cfg = ShadowConfig {
                        radius: 0.01,
                        elevation: 0.01,
                        ..Default::default()
                    };
                    cfg.base *= alpha;
                    rounded_rect_shadow(ui, r, &cfg);
                    ui.fill_path(&r.rounded(0.01), Color { a: alpha, ..ui.background() });
                    self.scroll.render(ui, |ui| {
                        for (id, (opt, btn)) in self.options.iter_mut().enumerate() {
                            if id != 0 {
                                ui.fill_rect(Rect::new(0.02, -0.001, r.w - 0.04, 0.002), semi_white(0.7 * alpha));
                            }
                            let r = Rect::new(0., 0., r.w, self.height);
                            btn.set(ui, r);
                            let chosen = id == self.selected;
                            if chosen {
                                ui.fill_rect(r.feather(-0.007), semi_black(0.4 * alpha));
                            }
                            ui.text(opt.as_str())
                                .pos(self.left, self.height / 2.)
                                .anchor(0., 0.5)
                                .no_baseline()
                                .size(self.size)
                                .max_width(r.w - self.left * 2.)
                                .color(semi_white(alpha))
                                .draw();
                            ui.dy(self.height);
                        }
                        (r.w, self.options.len() as f32 * self.height)
                    });
                });
            });
        });
    }

    pub fn update(&mut self, t: f32) {
        self.scroll.update(t);
        self.fader.done(t);
    }

    pub fn touch(&mut self, touch: &Touch, t: f32) -> bool {
        if self.showing {
            if touch.phase != TouchPhase::Started || self.rect.contains(touch.position) {
                if self.scroll.touch(touch, t) {
                    true
                } else {
                    for (id, (_, btn)) in self.options.iter_mut().enumerate() {
                        if btn.touch(touch) {
                            button_hit();
                            if self.selected != id {
                                self.selected = id;
                                self.changed = true;
                            }
                            self.dismiss(t);
                            return true;
                        }
                    }
                    false
                }
            } else if touch.phase == TouchPhase::Started {
                self.dismiss(t);
                true
            } else {
                false
            }
        } else {
            false
        }
    }

    #[inline]
    pub fn showing(&self) -> bool {
        self.showing
    }

    #[inline]
    pub fn changed(&mut self) -> bool {
        if self.changed {
            self.changed = false;
            true
        } else {
            false
        }
    }
}

pub struct ChooseButton {
    btn: DRectButton,
    popup: Popup,
    width: Option<f32>,
    height: f32,
    need_to_show: bool,
}

impl ChooseButton {
    pub fn new() -> Self {
        Self {
            btn: DRectButton::new(),
            popup: Popup::new(),
            width: None,
            height: 0.34,
            need_to_show: false,
        }
    }

    #[inline]
    pub fn with_options(mut self, options: Vec<String>) -> Self {
        self.popup = self.popup.with_options(options);
        self
    }

    #[inline]
    pub fn with_selected(mut self, selected: usize) -> Self {
        self.popup.selected = selected;
        self
    }

    #[inline]
    pub fn selected(&self) -> usize {
        self.popup.selected
    }

    #[inline]
    pub fn changed(&mut self) -> bool {
        self.popup.changed()
    }

    pub fn render(&mut self, ui: &mut Ui, r: Rect, t: f32) {
        self.btn
            .render_text(ui, r, t, &self.popup.options[self.popup.selected].0, self.popup.size, false);
        if self.need_to_show {
            let pad = 0.007;
            let mut rr = Rect::new(r.x, r.bottom() + pad, self.width.unwrap_or(r.w), self.height);
            let delta = 0.1;
            rr.x -= delta;
            rr.w += delta;
            self.popup.set_bottom(true);
            self.popup.show(ui, t, rr);
            self.need_to_show = false;
        }
    }

    #[inline]
    pub fn render_top(&mut self, ui: &mut Ui, t: f32, alpha: f32) {
        self.popup.render(ui, t, alpha);
    }

    pub fn update(&mut self, t: f32) {
        self.popup.update(t);
    }

    pub fn top_touch(&mut self, touch: &Touch, t: f32) -> bool {
        if self.popup.showing() {
            self.popup.touch(touch, t);
            true
        } else {
            false
        }
    }

    pub fn touch(&mut self, touch: &Touch, t: f32) -> bool {
        if self.btn.touch(touch, t) {
            self.need_to_show = true;
            true
        } else {
            false
        }
    }
}
