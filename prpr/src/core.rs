pub use macroquad::color::Color;

pub const NOTE_WIDTH_RATIO_BASE: f32 = 0.13175016;
pub const HEIGHT_RATIO: f32 = 0.83175;

pub const EPS: f32 = 1e-5;

pub type Point = nalgebra::Point2<f32>;
pub type Vector = nalgebra::Vector2<f32>;
pub type Matrix = nalgebra::Matrix3<f32>;

mod anim;
pub use anim::{Anim, AnimFloat, AnimVector, Keyframe};

mod chart;
pub use chart::{Chart, ChartExtra, ChartSettings};

mod effect;
pub use effect::{Effect, Uniform};

mod line;
pub use line::{JudgeLine, JudgeLineCache, JudgeLineKind, UIElement};

mod note;
use macroquad::prelude::set_pc_assets_folder;
pub use note::{BadNote, Note, NoteKind, RenderConfig};

mod object;
pub use object::{CtrlObject, Object};

mod render;
pub use render::{copy_fbo, internal_id, MSRenderTarget};

mod resource;
pub use resource::{NoteStyle, ParticleEmitter, ResPackInfo, Resource, ResourcePack, DPI_VALUE};

mod smooth;
pub use smooth::Smooth;

mod tween;
pub use tween::{easing_from, BezierTween, ClampedTween, StaticTween, TweenFunction, TweenId, TweenMajor, TweenMinor, Tweenable, TWEEN_FUNCTIONS};

mod video;
pub use video::Video;

pub fn init_assets() {
    if let Ok(mut exe) = std::env::current_exe() {
        while exe.pop() {
            if exe.join("assets").exists() {
                std::env::set_current_dir(exe).unwrap();
                break;
            }
        }
    }
    set_pc_assets_folder("assets");
}

#[derive(serde::Deserialize)]
pub struct Triple(i32, u32, u32);
impl Default for Triple {
    fn default() -> Self {
        Self(0, 0, 1)
    }
}

impl Triple {
    pub fn beats(&self) -> f32 {
        self.0 as f32 + self.1 as f32 / self.2 as f32
    }
}

#[derive(Default)] // the default is a dummy
pub struct BpmList {
    elements: Vec<(f32, f32, f32)>, // (beats, time, bpm)
    cursor: usize,
}

impl BpmList {
    pub fn new(ranges: Vec<(f32, f32)> /*(beat, bpm)*/) -> Self {
        let mut elements = Vec::new();
        let mut time = 0.0;
        let mut last_beats = 0.0;
        let mut last_bpm: Option<f32> = None;
        for (now_beats, bpm) in ranges {
            if let Some(bpm) = last_bpm {
                time += (now_beats - last_beats) * (60. / bpm);
            }
            last_beats = now_beats;
            last_bpm = Some(bpm);
            elements.push((now_beats, time, bpm));
        }
        BpmList { elements, cursor: 0 }
    }

    pub fn time_beats(&mut self, beats: f32) -> f32 {
        while let Some(kf) = self.elements.get(self.cursor + 1) {
            if kf.0 > beats {
                break;
            }
            self.cursor += 1;
        }
        while self.cursor != 0 && self.elements[self.cursor].0 > beats {
            self.cursor -= 1;
        }
        let (start_beats, time, bpm) = &self.elements[self.cursor];
        time + (beats - start_beats) * (60. / bpm)
    }

    pub fn time(&mut self, triple: &Triple) -> f32 {
        self.time_beats(triple.beats())
    }

    pub fn beat(&mut self, time: f32) -> f32 {
        while let Some(kf) = self.elements.get(self.cursor + 1) {
            if kf.1 > time {
                break;
            }
            self.cursor += 1;
        }
        while self.cursor != 0 && self.elements[self.cursor].1 > time {
            self.cursor -= 1;
        }
        let (beats, start_time, bpm) = &self.elements[self.cursor];
        beats + (time - start_time) / (60. / bpm)
    }
}
