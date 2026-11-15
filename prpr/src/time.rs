//! Time manager for music time and real time synchronization.

use crate::config::Config;

pub struct TimeManager {
    pub adjust_time: bool,
    pub start_time: f64,
    pause_time: Option<f64>,
    pub speed: f64,
    pub force: f64,
    wait: f64,

    get_time_fn: Box<dyn Fn() -> f64>,
}

impl Default for TimeManager {
    fn default() -> Self {
        Self::new(1.0, false)
    }
}

impl TimeManager {
    pub fn from_config(config: &Config) -> Self {
        Self::new(1., config.adjust_time)
    }

    pub fn manual(get_time_fn: Box<dyn Fn() -> f64>) -> Self {
        let start_time = get_time_fn();
        Self {
            adjust_time: false,
            start_time,
            pause_time: None,
            speed: 1.0,
            wait: f64::NEG_INFINITY,
            force: 3e-3,

            get_time_fn,
        }
    }

    pub fn new(speed: f64, adjust_time: bool) -> Self {
        // we use performance.now() on web since audioContext.currentTime is not stable
        // and may cause serious latency problem
        #[cfg(target_arch = "wasm32")]
        let get_time_fn = {
            let perf = web_sys::window().unwrap().performance().unwrap();
            move || perf.now() / 1000.
        };
        #[cfg(not(target_arch = "wasm32"))]
        let get_time_fn = {
            let start = std::time::Instant::now();
            move || start.elapsed().as_secs_f64()
        };
        let t = get_time_fn();
        Self {
            adjust_time,
            start_time: t,
            pause_time: None,
            speed,
            wait: f64::NEG_INFINITY,
            force: 3e-3,

            get_time_fn: Box::new(get_time_fn),
        }
    }

    pub fn real_time(&self) -> f64 {
        (self.get_time_fn)()
    }

    pub fn reset(&mut self) {
        self.start_time = self.real_time();
        self.pause_time = None;
        self.wait = f64::NEG_INFINITY;
    }

    pub fn wait(&mut self) {
        self.wait = self.real_time() + 0.1;
    }

    pub fn dont_wait(&mut self) {
        self.wait = f64::NEG_INFINITY;
    }

    #[must_use]
    pub fn now(&self) -> f64 {
        (self.pause_time.unwrap_or_else(&self.get_time_fn) - self.start_time) * self.speed
    }

    pub fn update(&mut self, music_time: f64) {
        if !self.adjust_time || self.real_time() <= self.wait || self.pause_time.is_some() {
            return;
        }

        /* Clamp the auto-adjust, otherwise a single bad music_time can
         * push the clock backwards.
         *
         * `music_time` comes from the audio backend. When the app goes to
         * the background, the audio session gets interrupted, or the user
         * presses a volume key, the backend may report 0, a negative value,
         * or jump back a few seconds. A large negative `diff` moves
         * `start_time` forward, which makes `now()` decrease — the countdown
         * runs 3, 4, 5... and the chart plays in reverse.
         *
         * Rule: never let time run backwards. A desync is audible and
         * recoverable; reversed time corrupts the whole judgement system. */
        if !music_time.is_finite() {
            tracing::warn!("time: music_time is not finite ({music_time}), skipping adjust");
            return;
        }
        let diff = music_time - self.now();
        if !diff.is_finite() || diff.abs() < 0.08 {
            return;
        }
        if diff.abs() >= 0.5 {
            tracing::warn!(
                "time: audio clock off by {diff}s, skipping adjust to avoid time reversal"
            );
            return;
        }
        self.start_time -= diff * self.force;
    }

    #[must_use]
    pub fn paused(&self) -> bool {
        self.pause_time.is_some()
    }

    pub fn pause(&mut self) {
        self.pause_time = Some(self.real_time());
    }

    pub fn resume(&mut self) {
        /* `pause_time.take().unwrap()` panics when resume() is called without
         * a preceding pause(). That is not a bug in the caller — it is a
         * legitimate state:
         *
         *   · cold start on iOS
         *   · returning from background
         *
         * In both cases the platform jumps straight into the resume path.
         * The panic poisons the main future, after which every frame fails
         * with "async fn resumed after panicking" — the screen flickers and
         * shows black blocks, and it looks like a GPU problem.
         *
         * When there is no pause_time, the time base was never shifted, so
         * there is nothing to restore. Just keep it. */
        if let Some(pause_time) = self.pause_time.take() {
            self.start_time += self.real_time() - pause_time;
        }
        self.wait();
    }

    pub fn seek_to(&mut self, pos: f64) {
        self.start_time = self.pause_time.unwrap_or_else(&self.get_time_fn) - pos / self.speed;
        self.wait();
    }
}
