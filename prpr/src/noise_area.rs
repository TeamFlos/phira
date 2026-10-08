//! Phigros `blockAreaList` (噪域) data, hit testing and rendering entry point.
//!
//! The chart field is kept separate from the global switches so charts without
//! `blockAreaList` stay on the normal rendering path.
//!
//! Field semantics and the easing table follow the official chart standard
//! ("blockArea" chapter of phira-docs). `easeType` names do not match the
//! curves they produce: the original generates them from a power exponent
//! `n = (type - 1) / 3 + 2`, so `InSine` is really a quadratic curve and the
//! curve is sampled into a 101-entry table with linear interpolation.
use macroquad::prelude::Vec2;
use serde::{Deserialize, Serialize};

// `Point` here is the percentage type below, so the transform point keeps its
// module path.
use crate::core::{Matrix, Point as CorePoint, Vector};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct NoiseAreaConfig {
    /// Master switch. A chart without `blockAreaList` is never affected.
    pub enabled: bool,
    /// Sample the coverage mask instead of taking a single centre sample.
    pub precise_edges: bool,
    /// Skip the displacement/distortion part of the active block shader.
    pub remove_distortion: bool,
    /// Freeze every time-driven effect, keeping the block edges perfectly still.
    pub no_jitter: bool,
    /// Keep the music untouched while a finger is blocked.
    pub music_unaffected: bool,
    /// Fall back to the plain quad renderer (no render targets, no shaders).
    pub low_performance: bool,
}

