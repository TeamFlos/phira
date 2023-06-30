use super::Tweenable;

pub struct Smooth<T: Tweenable> {
    from: T,
    to: T,
    start_time: f32,
    end_time: f32,
    interpolator: fn(f32) -> f32,
}

impl<T: Tweenable + Default> Default for Smooth<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T: Tweenable> Smooth<T> {
    pub fn new(init: T) -> Self {
        Self {
            from: init.clone(),
            to: init,
            start_time: 0.,
            end_time: 1.,
            interpolator: |p| 1. - (1. - p).powi(3),
        }
    }

    #[inline]
    pub fn with_interpolator(mut self, f: fn(f32) -> f32) -> Self {
        self.interpolator = f;
        self
    }

    #[inline]
    pub fn transiting(&self, t: f32) -> bool {
        (self.start_time..self.end_time).contains(&t)
    }

    pub fn to(&self) -> &T {
        &self.to
    }

    pub fn now(&self, t: f32) -> T {
        T::tween(&self.from, &self.to, (self.interpolator)(((t - self.start_time) / (self.end_time - self.start_time)).min(1.)))
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

    pub fn alter_to(&mut self, to: T) {
        self.to = to;
    }
}
