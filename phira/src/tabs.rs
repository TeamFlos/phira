use crate::{anim::Anim, Result};
use macroquad::prelude::*;
use prpr::{
    ext::{semi_black, RectExt},
    ui::{button_hit, rounded_rect_shadow, RectButton, ShadowConfig, Ui},
};
use std::borrow::Cow;

pub type TitleFn = fn() -> Cow<'static, str>;

struct TabItem<T> {
    value: T,
    title: TitleFn,
    btn: RectButton,
}

pub struct Tabs<T> {
    items: Vec<TabItem<T>>,
    selected: usize,

    y_upper: Anim<f32>,
    y_lower: Anim<f32>,

    content_progress: Anim<f32>,
    prev_go_up: bool,
    prev: usize,

    changed: bool,
}

impl<T> Tabs<T> {
    const LEFT: f32 = -0.94;
    const WIDTH: f32 = 0.2;
    const DURATIONS: (f32, f32) = (0.24, 0.35);
    const CONTENT_DY: f32 = 0.06;
    const CONTENT_DURATION: f32 = 0.4;

    pub fn new(items: impl IntoIterator<Item = (T, TitleFn)>) -> Self {
        Tabs {
            items: items
                .into_iter()
                .map(|(value, title)| TabItem {
                    value,
                    title,
                    btn: RectButton::new(),
                })
                .collect(),
            selected: 0,

            y_upper: Anim::new(0.),
            y_lower: Anim::new(0.),

            content_progress: Anim::new(1.),
            prev_go_up: false,
            prev: 0,

            changed: false,
        }
    }

    pub fn selected(&self) -> &T {
        &self.items[self.selected].value
    }

    pub fn selected_mut(&mut self) -> &mut T {
        &mut self.items[self.selected].value
    }

    pub fn changed(&mut self) -> bool {
        let changed = self.changed;
        self.changed = false;
        changed
    }

    pub fn goto(&mut self, t: f32, index: usize) {
        if index == self.selected {
            return;
        }

        let (mut upper, mut lower) = Self::DURATIONS;
        if index > self.selected {
            std::mem::swap(&mut upper, &mut lower);
            self.prev_go_up = true;
        } else {
            self.prev_go_up = false;
        }

        self.prev = self.selected;
        self.selected = index;
        self.y_upper.begin(t, upper);
        self.y_lower.begin(t, lower);
        self.content_progress.start(0., 1., t, Self::CONTENT_DURATION);

        self.changed = true;
    }

    pub fn touch(&mut self, touch: &Touch, t: f32) -> bool {
        for (index, item) in self.items.iter_mut().enumerate() {
            if item.btn.touch(touch) {
                button_hit();
                self.goto(t, index);
                return true;
            }
        }

        false
    }

    fn render_plain(&mut self, ui: &mut Ui, c: Color, first: bool) {
        let mut r = Rect::new(Self::LEFT, -ui.top + 0.16, Self::WIDTH, 0.125);
        for (index, item) in self.items.iter_mut().enumerate() {
            if index == self.selected {
                self.y_upper.alter_to(r.y);
                self.y_lower.alter_to(r.bottom());
            }
            item.btn.set(ui, r);
            if first {
                ui.fill_rect(r, semi_black(0.4 * c.a));
            }
            ui.text((item.title)())
                .pos(r.center().x, r.center().y)
                .anchor(0.5, 0.5)
                .no_baseline()
                .size(0.5)
                .color(c)
                .draw();
            r.y += 0.125;
        }
    }

    pub fn render(&mut self, ui: &mut Ui, t: f32, cr: Rect, mut f: impl FnMut(&mut Ui, &mut T) -> Result<()>) -> Result<()> {
        self.render_plain(ui, WHITE, true);

        let y_upper = self.y_upper.now(t);
        let y_lower = self.y_lower.now(t);
        let r = Rect::new(Self::LEFT, y_upper, Self::WIDTH, y_lower - y_upper).nonuniform_feather(0.007, -0.012);
        rounded_rect_shadow(
            ui,
            r,
            &ShadowConfig {
                radius: 0.,
                ..Default::default()
            },
        );
        ui.fill_rect(r, WHITE);
        ui.scissor(r, |ui| self.render_plain(ui, BLACK, false));

        ui.fill_path(&cr.rounded(0.005), semi_black(0.4));
        ui.scissor::<Result<()>>(cr, |ui| {
            let p = self.content_progress.now(t);
            if p < 1. {
                ui.scope(|ui| {
                    let dy = Self::CONTENT_DY * p;
                    ui.dy(if self.prev_go_up { -dy } else { dy });
                    ui.alpha(1. - p, |ui| f(ui, &mut self.items[self.prev].value))
                })?;
            }

            ui.scope(|ui| {
                let dy = Self::CONTENT_DY * (1. - p);
                ui.dy(if self.prev_go_up { dy } else { -dy });
                ui.alpha(p, |ui| f(ui, &mut self.items[self.selected].value))
            })?;

            Ok(())
        })?;

        Ok(())
    }
}
