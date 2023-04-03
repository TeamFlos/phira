use super::Ui;
use crate::{
    core::{Matrix, Point},
    judge::VelocityTracker,
};
use macroquad::prelude::{Rect, Touch, TouchPhase, Vec2};
use nalgebra::Translation2;

const THRESHOLD: f32 = 0.03;
const EXTEND: f32 = 0.33;

pub struct Scroller {
    touch: Option<(u64, f32, f32, bool)>,
    pub offset: f32,
    bound: f32,
    size: f32,
    speed: f32,
    last_time: f32,
    tracker: VelocityTracker,
    pub pulled: bool,
    frame_touched: bool,
    pub step: f32,
    pub last_step: usize,
}

impl Default for Scroller {
    fn default() -> Self {
        Self::new()
    }
}

impl Scroller {
    pub fn new() -> Self {
        Self {
            touch: None,
            offset: 0.,
            bound: 0.,
            size: 0.,
            speed: 0.,
            last_time: 0.,
            tracker: VelocityTracker::empty(),
            pulled: false,
            frame_touched: true,
            step: f32::NAN,
            last_step: 0,
        }
    }

    pub fn halt(&mut self) {
        self.touch = None;
    }

    pub fn touch(&mut self, id: u64, phase: TouchPhase, val: f32, t: f32) -> bool {
        match phase {
            TouchPhase::Started => {
                if 0. <= val && val < self.bound {
                    self.tracker.reset();
                    self.tracker.push(t, Point::new(val, 0.));
                    self.speed = 0.;
                    self.touch = Some((id, val, self.offset, false));
                    self.frame_touched = true;
                }
            }
            TouchPhase::Stationary | TouchPhase::Moved => {
                if let Some((sid, st, st_off, unlock)) = &mut self.touch {
                    if *sid == id {
                        self.tracker.push(t, Point::new(val, 0.));
                        if (*st - val).abs() > THRESHOLD {
                            *unlock = true;
                        }
                        if *unlock {
                            self.offset = (*st_off + (*st - val)).clamp(-EXTEND, self.size + EXTEND);
                        }
                        self.frame_touched = true;
                    }
                }
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                if matches!(self.touch, Some((sid, ..)) if sid == id) {
                    self.tracker.push(t, Point::new(val, 0.));
                    let speed = self.tracker.speed().x;
                    if speed.abs() > 0.2 {
                        self.speed = -speed * 0.4;
                        self.last_time = t;
                    }
                    if self.offset <= -EXTEND * 0.7 {
                        self.pulled = true;
                    }
                    let res = self.touch.map(|it| it.3).unwrap_or_default();
                    self.touch = None;
                    self.frame_touched = true;
                    return res;
                }
            }
        }
        self.touch.map(|it| it.3).unwrap_or_default()
    }

    pub fn update(&mut self, t: f32) {
        if !self.frame_touched {
            if let Some((id, ..)) = self.touch {
                // self.touch(id, TouchPhase::Cancelled, 0., 0.);
            }
        }
        let dt = t - self.last_time;
        self.offset += self.speed * dt;
        const K: f32 = 3.;
        let unlock = self.touch.map_or(false, |it| it.3);
        if unlock {
            self.speed *= (0.5_f32).powf((t - self.last_time) / 0.4);
        } else {
            let mut to = None;
            if self.offset < 0. {
                to = Some(0.);
            }
            if self.offset > self.size {
                to = Some(self.size);
            }
            if !self.step.is_nan() {
                let lower = (self.offset / self.step).floor() * self.step;
                let upper = lower + self.step;
                let range = 0.0..self.size;
                if range.contains(&lower) && to.map_or(true, |it| (it - self.offset).abs() >= (lower - self.offset).abs()) {
                    to = Some(lower);
                }
                if range.contains(&upper) && to.map_or(true, |it| (it - self.offset).abs() >= (upper - self.offset).abs()) {
                    to = Some(upper);
                }
            }
            if let Some(to) = to {
                self.speed = (to - self.offset) * K;
            }
        }
        // if !unlock && self.offset < 0. {
        // self.speed = -self.offset * K;
        // } else if !unlock && self.offset > self.size {
        // self.speed = (self.size - self.offset) * K;
        // } else {
        // self.speed *= (0.5_f32).powf((t - self.last_time) / 0.4);
        // }
        self.last_time = t;
        if self.pulled {
            self.pulled = false;
        }
        self.frame_touched = false;
    }

    pub fn bound(&mut self, bound: f32) {
        self.bound = bound;
    }

    pub fn size(&mut self, size: f32) {
        self.size = size;
    }
}

pub struct Scroll {
    pub x_scroller: Scroller,
    pub y_scroller: Scroller,
    size: (f32, f32),
    matrix: Option<Matrix>,
}

impl Default for Scroll {
    fn default() -> Self {
        Self::new()
    }
}

impl Scroll {
    pub fn new() -> Self {
        Self {
            x_scroller: Scroller::new(),
            y_scroller: Scroller::new(),
            size: (2., 2.),
            matrix: None,
        }
    }

    pub fn set_offset(&mut self, x: f32, y: f32) {
        self.x_scroller.offset = x;
        self.y_scroller.offset = y;
    }

    pub fn touch(&mut self, touch: &Touch, t: f32) -> bool {
        let Some(matrix) = self.matrix else { return false; };
        let pt = touch.position;
        let pt = matrix.transform_point(&Point::new(pt.x, pt.y));
        if touch.phase == TouchPhase::Started && (pt.x < 0. || pt.y < 0. || pt.x > self.size.0 || pt.y > self.size.1) {
            return false;
        }
        // self.x_scroller.touch(touch.id, touch.phase, pt.x, t) |
        self.y_scroller.touch(touch.id, touch.phase, pt.y, t)
    }

    pub fn update(&mut self, t: f32) {
        self.x_scroller.update(t);
        self.y_scroller.update(t);
    }

    pub fn contains(&self, touch: &Touch) -> bool {
        self.matrix.map_or(false, |mat| {
            let Vec2 { x, y } = touch.position;
            let p = mat.transform_point(&Point::new(x, y));
            !(p.x < 0. || p.x >= self.size.0 || p.y < 0. || p.y >= self.size.1)
        })
    }

    pub fn render(&mut self, ui: &mut Ui, content: impl FnOnce(&mut Ui) -> (f32, f32)) {
        self.matrix = Some(ui.get_matrix().try_inverse().unwrap());
        ui.scissor(Some(Rect::new(0., 0., self.size.0, self.size.1)));
        let s = ui.with(Translation2::new(-self.x_scroller.offset, -self.y_scroller.offset).to_homogeneous(), content);
        ui.scissor(None);
        self.x_scroller.size((s.0 - self.size.0).max(0.));
        self.y_scroller.size((s.1 - self.size.1).max(0.));
    }

    pub fn size(&mut self, size: (f32, f32)) {
        self.size = size;
        self.x_scroller.bound(size.0);
        self.y_scroller.bound(size.1);
    }

    pub fn rect(&self) -> Rect {
        Rect::new(0., 0., self.size.0, self.size.1)
    }
}