impl Default for NoiseAreaConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            precise_edges: false,
            remove_distortion: false,
            no_jitter: false,
            music_unaffected: false,
            low_performance: false,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BlockArea {
    pub top_right_percentage: Point,
    pub bottom_left_percentage: Point,
    pub appear_time: f32,
    pub enable_time: f32,
    pub disable_time: f32,
    pub disappear_time: f32,
    pub is_subtract: bool,
    pub rotate_events: Vec<BlockRotateEvent>,
    pub move_events: Vec<BlockMoveEvent>,
    pub scale_events: Vec<BlockScaleEvent>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockRotateEvent {
    pub anchor: Point,
    pub time: f32,
    pub rotation: f32,
    #[serde(default)]
    pub ease_type: i32,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockMoveEvent {
    pub end_position: Point,
    pub time: f32,
    #[serde(default)]
    pub ease_type_x: i32,
    #[serde(default)]
    pub ease_type_y: i32,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockScaleEvent {
    pub anchor: Point,
    pub time: f32,
    pub scale: Point,
    #[serde(default)]
    pub ease_type_x: i32,
    #[serde(default)]
    pub ease_type_y: i32,
}

mod perf;
pub mod render;

/// Longest easing table index; the original samples `1%` steps.
const EASE_SAMPLES: i32 = 100;
/// `disabledBlockReadyDuration` / `disabledBlockShowDuration`, both fixed at 0.5s.
pub const BLOCK_FADE_DURATION: f32 = 0.5;

/// Curve of a single `easeType`, before the 101-entry table sampling.
fn ease_raw(n: i32, t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    match n {
        // 13 = Zero: the value stays at the previous event until its own time.
        13 => 0.,
        // 14 = One: the value jumps to the target at this event's time.
        14 => 1.,
        // 1..=12: exponent n = (type - 1) / 3 + 2.
        1..=12 => {
            let group = (n - 1) / 3;
            let kind = (n - 1) % 3;
            let exp = group + 2;
            match kind {
                0 => t.powi(exp),
                1 => 1. - (1. - t).powi(exp),
                _ => {
                    if t < 0.5 {
                        (2. * t).powi(exp) / 2.
                    } else {
                        1. - (2. - 2. * t).powi(exp) / 2.
                    }
                }
            }
        }
        _ => t,
    }
}

/// `GetEase.GetEaseWithProgress`: 101-entry table with linear interpolation.
fn ease(n: i32, t: f32) -> f32 {
    let p = t.clamp(0., 1.) * EASE_SAMPLES as f32;
    let i = p.floor();
    let a = ease_raw(n, i / EASE_SAMPLES as f32);
    if i >= EASE_SAMPLES as f32 {
        return a;
    }
    a + (ease_raw(n, (i + 1.) / EASE_SAMPLES as f32) - a) * (p - i)
}

fn ratio(t: f32, a: f32, b: f32) -> f32 {
    if b <= a {
        1.
    } else {
        ((t - a) / (b - a)).clamp(0., 1.)
    }
}

/// Half of the chart's render height in world units; only the ratio
/// world-size/screen-size matters, so the absolute value is free.
const WORLD_HALF_HEIGHT: f32 = 5.;

/// Percentage coordinates -> world coordinates (isotropic, centre at origin).
pub fn world(p: Point, aspect: f32) -> Vec2 {
    Vec2::new((p.x - 0.5) * 2. * WORLD_HALF_HEIGHT * aspect, (p.y - 0.5) * 2. * WORLD_HALF_HEIGHT)
}

/// Screen point (x in [-1,1], y in [-1/aspect, 1/aspect]) -> world coordinates.
pub fn screen_to_world(touch: Vec2, aspect: f32) -> Vec2 {
    Vec2::new(touch.x * WORLD_HALF_HEIGHT * aspect, -touch.y * WORLD_HALF_HEIGHT * aspect)
}

/// World position of a note, for the case where there is no finger to test.
///
/// `tr` is the note's judge line transform — the same matrix `Judge` inverts
/// touches with to compare them against a note — and `translation` is the note's
/// own offset in chart space. Notes ride their line, so the line is part of the
/// answer; the vertical axis is squashed by the aspect ratio on the way in.
pub fn note_world(tr: &Matrix, translation: Vector, aspect: f32) -> Vec2 {
    let p = tr.transform_point(&CorePoint::new(translation.x, translation.y / aspect));
    screen_to_world(Vec2::new(p.x, -p.y), aspect)
}

fn rotate(p: Vec2, angle: f32) -> Vec2 {
    let (s, c) = angle.sin_cos();
    Vec2::new(c * p.x - s * p.y, s * p.x + c * p.y)
}

fn safe_div(a: f32, b: f32) -> f32 {
    if b.abs() < f32::from_bits(8) {
        1.
    } else {
        a / b
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Pose {
    pub center: Vec2,
    pub size: Vec2,
    pub angle: f32,
}

impl Pose {
    /// `adjusted` tightens the rectangle by the original's edge tolerance, and
    /// `blocked` requires a point to satisfy both variants, so the outermost band
    /// never counts as covered.
    pub fn contains(self, p: Vec2, adjusted: bool, subtract: bool) -> bool {
        if self.size.x.abs() < 0.0001 || self.size.y.abs() < 0.0001 {
            return false;
        }
        let local = rotate(p - self.center, -self.angle) / self.size;
        let sign = if subtract { 1. } else { -1. };
        let half = if adjusted {
            Vec2::splat(0.5) + Vec2::new((0.3 / self.size.x.abs()).min(0.25), (0.3 / self.size.y.abs()).min(0.25)) * sign
        } else {
            Vec2::splat(0.5)
        };
        local.x.abs() <= half.x && local.y.abs() <= half.y
    }

    pub fn corners(self) -> [Vec2; 4] {
        [Vec2::new(-0.5, -0.5), Vec2::new(0.5, -0.5), Vec2::new(0.5, 0.5), Vec2::new(-0.5, 0.5)]
            .map(|p| self.center + rotate(p * self.size, self.angle))
    }
}

impl BlockArea {
    /// Clamp the four time points, reject non-finite data, sort the events.
    pub fn normalize(&mut self) -> anyhow::Result<()> {
        anyhow::ensure!(
            [
                self.appear_time,
                self.enable_time,
                self.disable_time,
                self.disappear_time,
                self.top_right_percentage.x,
                self.top_right_percentage.y,
                self.bottom_left_percentage.x,
                self.bottom_left_percentage.y
            ]
            .iter()
            .all(|v| v.is_finite()),
            "block area coordinates and times must be finite"
        );
        self.enable_time = self.enable_time.max(self.appear_time);
        self.disable_time = self.disable_time.max(self.enable_time);
        self.disappear_time = self.disappear_time.max(self.disable_time);
        for e in &self.move_events {
            anyhow::ensure!(
                [e.time, e.end_position.x, e.end_position.y].iter().all(|v| v.is_finite())
                    && (0..=14).contains(&e.ease_type_x)
                    && (0..=14).contains(&e.ease_type_y),
                "block area move event is invalid or uses an unknown ease type"
            );
        }
        for e in &self.scale_events {
            anyhow::ensure!(
                [e.time, e.anchor.x, e.anchor.y, e.scale.x, e.scale.y].iter().all(|v| v.is_finite())
                    && (0..=14).contains(&e.ease_type_x)
                    && (0..=14).contains(&e.ease_type_y),
                "block area scale event is invalid or uses an unknown ease type"
            );
        }
        for e in &self.rotate_events {
            anyhow::ensure!(
                [e.time, e.anchor.x, e.anchor.y, e.rotation].iter().all(|v| v.is_finite()) && (0..=14).contains(&e.ease_type),
                "block area rotate event is invalid or uses an unknown ease type"
            );
        }
        self.move_events.sort_by(|a, b| a.time.total_cmp(&b.time));
        self.scale_events.sort_by(|a, b| a.time.total_cmp(&b.time));
        self.rotate_events.sort_by(|a, b| a.time.total_cmp(&b.time));
        Ok(())
    }

    /// `enableTime <= t < disableTime`.
    pub fn active(&self, t: f32) -> bool {
        t >= self.enable_time && t < self.disable_time
    }

    /// `appearTime <= t < disappearTime`.
    pub fn visible(&self, t: f32) -> bool {
        t >= self.appear_time && t < self.disappear_time
    }

    /// `[enableTime - 0.5, enableTime)`, the `Ready` phase.
    pub fn ready(&self, t: f32) -> bool {
        t >= self.enable_time - BLOCK_FADE_DURATION && t < self.enable_time && self.appear_time < self.enable_time
    }

    /// Fade-in alpha of the `Disabled` phase; `Ready` and `Active` are opaque.
    pub fn fade(&self, t: f32) -> f32 {
        if self.appear_time >= self.enable_time {
            return 1.;
        }
        ((t - self.appear_time) / BLOCK_FADE_DURATION).clamp(0., 1.)
    }

    pub fn pose(&self, t: f32, aspect: f32) -> Pose {
        let lo = world(self.bottom_left_percentage, aspect);
        let hi = world(self.top_right_percentage, aspect);
        let base = (lo + hi) * 0.5;
        let mut center = base;
        let mut scale = Vec2::ONE;
        if let Some(first) = self.scale_events.first() {
            scale = Vec2::new(first.scale.x, first.scale.y);
            for (i, e) in self.scale_events.iter().enumerate() {
                let Some(next) = self.scale_events.get(i + 1) else { break };
                let p = ratio(t, e.time, next.time);
                let completed = t >= next.time;
                let v = Vec2::new(
                    e.scale.x + (next.scale.x - e.scale.x) * if completed { 1. } else { ease(e.ease_type_x, p) },
                    e.scale.y + (next.scale.y - e.scale.y) * if completed { 1. } else { ease(e.ease_type_y, p) },
                );
                let anchor = world(e.anchor, aspect);
                let rel = Vec2::new(safe_div(v.x, e.scale.x), safe_div(v.y, e.scale.y));
                center = anchor + (center - anchor) * rel;
                scale = v;
                if t < next.time {
                    break;
                }
            }
        }
        let mut angle = 0.;
        if let Some(first) = self.rotate_events.first() {
            angle = first.rotation;
            for (i, e) in self.rotate_events.iter().enumerate() {
                let Some(next) = self.rotate_events.get(i + 1) else { break };
                let v = e.rotation
                    + (next.rotation - e.rotation)
                        * if t >= next.time {
                            1.
                        } else {
                            ease(e.ease_type, ratio(t, e.time, next.time))
                        };
                let anchor = world(e.anchor, aspect);
                center = anchor + rotate(center - anchor, (v - e.rotation).to_radians());
                angle = v;
                if t < next.time {
                    break;
                }
            }
        }
        if let Some(first) = self.move_events.first() {
            let mut v = first.end_position;
            for (i, e) in self.move_events.iter().enumerate() {
                let Some(next) = self.move_events.get(i + 1) else { break };
                let p = ratio(t, e.time, next.time);
                let completed = t >= next.time;
                v = Point {
                    x: e.end_position.x + (next.end_position.x - e.end_position.x) * if completed { 1. } else { ease(e.ease_type_x, p) },
                    y: e.end_position.y + (next.end_position.y - e.end_position.y) * if completed { 1. } else { ease(e.ease_type_y, p) },
                };
                if t < next.time {
                    break;
                }
            }
            center += world(v, aspect) - base;
        }
        Pose {
            center,
            size: ((hi - lo) * scale).abs(),
            angle: angle.to_radians(),
        }
    }
}

/// Union of every active normal block, XOR the subtract blocks, and require the
/// point to also fall inside the edge-adjusted union so the border stays sticky.
///
/// `p` is in world coordinates, so both the touch path and the keyboard path (via
/// `note_world`) can ask the same question and get the same answer.
pub fn blocked(areas: &[BlockArea], p: Vec2, t: f32, aspect: f32) -> bool {
    let (mut a, mut b, mut c, mut d) = (false, false, false, false);
    for area in areas.iter().filter(|a| a.active(t)) {
        let pose = area.pose(t, aspect);
        if pose.contains(p, false, area.is_subtract) {
            if area.is_subtract {
                b ^= true
            } else {
                a = true
            }
        }
        if pose.contains(p, true, area.is_subtract) {
            if area.is_subtract {
                d ^= true
            } else {
                c = true
            }
        }
    }
    (a ^ b) && (c ^ d)
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Hover {
    pub finger: Option<u64>,
    pub position: Vec2,
    pub scale: f32,
    start: f32,
    target: f32,
    elapsed: f32,
    moving: bool,
}

impl Hover {
    fn animate(&mut self, target: f32, show: bool) {
        self.start = if show { 0. } else { self.scale };
        self.scale = self.start;
        self.target = target;
        self.elapsed = 0.;
        self.moving = true;
    }

    fn tick(&mut self, dt: f32) {
        if self.moving {
            let p = (self.elapsed / 0.1).clamp(0., 1.);
            self.scale = self.start + (self.target - self.start) * p;
            self.elapsed += dt.max(0.);
            if p >= 1. {
                self.moving = false
            }
        }
    }
}

/// Per-frame state. A finger that ever touched the area stays blocked until it
/// is released, matching the original: moving out of the area without lifting
/// the finger keeps producing nothing.
#[derive(Clone, Debug, Default)]
pub struct NoiseAreaState {
    pub blocked_ids: std::collections::HashSet<u64>,
    pub hovers: [Hover; 10],
    pub positions: Vec<Vec2>,
}

impl NoiseAreaState {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn update(&mut self, areas: &[BlockArea], touches: &[(u64, Vec2)], t: f32, aspect: f32, enabled: bool, dt: f32) {
        if !enabled {
            self.clear();
            return;
        }
        self.blocked_ids.retain(|id| touches.iter().any(|(i, _)| i == id));
        self.positions.clear();
        let mut seen = [false; 10];
        for &(id, p) in touches {
            if self.blocked_ids.contains(&id) || blocked(areas, p, t, aspect) {
                self.blocked_ids.insert(id);
                self.positions.push(p);
                let slot = self
                    .hovers
                    .iter()
                    .position(|h| h.finger == Some(id))
                    .or_else(|| self.hovers.iter().position(|h| h.finger.is_none()));
                if let Some(i) = slot {
                    let h = &mut self.hovers[i];
                    if h.finger != Some(id) {
                        h.finger = Some(id);
                        h.animate(11.5, true)
                    }
                    h.position = p;
                    seen[i] = true;
                }
            }
        }
        for (i, h) in self.hovers.iter_mut().enumerate() {
            if h.finger.is_some() && !seen[i] {
                h.finger = None;
                h.animate(0., false)
            }
            h.tick(dt)
        }
    }

    pub fn is_blocked(&self, id: u64) -> bool {
        self.blocked_ids.contains(&id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{core::ChartExtra, parse::parse_phigros};
    use macroquad::prelude::vec2;

    fn area(sub: bool) -> BlockArea {
        BlockArea {
            bottom_left_percentage: Point { x: 0.2, y: 0.2 },
            top_right_percentage: Point { x: 0.8, y: 0.8 },
            enable_time: 1.,
            disable_time: 3.,
            disappear_time: 4.,
            is_subtract: sub,
            ..Default::default()
        }
    }

    #[test]
    fn union_parity_and_edge_protection() {
        let a = area(false);
        assert!(blocked(&[a.clone(), a.clone()], Vec2::ZERO, 2., 1.));
        assert!(!blocked(&[a.clone(), area(true)], Vec2::ZERO, 2., 1.));
        assert!(blocked(&[a.clone(), area(true), area(true)], Vec2::ZERO, 2., 1.));
        assert!(!blocked(&[a], vec2(2.9, 0.), 2., 1.));
        assert!(blocked(&[area(true)], Vec2::ZERO, 2., 1.));
    }

    #[test]
    fn persists_outside_until_release() {
        let mut s = NoiseAreaState::default();
        s.update(&[area(false)], &[(7, Vec2::ZERO)], 2., 1., true, 0.016);
        assert!(s.is_blocked(7));
        s.update(&[area(false)], &[(7, vec2(20., 20.))], 5., 1., true, 0.016);
        assert!(s.is_blocked(7));
        assert_eq!(s.hovers[0].position, vec2(20., 20.));
        s.update(&[], &[], 5., 1., true, 0.016);
        assert!(!s.is_blocked(7));
        assert!(s.hovers[0].scale > 0.);
        for _ in 0..10 {
            s.update(&[], &[], 5., 1., true, 0.016);
        }
        assert_eq!(s.hovers[0].scale, 0.);
    }

    #[test]
    fn ten_visual_slots_and_reuse() {
        let mut s = NoiseAreaState::default();
        let touches = (0..11).map(|i| (i, Vec2::ZERO)).collect::<Vec<_>>();
        s.update(&[area(false)], &touches, 2., 1., true, 0.05);
        assert_eq!(s.blocked_ids.len(), 11);
        assert_eq!(s.hovers.iter().filter(|h| h.finger.is_some()).count(), 10);
        s.update(&[], &[], 2., 1., true, 0.05);
        s.update(&[area(false)], &[(50, Vec2::ZERO)], 2., 1., true, 0.05);
        assert_eq!(s.hovers[0].finger, Some(50));
        s.update(&[], &[(50, Vec2::ZERO)], 2., 1., false, 0.05);
        assert!(s.blocked_ids.is_empty());
    }

    #[test]
    fn phases_follow_the_four_time_points() {
        let a = BlockArea {
            appear_time: 1.,
            enable_time: 2.,
            disable_time: 3.,
            disappear_time: 4.,
            ..area(false)
        };
        // HiddenBefore
        assert!(!a.visible(0.9));
        assert!(!a.ready(0.9));
        // Disabled, fading in over 0.5s
        assert!(a.visible(1.));
        assert!(!a.active(1.));
        assert_eq!(a.fade(1.), 0.);
        assert!((a.fade(1.25) - 0.5).abs() < 1e-6);
        assert_eq!(a.fade(1.5), 1.);
        // Ready
        assert!(a.ready(1.5));
        assert!(a.ready(1.99));
        assert!(!a.ready(2.));
        // Active
        assert!(a.active(2.));
        assert!(!a.active(3.));
        assert_eq!(a.fade(2.5), 1.);
        // Disabled again, no fade out
        assert!(a.visible(3.9));
        assert_eq!(a.fade(3.9), 1.);
        // HiddenAfter
        assert!(!a.visible(4.));
    }

    #[test]
    fn no_ready_phase_when_the_area_starts_active() {
        let a = BlockArea {
            appear_time: 2.,
            enable_time: 2.,
            disable_time: 3.,
            disappear_time: 4.,
            ..area(false)
        };
        assert!(!a.ready(1.8));
        assert!(a.active(2.));
        assert_eq!(a.fade(2.), 1.);
    }

    #[test]
    fn animation_and_custom_curve_validation() {
        let mut a = area(false);
        a.move_events = vec![
            BlockMoveEvent {
                time: 1.,
                end_position: Point { x: 0.5, y: 0.5 },
                ..Default::default()
            },
            BlockMoveEvent {
                time: 2.,
                end_position: Point { x: 0.8, y: 0.5 },
                ..Default::default()
            },
        ];
        a.normalize().unwrap();
        assert!((a.pose(1.5, 1.).center.x - 1.5).abs() < 0.0001);
        assert_eq!(ease(13, 1.), 0.);
        assert_eq!(ease(14, 0.), 1.);
        a.move_events[0].ease_type_x = 15;
        assert!(a.normalize().is_err());
    }

    #[test]
    fn ease_table_matches_the_original_power_curves() {
        // 1 = InQuad, 4 = InCubic, 7 = InQuart, 10 = InQuint.
        assert!((ease(1, 0.5) - 0.25).abs() < 1e-5);
        assert!((ease(4, 0.5) - 0.125).abs() < 1e-5);
        assert!((ease(7, 0.5) - 0.0625).abs() < 1e-5);
        assert!((ease(10, 0.5) - 0.03125).abs() < 1e-5);
        // 0 = Linear.
        assert!((ease(0, 0.375) - 0.375).abs() < 1e-5);
        // Table endpoints.
        assert!((ease(5, 1.) - 1.).abs() < 1e-5);
        assert!((ease(5, 0.) - 0.).abs() < 1e-5);
    }

    #[test]
    fn missing_config_gets_default_and_switches_independent() {
        let cfg: crate::config::Config = serde_json::from_str("{}").unwrap();
        assert!(cfg.noise_area.enabled);
        assert!(!cfg.noise_area.precise_edges);
        assert!(!cfg.noise_area.low_performance);
        let mut n = cfg.noise_area;
        n.music_unaffected = true;
        n.low_performance = true;
        assert!(!n.remove_distortion);
        assert!(!n.precise_edges);
        let json = serde_json::to_string(&n).unwrap();
        let back: NoiseAreaConfig = serde_json::from_str(&json).unwrap();
        assert!(back.music_unaffected);
        assert!(back.low_performance);
    }

    // ---- 边界：坐标系 ----

    /// `screen_to_world` must invert `world` at every aspect ratio. Its input is the
    /// viewport-normalized space `Judge::touch_transform` produces
    /// (`x in [-1, 1]`, `y in [-1/aspect, 1/aspect]`), never raw pixels.
    #[test]
    fn screen_to_world_inverts_world_for_every_aspect_ratio() {
        for aspect in [0.5f32, 1.0, 16. / 9., 3.0] {
            for &(px, py) in &[(0.25f32, 0.75f32), (0.5, 0.5), (0., 0.), (1., 1.)] {
                let projected = world(Point { x: px, y: py }, aspect);
                let normalized = vec2(projected.x / (WORLD_HALF_HEIGHT * aspect), -projected.y / (WORLD_HALF_HEIGHT * aspect));
                let back = screen_to_world(normalized, aspect);
                assert!((back - projected).length() < 1e-4, "aspect={aspect}, ({px},{py})");
            }
        }
    }

    /// `screen_to_world` only accepts viewport-normalized coordinates (what
    /// `touch_transform` emits). Both wrong inputs look alike from the outside and
    /// each silently breaks hit testing:
    /// - raw pixels (using `touches()` instead of the transformed map) land orders
    ///   of magnitude outside the playfield;
    /// - an already-normalized value passed through `touch_transform` a second time
    ///   collapses onto the top-left corner.
    #[test]
    fn wrong_coordinate_spaces_are_far_from_the_playfield() {
        let aspect = 16. / 9.;
        let playfield = world(Point { x: 1., y: 1. }, aspect).length();
        // Raw pixels.
        assert!(screen_to_world(vec2(1920. / 2., 1080. / 2.), aspect).length() > playfield * 10.);
        // Normalized value re-normalized as if it were a pixel (1920x1080 viewport).
        let once = vec2(0., 0.);
        let twice = vec2((once.x - 0.) / 1920. * 2. - 1., ((once.y - 0.) / 1080. * 2. - 1.) / aspect);
        assert!((twice - vec2(-1., -1. / aspect)).length() < 1e-3);
    }

    #[test]
    fn world_projection_stretches_x_by_the_aspect_ratio_only() {
        for aspect in [0.5f32, 1.0, 2.5] {
            assert!(world(Point { x: 0.5, y: 0.5 }, aspect).abs_diff_eq(Vec2::ZERO, 1e-6));
            let corner = world(Point { x: 1., y: 1. }, aspect);
            assert!((corner.x - WORLD_HALF_HEIGHT * aspect).abs() < 1e-6);
            assert!((corner.y - WORLD_HALF_HEIGHT).abs() < 1e-6);
        }
    }

    /// A note has to be tested through its judge line, because that is what puts it
    /// on screen: chart space is isotropic, its `y` is squashed by the aspect ratio,
    /// and the line's own transform drags the note along with it. A chart-space-only
    /// test would silently disagree with the overlay as soon as the line moves.
    #[test]
    fn note_world_follows_the_line_transform() {
        let aspect = 16. / 9.;
        let identity = Matrix::identity();
        let centre = note_world(&identity, Vector::new(0., 0.), aspect);
        assert!(centre.abs_diff_eq(Vec2::ZERO, 1e-6), "{centre:?}");
        let right = note_world(&identity, Vector::new(1., 0.), aspect);
        assert!((right.x - WORLD_HALF_HEIGHT * aspect).abs() < 1e-6, "{right:?}");
        let top = note_world(&identity, Vector::new(0., 1.), aspect);
        assert!((top.y - WORLD_HALF_HEIGHT).abs() < 1e-6, "{top:?}");
        let moved = Matrix::new_translation(&Vector::new(0.5, 0.));
        let right = note_world(&moved, Vector::new(1., 0.), aspect);
        assert!((right.x - 1.5 * WORLD_HALF_HEIGHT * aspect).abs() < 1e-6, "{right:?}");
    }

    /// The keyboard path must agree with the touch path, or the feature would be
    /// consistent for fingers and wrong for keys. `Judge` compares a touch against a
    /// note by inverting the line transform, so a note placed where a finger is must
    /// land on exactly the same world point the finger maps to.
    #[test]
    fn a_note_under_a_finger_lands_on_the_same_world_point() {
        let aspect = 16. / 9.;
        let tr = Matrix::new_translation(&Vector::new(0.3, 0.2)) * Matrix::new_rotation(0.5);
        let inv = tr.try_inverse().unwrap();
        for &(tx, ty) in &[(0.2f32, -0.1f32), (-0.4, 0.05), (0., 0.)] {
            // Exactly what `Judge` does to a touch before comparing it to a note.
            let local = inv.transform_point(&CorePoint::new(tx, -ty));
            // The same location, taken through the note path instead.
            let from_note = note_world(&tr, Vector::new(local.x, local.y * aspect), aspect);
            let from_touch = screen_to_world(vec2(tx, ty), aspect);
            assert!((from_note - from_touch).length() < 1e-4, "{from_note:?} vs {from_touch:?}");
        }
    }

    // ---- 边界：退化几何 ----

    #[test]
    fn zero_extent_and_inverted_rectangles_behave() {
        let flat = BlockArea {
            bottom_left_percentage: Point { x: 0.5, y: 0.5 },
            top_right_percentage: Point { x: 0.5, y: 0.5 },
            enable_time: 0.,
            disable_time: 1.,
            disappear_time: 1.,
            ..Default::default()
        };
        assert!(flat.active(0.5));
        assert!(!blocked(&[flat], Vec2::ZERO, 0.5, 1.));

        let inverted = BlockArea {
            bottom_left_percentage: Point { x: 0.8, y: 0.8 },
            top_right_percentage: Point { x: 0.2, y: 0.2 },
            enable_time: 0.,
            disable_time: 1.,
            disappear_time: 1.,
            ..Default::default()
        };
        assert!(blocked(&[inverted], Vec2::ZERO, 0.5, 1.));
    }

    #[test]
    fn rotation_moves_the_hit_region_out_of_its_unrotated_extent() {
        let base = BlockArea {
            bottom_left_percentage: Point { x: 0.2, y: 0.3 },
            top_right_percentage: Point { x: 0.8, y: 0.7 },
            enable_time: 0.,
            disable_time: 2.,
            disappear_time: 3.,
            ..Default::default()
        };
        let rotated = BlockArea {
            rotate_events: vec![BlockRotateEvent {
                anchor: Point { x: 0.5, y: 0.5 },
                time: 0.,
                rotation: 90.,
                ease_type: 0,
            }],
            ..base.clone()
        };
        // 6 x 4 world units unrotated; this probe sits above the top edge.
        let probe = vec2(0., 2.5);
        assert!(!blocked(&[base], probe, 1., 1.));
        assert!(blocked(&[rotated], probe, 1., 1.));
    }

    #[test]
    fn scaling_to_zero_disables_the_region() {
        let area = BlockArea {
            bottom_left_percentage: Point { x: 0.2, y: 0.2 },
            top_right_percentage: Point { x: 0.8, y: 0.8 },
            enable_time: 0.,
            disable_time: 2.,
            disappear_time: 3.,
            scale_events: vec![BlockScaleEvent {
                anchor: Point { x: 0.5, y: 0.5 },
                time: 0.,
                scale: Point { x: 0., y: 0. },
                ease_type_x: 0,
                ease_type_y: 0,
            }],
            ..Default::default()
        };
        assert!(!blocked(&[area], Vec2::ZERO, 1., 1.));
    }

    #[test]
    fn subtract_outside_the_union_does_not_remove_anything() {
        let normal = area(false);
        let far_away = BlockArea {
            bottom_left_percentage: Point { x: 0.05, y: 0.05 },
            top_right_percentage: Point { x: 0.1, y: 0.1 },
            enable_time: 1.,
            disable_time: 3.,
            disappear_time: 4.,
            is_subtract: true,
            ..Default::default()
        };
        assert!(blocked(&[normal.clone(), far_away], Vec2::ZERO, 2., 1.));
        // A subtract region that covers the probe still punches the hole.
        assert!(!blocked(&[normal, area(true)], Vec2::ZERO, 2., 1.));
    }

    // ---- 边界：时间相位 ----

    #[test]
    fn phase_boundaries_and_extreme_times() {
        let area = BlockArea {
            appear_time: 1.,
            enable_time: 2.,
            disable_time: 3.,
            disappear_time: 4.,
            ..area(false)
        };
        assert!(area.visible(1.));
        assert!(!area.visible(4.));
        assert!(area.active(2.));
        assert!(!area.active(3.));
        assert!(!area.active(1.999));
        for t in [-1e9f32, -1e-3, 1e9, f32::MAX] {
            assert!(!area.active(t), "t={t}");
            assert_eq!(area.visible(t), (1. ..4.).contains(&t), "t={t}");
        }
    }

    #[test]
    fn zero_length_windows_are_never_active_or_visible() {
        let area = BlockArea {
            appear_time: 2.,
            enable_time: 2.,
            disable_time: 2.,
            disappear_time: 2.,
            ..area(false)
        };
        for t in [0., 2., 3.] {
            assert!(!area.active(t), "t={t}");
            assert!(!area.visible(t), "t={t}");
        }
    }

    #[test]
    fn ready_phase_only_exists_when_appear_precedes_enable() {
        let instant = BlockArea {
            appear_time: 2.,
            enable_time: 2.,
            disable_time: 3.,
            disappear_time: 4.,
            ..area(false)
        };
        assert!(!instant.ready(1.8));
        assert!(!instant.ready(2.));
        assert_eq!(instant.fade(2.), 1.);
        // Longer lead-in means the ready window starts earlier.
        let lead = BlockArea {
            enable_time: 3.,
            ..instant.clone()
        };
        assert!(lead.ready(3. - BLOCK_FADE_DURATION));
        assert!(!lead.ready(3. - BLOCK_FADE_DURATION - 1e-3));
    }

    // ---- 边界：normalize ----

    #[test]
    fn normalize_clamps_reversed_time_points() {
        let mut area = BlockArea {
            appear_time: 3.,
            enable_time: 1.,
            disable_time: 2.,
            disappear_time: 0.,
            ..Default::default()
        };
        area.normalize().unwrap();
        assert_eq!(area.appear_time, 3.);
        assert_eq!(area.enable_time, 3.);
        assert_eq!(area.disable_time, 3.);
        assert_eq!(area.disappear_time, 3.);
    }

    #[test]
    fn normalize_rejects_every_non_finite_field() {
        let cases: [fn(&mut BlockArea); 8] = [
            |a| a.appear_time = f32::NAN,
            |a| a.enable_time = f32::INFINITY,
            |a| a.disable_time = f32::NEG_INFINITY,
            |a| a.disappear_time = f32::NAN,
            |a| a.top_right_percentage.x = f32::NAN,
            |a| a.top_right_percentage.y = f32::INFINITY,
            |a| a.bottom_left_percentage.x = f32::NAN,
            |a| a.bottom_left_percentage.y = f32::NAN,
        ];
        for (i, mutate) in cases.into_iter().enumerate() {
            let mut area = area(false);
            mutate(&mut area);
            assert!(area.normalize().is_err(), "case {i} slipped through");
        }
    }

    #[test]
    fn normalize_rejects_unknown_ease_types_in_every_event_kind() {
        let mut with_move = area(false);
        with_move.move_events = vec![BlockMoveEvent {
            ease_type_x: -1,
            ..Default::default()
        }];
        assert!(with_move.normalize().is_err());

        let mut with_scale = area(false);
        with_scale.scale_events = vec![BlockScaleEvent {
            ease_type_y: 15,
            ..Default::default()
        }];
        assert!(with_scale.normalize().is_err());

        let mut with_rotate = area(false);
        with_rotate.rotate_events = vec![BlockRotateEvent {
            ease_type: 15,
            ..Default::default()
        }];
        assert!(with_rotate.normalize().is_err());
    }

    #[test]
    fn normalize_sorts_unordered_events_and_accepts_empty_lists() {
        let mut area = area(false);
        area.move_events = vec![
            BlockMoveEvent {
                time: 2.,
                end_position: Point { x: 0.9, y: 0.5 },
                ..Default::default()
            },
            BlockMoveEvent {
                time: 1.,
                end_position: Point { x: 0.5, y: 0.5 },
                ..Default::default()
            },
        ];
        area.normalize().unwrap();
        assert!(area.move_events[0].time < area.move_events[1].time);
        // Movement would relocate the region, so clear the events before probing.
        area.move_events.clear();
        area.scale_events.clear();
        area.rotate_events.clear();
        area.normalize().unwrap();
        assert!(blocked(&[area], Vec2::ZERO, 2., 1.));
    }

    // ---- 边界：缓动表 ----

    #[test]
    fn every_ease_type_is_monotone_and_bounded() {
        for n in 0..=14 {
            let mut previous = ease_raw(n, 0.);
            for i in 0..=EASE_SAMPLES {
                let t = i as f32 / EASE_SAMPLES as f32;
                let v = ease_raw(n, t);
                assert!((-1e-6..=1. + 1e-6).contains(&v), "type={n} t={t} v={v}");
                assert!(v >= previous - 1e-6, "type={n} regressed at t={t}");
                previous = v;
            }
        }
    }

    #[test]
    fn table_interpolation_is_continuous_and_hits_both_ends() {
        for n in [0, 1, 4, 7, 10, 12, 13, 14] {
            assert!((ease(n, 0.) - ease_raw(n, 0.)).abs() < 1e-5, "type={n}");
            assert!((ease(n, 1.) - ease_raw(n, 1.)).abs() < 1e-5, "type={n}");
            let mut previous = ease(n, 0.);
            for i in 1..=200 {
                let v = ease(n, i as f32 / 200.);
                assert!((v - previous).abs() < 0.05, "type={n} jumped at step {i}");
                previous = v;
            }
        }
    }

    #[test]
    fn unknown_ease_types_fall_back_to_linear() {
        for n in [-7, 15, 99] {
            assert!((ease(n, 0.3) - 0.3).abs() < 1e-4, "type={n}");
        }
    }

    // ---- 边界：状态机 ----

    #[test]
    fn disabling_the_switch_clears_state_and_blocks_nothing() {
        let mut state = NoiseAreaState::default();
        state.update(&[area(false)], &[(1, Vec2::ZERO)], 2., 1., true, 0.016);
        assert!(state.is_blocked(1));
        state.update(&[area(false)], &[(1, Vec2::ZERO)], 2., 1., false, 0.016);
        assert!(!state.is_blocked(1));
        assert!(state.positions.is_empty());
        assert!(state.hovers.iter().all(|h| h.finger.is_none()));
    }

    #[test]
    fn negative_and_huge_frame_times_keep_the_hover_animation_sane() {
        let mut state = NoiseAreaState::default();
        state.update(&[area(false)], &[(1, Vec2::ZERO)], 2., 1., true, -5.);
        assert!(state.hovers[0].scale.is_finite() && state.hovers[0].scale >= 0.);
        for _ in 0..3 {
            state.update(&[area(false)], &[(1, Vec2::ZERO)], 2., 1., true, 1e6);
        }
        assert_eq!(state.hovers[0].scale, 11.5);
    }

    #[test]
    fn lifting_a_finger_frees_its_hover_slot_for_reuse() {
        let mut state = NoiseAreaState::default();
        state.update(&[area(false)], &[(1, Vec2::ZERO), (2, Vec2::ZERO)], 2., 1., true, 0.016);
        assert!(state.is_blocked(1) && state.is_blocked(2));
        state.update(&[area(false)], &[(2, Vec2::ZERO)], 2., 1., true, 0.016);
        assert!(!state.is_blocked(1));
        assert_eq!(state.hovers.iter().filter(|h| h.finger == Some(1)).count(), 0);
        state.update(&[area(false)], &[(3, Vec2::ZERO)], 2., 1., true, 0.016);
        assert!(state.hovers.iter().any(|h| h.finger == Some(3)));
    }

    #[test]
    fn blocked_state_survives_time_jumps_until_the_finger_lifts() {
        let mut state = NoiseAreaState::default();
        state.update(&[area(false)], &[(9, Vec2::ZERO)], 2., 1., true, 0.016);
        assert!(state.is_blocked(9));
        state.update(&[], &[(9, vec2(999., 999.))], 1e6, 1., true, 0.016);
        assert!(state.is_blocked(9));
        state.update(&[], &[], 1e6, 1., true, 0.016);
        assert!(!state.is_blocked(9));
    }

    #[test]
    fn thousands_of_areas_stay_correct_and_within_budget() {
        let areas: Vec<BlockArea> = (0..1000)
            .map(|i| {
                let base = (i % 32) as f32 / 32.;
                BlockArea {
                    bottom_left_percentage: Point { x: base, y: base },
                    top_right_percentage: Point {
                        x: base + 1. / 64.,
                        y: base + 1. / 64.,
                    },
                    enable_time: 0.,
                    disable_time: 1e6,
                    disappear_time: 1e6,
                    ..Default::default()
                }
            })
            .collect();
        let mut state = NoiseAreaState::default();
        let started = std::time::Instant::now();
        for frame in 0..30u64 {
            // Fresh ids each frame: reusing ids would short-circuit on blocked_ids.
            let touches: Vec<(u64, Vec2)> = (0..10).map(|i| (frame * 10 + i, vec2(i as f32 - 5., 0.))).collect();
            state.update(&areas, &touches, 1., 1., true, 0.016);
        }
        assert!(state.blocked_ids.len() <= 10);
        assert!(started.elapsed().as_millis() < 2000, "block area update is too slow");
    }

    // ---- 边界：谱面解析 ----

    const MINIMAL_CHART: &str = r#"{"formatVersion":3,"offset":0,"judgeLineList":[]}"#;

    #[test]
    fn charts_without_block_area_list_keep_an_empty_set() {
        let chart = parse_phigros(MINIMAL_CHART, ChartExtra::default()).unwrap();
        assert!(chart.extra.block_areas.is_empty());
    }

    #[test]
    fn empty_and_null_block_area_lists_are_both_accepted() {
        for suffix in [r#","blockAreaList":[]"#, r#","blockAreaList":null"#] {
            let json = format!(r#"{{"formatVersion":3,"offset":0,"judgeLineList":[]{suffix}}}"#);
            match parse_phigros(&json, ChartExtra::default()) {
                Ok(chart) => assert!(chart.extra.block_areas.is_empty(), "{json}"),
                Err(err) => panic!("{json} should still parse, got {err}"),
            }
        }
    }

    #[test]
    fn invalid_block_areas_are_dropped_without_failing_the_chart() {
        let json = r#"{"formatVersion":3,"offset":0,"judgeLineList":[],"blockAreaList":[
            {"bottomLeftPercentage":{"x":0.2,"y":0.2},"topRightPercentage":{"x":0.8,"y":0.8},"appearTime":0,"enableTime":1,"disableTime":2,"disappearTime":3},
            {"bottomLeftPercentage":{"x":0.2,"y":0.2},"topRightPercentage":{"x":0.8,"y":0.8},"enableTime":1,"disableTime":2,"disappearTime":3,"moveEvents":[{"time":0,"endPosition":{"x":0.5,"y":0.5},"easeTypeX":99}]}
        ]}"#;
        let chart = parse_phigros(json, ChartExtra::default()).unwrap();
        assert_eq!(chart.extra.block_areas.len(), 1, "the invalid entry should be skipped");
    }
}
