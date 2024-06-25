use crate::anim::Anim;
use macroquad::prelude::*;
use prpr::ui::{RectButton, Ui};

pub struct ThreeD {
    center: Anim<Vec2>,
    inner: RectButton,
    pub anchor: Vec2,
    pub angle: f32,
}

impl ThreeD {
    const DURATION: f32 = 0.2;

    pub fn new() -> Self {
        Self {
            center: Anim::new(Vec2::default()),
            inner: RectButton::new(),
            anchor: Vec2::default(),
            angle: 0.08,
        }
    }

    pub fn sync(&mut self) {
        self.center.start(self.anchor, self.anchor, 0., Self::DURATION);
    }

    pub fn touch(&mut self, touch: &Touch, t: f32) {
        self.inner.touch(touch);
        if self.inner.touching() {
            self.center.goto(touch.position, t, Self::DURATION);
        }
    }

    pub fn build(point: Vec2, r: Rect, angle: f32) -> Mat4 {
        let ct = r.center();
        let mut delta = point - ct;
        let length = delta.length();
        let eps = 1e-4;
        if length > eps {
            delta /= length;
            Mat4::from_translation(vec3(ct.x, ct.y, 0.))
                * Mat4::perspective_infinite_rh(std::f32::consts::FRAC_PI_2, 1., 1.)
                * Mat4::from_rotation_translation(
                    Quat::from_axis_angle(vec3(-delta.y, delta.x, 0.), ((1. - (1. - length).powi(3)) / 0.6).min(1.) * angle),
                    vec3(0., 0., -1.),
                )
                * Mat4::from_translation(vec3(-ct.x, -ct.y, 0.))
        } else {
            Mat4::IDENTITY
        }
    }

    pub fn now(&mut self, ui: &mut Ui, rect: Rect, t: f32) -> Mat4 {
        self.inner.set(ui, rect);

        if !self.inner.touching() {
            self.center.goto(self.anchor, t, Self::DURATION);
        }

        Self::build(self.center.now(t), rect, self.angle)
    }
}
