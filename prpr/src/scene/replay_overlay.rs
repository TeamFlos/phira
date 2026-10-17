use crate::judge::{InputEvent, MovePoint, JUDGE_TICK_HZ};
use std::collections::BTreeMap;

pub const TRAIL_SECONDS: f64 = 0.5;
pub const RELEASE_FADE_SECONDS: f64 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReplaySample {
    pub time: f64,
    pub x: f32,
    pub y: f32,
}
#[derive(Clone, Debug, PartialEq)]
pub struct ReplayTrace {
    pub id: u8,
    pub color: [f32; 3],
    pub current: (f32, f32),
    pub active: bool,
    pub ended_at: Option<f64>,
    pub samples: Vec<ReplaySample>,
    pub last_was_swipe: bool,
    awaiting_resample: bool,
}

impl ReplayTrace {
    fn new(id: u8, x: f32, y: f32, time: f64) -> Self {
        Self {
            id,
            color: slot_color(id),
            current: (x, y),
            active: true,
            ended_at: None,
            samples: vec![ReplaySample { time, x, y }],
            last_was_swipe: false,
            awaiting_resample: false,
        }
    }

    fn set_position(&mut self, time: f64, x: f32, y: f32) {
        self.current = (x, y);
        self.last_was_swipe = false;
        self.awaiting_resample = false;
        if self.samples.last().is_none_or(|last| last.time < time) {
            self.samples.push(ReplaySample { time, x, y });
        } else if let Some(last) = self.samples.last_mut() {
            *last = ReplaySample { time, x, y };
        }
    }

    fn push_position(&mut self, time: f64, x: f32, y: f32) {
        self.current = (x, y);
        self.last_was_swipe = true;
        self.awaiting_resample = false;
        let sample = ReplaySample { time, x, y };
        if self.samples.last().is_none_or(|last| *last != sample) {
            self.samples.push(sample);
        }
    }
}

#[derive(Default)]
pub struct ReplayOverlay {
    traces: BTreeMap<u8, ReplayTrace>,
    cursor: usize,
    last_time: f64,
}

impl ReplayOverlay {
    pub fn reset(&mut self) {
        self.traces.clear();
        self.cursor = 0;
        self.last_time = f64::NEG_INFINITY;
    }

    pub fn update(&mut self, inputs: &[InputEvent], cursor: usize, now: f64) {
        if cursor < self.cursor || now + 1e-6 < self.last_time {
            self.reset();
        }

        let end = cursor.min(inputs.len());
        for event in &inputs[self.cursor..end] {
            self.apply(event);
        }
        self.cursor = end;
        self.last_time = now;
        self.trim(now);
    }

    /// Re-anchor surviving slots after a seek. A slot whose position did not
    /// move enough to produce a recorded `Move` still remains alive in the
    /// judge, so the display must receive that state from the judge as well.
    pub fn resample_active(&mut self, current: &[(u8, f32, f32)], now: f64) {
        self.traces.retain(|id, trace| {
            if !trace.awaiting_resample {
                return true;
            }
            let Some((_, x, y)) = current.iter().find(|(current_id, _, _)| current_id == id) else {
                return false;
            };
            trace.set_position(now, *x, *y);
            true
        });
    }

    pub fn active_count(&self) -> usize {
        self.traces.values().filter(|trace| trace.active).count()
    }

    pub fn traces(&self) -> impl Iterator<Item = &ReplayTrace> {
        self.traces.values()
    }

    pub fn fade_alpha(trace: &ReplayTrace, now: f64) -> f32 {
        trace
            .ended_at
            .map(|ended| (1. - (now - ended) / RELEASE_FADE_SECONDS).clamp(0., 1.) as f32)
            .unwrap_or(1.)
    }

    pub fn sample_alpha(sample: ReplaySample, now: f64) -> f32 {
        (1. - (now - sample.time) / TRAIL_SECONDS).clamp(0., 1.) as f32
    }

    fn apply(&mut self, event: &InputEvent) {
        match event {
            InputEvent::Start { id, time, x, y } | InputEvent::Appear { id, time, x, y } => {
                let trace = self.traces.entry(*id).or_insert_with(|| ReplayTrace::new(*id, *x, *y, *time));
                trace.active = true;
                trace.ended_at = None;
                trace.samples.clear();
                trace.set_position(*time, *x, *y);
            }
            InputEvent::Move { tick, points } => {
                let time = *tick as f64 / JUDGE_TICK_HZ;
                for point in points {
                    self.set_position(*point, time);
                }
                self.traces.retain(|_, trace| !trace.awaiting_resample);
            }
            InputEvent::End { id, time } => {
                if let Some(trace) = self.traces.get_mut(id) {
                    trace.active = false;
                    trace.ended_at = Some(*time);
                }
            }
            InputEvent::Seek { .. } => {
                self.traces.retain(|_, trace| {
                    if trace.active {
                        trace.samples.clear();
                        trace.ended_at = None;
                        trace.last_was_swipe = false;
                        trace.awaiting_resample = true;
                        true
                    } else {
                        false
                    }
                });
            }
            InputEvent::Swipe {
                id, time, x0, y0, x1, y1, ..
            } => {
                if let Some(trace) = self.traces.get_mut(id) {
                    if trace.active {
                        trace.push_position(*time, *x0, *y0);
                        trace.push_position(*time, *x1, *y1);
                    }
                }
            }
        }
    }

    fn set_position(&mut self, point: MovePoint, time: f64) {
        if let Some(trace) = self.traces.get_mut(&point.id) {
            if trace.active {
                trace.set_position(time, point.x, point.y);
            }
        }
    }

    fn trim(&mut self, now: f64) {
        let trail_start = now - TRAIL_SECONDS;
        self.traces.retain(|_, trace| {
            trace.samples.retain(|sample| sample.time >= trail_start);
            trace.active || trace.ended_at.is_some_and(|ended| now - ended < RELEASE_FADE_SECONDS)
        });
    }
}

pub fn slot_color(id: u8) -> [f32; 3] {
    let noise = splitmix64(id as u64 + 0x9e37_79b9_7f4a_7c15);
    let hue = ((id as f32 * 0.618_033_95) + unit_noise(noise) * 0.19).fract();
    let saturation = 0.62 + unit_noise(splitmix64(noise)) * 0.22;
    let value = 0.9 + unit_noise(splitmix64(noise ^ 0xa076_1d64_78bd_642f)) * 0.1;
    hsv_to_rgb(hue, saturation, value)
}

fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut result = value;
    result = (result ^ (result >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    result = (result ^ (result >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    result ^ (result >> 31)
}

fn unit_noise(value: u64) -> f32 {
    (value as f64 / u64::MAX as f64) as f32
}

fn hsv_to_rgb(hue: f32, saturation: f32, value: f32) -> [f32; 3] {
    let scaled = hue * 6.;
    let sector = scaled.floor() as u32;
    let fraction = scaled - sector as f32;
    let p = value * (1. - saturation);
    let q = value * (1. - saturation * fraction);
    let t = value * (1. - saturation * (1. - fraction));
    match sector % 6 {
        0 => [value, t, p],
        1 => [q, value, p],
        2 => [p, value, t],
        3 => [p, q, value],
        4 => [t, p, value],
        _ => [value, p, q],
    }
}
