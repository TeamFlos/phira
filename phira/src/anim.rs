use prpr::core::{easing_from, StaticTween, TweenFunction, Tweenable};
use std::rc::Rc;

pub struct Anim<T: Tweenable> {
    pub from: T,
    pub to: T,
    pub start_time: f32,
    pub end_time: f32,
    pub interpolator: Rc<dyn TweenFunction>,
}

impl<T: Tweenable + Default> Default for Anim<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T: Tweenable> Anim<T> {
    pub fn new(init: T) -> Self {
        Self {
            from: init.clone(),
            to: init,
            start_time: f32::NEG_INFINITY,
            end_time: 1e-3,
            interpolator: StaticTween::get_rc(easing_from(prpr::core::TweenMajor::Cubic, prpr::core::TweenMinor::Out)),
        }
    }

    #[inline]
    pub fn transiting(&self, t: f32) -> bool {
        (self.start_time..self.end_time).contains(&t)
    }

    pub fn now(&self, t: f32) -> T {
        T::tween(&self.from, &self.to, self.progress(t))
    }

    pub fn progress(&self, t: f32) -> f32 {
        self.interpolator.y(((t - self.start_time) / (self.end_time - self.start_time)).min(1.))
    }

    pub fn start(&mut self, from: T, to: T, t: f32, duration: f32) {
        self.from = from;
        self.to = to;
        self.start_time = t;
        self.end_time = t + duration;
    }

    #[inline]
    pub fn goto(&mut self, to: T, t: f32, duration: f32) {
        self.start(self.now(t), to, t, duration)
    }

    pub fn begin(&mut self, t: f32, duration: f32) {
        self.start(self.now(t), self.to.clone(), t, duration)
    }

    #[inline]
    pub fn alter_to(&mut self, to: T) {
        self.to = to;
    }

    pub fn set(&mut self, value: T) {
        self.from = value.clone();
        self.to = value;
    }
}
