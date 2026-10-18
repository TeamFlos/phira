//! Judgement system
//!
//! The core (`advance` and its `step`s) judges a play against nothing but the
//! chart, a [`JudgeEnv`] and the canonical [`InputEvent`] stream — see
//! [`input`]. Live play produces that stream through the [`InputCollector`]
//! and appends every event it judges by to [`Judge::inputs`], so a replay
//! with [`Judge::replay`] goes through the same code and reproduces the run
//! note for note. Keyboard hits are the one exception — they never enter the
//! stream. Flick *judgement* is reproducible through the recorded swipe
//! steps; how those were detected is something only the producing client can
//! vouch for.
//!
//! The rules: symmetric Perfect/Good windows with no late Bad, a Bad window
//! on the early side that shrinks towards the lateral edge of the click
//! area, drags and flicks judged by contact alone, and click matching that
//! walks the candidates in global time order, so an early press can be
//! eaten by a drag aimed at a later tap.

use crate::{
    config::{Config, Mods},
    core::{BadNote, Chart, Matrix, NoteKind, Point, Resource, Vector},
    ext::{get_viewport, NotNanExt},
    play_record::PlayRecord,
};
use input::{InputCollector, InputSource, RawFrame, RawTouch, ReplaySource};
use macroquad::prelude::{
    utils::{register_input_subscriber, repeat_all_miniquad_input},
    *,
};
use miniquad::{EventHandler, MouseButton};
use once_cell::sync::Lazy;
use sasa::{PlaySfxParams, Sfx};
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    cmp::Reverse,
    collections::{BinaryHeap, HashSet},
    mem,
    num::FpCategory,
    sync::Arc,
};
use tracing::debug;

pub mod input;
pub use input::{swipe_dpi, InputEvent, MovePoint};

#[cfg(test)]
mod tests;

pub const LIMIT_PERFECT: f64 = 0.08;
pub const LIMIT_GOOD: f64 = 0.18;
pub const LIMIT_BAD: f64 = 0.22;
/// How far from its time a drag still counts as touched, and a flick as
/// swiped.
pub const LIMIT_DRAG: f64 = 0.10;
pub const LIMIT_FLICK: f64 = 0.14;
pub const UP_TOLERANCE: f64 = 0.05;
/// Two candidates this close in time count as simultaneous for click
/// matching, and are told apart by distance instead.
const SAME_TIME: f64 = 0.01;

/// One world unit — a tenth of the screen height — in this codebase's
/// line-local units (halves of the viewport width), at the reference 16:9
/// aspect.
const WU: f64 = 0.2 / (16. / 9.);
/// Lateral reach of click matching and of a hold's mid-contact check.
const CLICK_WIDTH: f64 = 1.9 * WU;
/// Lateral reach of a drag's touch check and a flick's swipe check.
const CONTACT_WIDTH: f64 = 2.1 * WU;

/// Rate, in Hz, of the fixed grid the judge samples touch positions on
/// (`tick / JUDGE_TICK_HZ` in chart seconds). Hold and drag judgement is
/// frame-rate independent because it happens on this grid, not per frame.
pub const JUDGE_TICK_HZ: f64 = 30.;
/// Movement below this (max of the per-axis deltas, in the stream's
/// coordinate space) is not worth a [`InputEvent::Move`].
pub const MOVE_DEADBAND: f32 = 2e-3;

/// Version of the judging rules. Bump it whenever the same input could be
/// judged differently, so that a recorded play can be re-judged under the
/// rules it was actually played with.
pub const JUDGE_VERSION: u8 = 2;

#[derive(Debug, Clone)]
pub enum HitSound {
    None,
    Click,
    Flick,
    Drag,
    Custom(String),
}

impl HitSound {
    pub fn play(&self, res: &mut Resource) {
        match self {
            HitSound::None => {}
            HitSound::Click => play_sfx(&mut res.sfx_click, &res.config),
            HitSound::Flick => play_sfx(&mut res.sfx_flick, &res.config),
            HitSound::Drag => play_sfx(&mut res.sfx_drag, &res.config),
            HitSound::Custom(s) => {
                if let Some(sfx) = res.extra_sfxs.get_mut(s) {
                    play_sfx(sfx, &res.config);
                }
            }
        }
    }

    pub fn default_from_kind(kind: &NoteKind) -> Self {
        match kind {
            NoteKind::Click => HitSound::Click,
            NoteKind::Flick => HitSound::Flick,
            NoteKind::Drag => HitSound::Drag,
            NoteKind::Hold { .. } => HitSound::Click,
        }
    }
}

pub fn play_sfx(sfx: &mut Sfx, config: &Config) {
    if config.volume_sfx <= 1e-2 {
        return;
    }
    let _ = sfx.play(PlaySfxParams {
        amplifier: config.volume_sfx,
    });
}

#[cfg(all(not(target_os = "windows"), not(target_os = "ios")))]
pub(crate) fn get_uptime() -> f64 {
    let mut time = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    let ret = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut time) };
    assert!(ret == 0);
    time.tv_sec as f64 + time.tv_nsec as f64 * 1e-9
}

#[cfg(target_os = "ios")]
pub(crate) fn get_uptime() -> f64 {
    objc2_foundation::NSProcessInfo::processInfo().systemUptime()
}

#[cfg(target_os = "windows")]
pub(crate) fn get_uptime() -> f64 {
    miniquad::native::windows::get_uptime()
}

#[derive(Debug)]
pub enum JudgeStatus {
    NotJudged,
    PreJudge,
    Judged,
    Hold(bool, f64, f64, bool, f64), // perfect, at, diff, pre-judge, up-time
}

#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Judgement {
    Perfect,
    Good,
    Bad,
    Miss,
}

impl From<Judgement> for u8 {
    fn from(judgement: Judgement) -> Self {
        judgement as Self
    }
}

impl TryFrom<u8> for Judgement {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Ok(match value {
            0 => Self::Perfect,
            1 => Self::Good,
            2 => Self::Bad,
            3 => Self::Miss,
            _ => return Err("invalid judgement"),
        })
    }
}

impl Serialize for Judgement {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(*self as u8)
    }
}

impl<'de> Deserialize<'de> for Judgement {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(u8::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// A single resolved judgement, kept for post-play statistics.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HitEvent {
    pub line_id: u32,
    pub note_id: u32,
    pub judgement: Judgement,
    /// Timing error in seconds, negative meaning early. `None` where there is
    /// no meaningful one: misses, and every drag or flick.
    pub diff: Option<f64>,
    /// Set when this miss is a hold that was hit and then released early,
    /// which is otherwise indistinguishable from one never touched at all.
    pub hold_broken: bool,
}

#[cfg(not(closed))]
#[derive(Default)]
pub(crate) struct JudgeInner {
    diffs: Vec<f64>,

    combo: u32,
    max_combo: u32,
    counts: [u32; 4],
    num_of_notes: u32,
    early_kind: [u32; 4],
    late_kind: [u32; 4],
}

#[cfg(not(closed))]
impl JudgeInner {
    pub fn new(num_of_notes: u32) -> Self {
        Self {
            diffs: Vec::new(),

            combo: 0,
            max_combo: 0,
            counts: [0; 4],
            num_of_notes,
            early_kind: [0; 4],
            late_kind: [0; 4],
        }
    }

    pub fn commit(&mut self, what: Judgement, diff: f64) {
        use Judgement::*;
        if matches!(what, Judgement::Good) {
            self.diffs.push(diff);
        }
        if diff < 0. {
            self.early_kind[what as usize] += 1;
        } else if diff > 0. {
            self.late_kind[what as usize] += 1;
        }
        self.counts[what as usize] += 1;
        match what {
            Perfect | Good => {
                self.combo += 1;
                if self.combo > self.max_combo {
                    self.max_combo = self.combo;
                }
            }
            _ => {
                self.combo = 0;
            }
        }
    }

    pub fn reset(&mut self) {
        self.combo = 0;
        self.max_combo = 0;
        self.counts = [0; 4];
        self.diffs.clear();
        self.early_kind = [0; 4];
        self.late_kind = [0; 4];
    }

    pub fn accuracy(&self) -> f64 {
        (self.counts[0] as f64 + self.counts[1] as f64 * 0.65) / self.num_of_notes as f64
    }

    pub fn real_time_accuracy(&self) -> f64 {
        let cnt = self.counts.iter().sum::<u32>();
        if cnt == 0 {
            return 1.;
        }
        (self.counts[0] as f64 + self.counts[1] as f64 * 0.65) / cnt as f64
    }

    pub fn score(&self) -> u32 {
        const TOTAL: u32 = 1000000;
        if self.counts[0] == self.num_of_notes {
            TOTAL
        } else {
            let score = (0.9 * self.accuracy() + self.max_combo as f64 / self.num_of_notes as f64 * 0.1) * TOTAL as f64;
            score.round() as u32
        }
    }

    pub fn result(&self) -> PlayResult {
        let early = self.diffs.iter().filter(|it| **it < 0.).count() as u32;
        PlayResult {
            score: self.score(),
            accuracy: self.accuracy(),
            max_combo: self.max_combo,
            num_of_notes: self.num_of_notes,
            counts: self.counts,
            early,
            late: self.diffs.len() as u32 - early,
            std: 0.,
            early_kind: self.early_kind,
            late_kind: self.late_kind,
        }
    }

    pub fn combo(&self) -> u32 {
        self.combo
    }

    pub fn counts(&self) -> [u32; 4] {
        self.counts
    }
}

#[rustfmt::skip]
#[cfg(closed)]
pub mod inner;
#[cfg(closed)]
use inner::*;

type Judgements = Vec<(f64, u32, u32, Result<Judgement, bool>)>;

/// Everything about the play the judge core needs beyond the chart and the
/// input stream: the coordinate frame the stream is interpreted against.
/// Live play builds one per frame, a replay passes the recorded values once.
#[derive(Clone, Copy, Debug)]
pub struct JudgeEnv {
    pub aspect_ratio: f32,
    pub speed: f64,
    pub flip_x: bool,
}

/// A side effect the core produced at time `time`, for the live side to
/// apply — particles, hit sounds, bad notes. A replay simply drops them.
#[derive(Clone, Copy, Debug)]
pub(crate) struct JudgeEffect {
    pub time: f64,
    pub line: u32,
    pub note: u32,
    pub kind: JudgeEffectKind,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum JudgeEffectKind {
    /// A hold head was hit: its hit sound plays right away.
    HoldHead,
    /// A note was resolved: `Perfect`/`Good` emit particles and play the
    /// note's sound, `Bad` spawns a bad note, `Miss` does nothing.
    Commit(Judgement),
}

/// A scheduled evaluation point: the exact time a pre-judged drag or flick
/// commits, or a held hold ends. Keeps commits and sounds on time instead of
/// waiting for the next tick.
#[derive(Clone, Copy, PartialEq)]
struct Sched {
    time: f64,
    line: u32,
    note: u32,
}

impl Eq for Sched {}

impl Ord for Sched {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.time
            .total_cmp(&other.time)
            .then_with(|| self.line.cmp(&other.line))
            .then_with(|| self.note.cmp(&other.note))
    }
}

impl PartialOrd for Sched {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// What scheduled the current evaluation point.
enum Cause {
    Tick(Vec<MovePoint>),
    Sched,
    Start { id: u8, x: f32, y: f32 },
    Appear { id: u8, x: f32, y: f32 },
    End { id: u8 },
    Swipe { step: SwipeStep, new: bool },
}

/// One swipe step due for judging, in the stream's coordinate space.
struct SwipeStep {
    id: u8,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
}

/// A candidate line at an evaluation point: the line's id, the live touches
/// mapped into its local frame, and the inverse of its transform, for
/// mapping swipe segments there too.
struct LineView {
    line: u32,
    inv: Matrix,
    touches: Vec<Option<Point>>,
}

/// A note resolved at an evaluation point, waiting to be committed.
struct Resolved {
    line: u32,
    note: u32,
    judgement: Judgement,
    diff: Option<f64>,
    hold_broken: bool,
}

/// The first tick strictly after `t`.
fn tick_after(t: f64) -> u32 {
    let mut tick = ((t * JUDGE_TICK_HZ).floor() + 1.).max(0.) as u32;
    while (tick as f64) / JUDGE_TICK_HZ <= t {
        tick += 1;
    }
    tick
}

#[repr(C)]
pub struct Judge {
    // notes of each line in order
    // LinkedList::drain_filter is unstable...
    pub notes: Vec<(Vec<u32>, usize)>,
    pub last_time: f64,

    key_down_count: u32,

    pub(crate) inner: JudgeInner,
    pub judgements: RefCell<Judgements>,
    /// Every resolved judgement of this run, in order. Only filled during real
    /// play — autoplay and spectator replay leave it empty, since neither
    /// produces timing data worth recording.
    pub hit_events: Vec<HitEvent>,
    /// The canonical input stream of this run, in processing order — what
    /// the judge consumed, at the precision it consumed it: presses,
    /// releases, tick-sampled positions, swipe steps, seeks. Only filled
    /// during real play; [`Judge::replay`] leaves it empty.
    pub inputs: Vec<InputEvent>,

    /// Chart time everything up to (inclusive) has been judged at.
    judged_until: f64,
    /// Next tick of the fixed grid to process.
    next_tick: u32,
    collector: InputCollector,
    /// Live touches in the stream's coordinate space, in order of arrival:
    /// slot and the last position emitted into the stream.
    live: Vec<(u8, f32, f32)>,
    /// Slots whose current swipe gesture has not judged a flick yet; a
    /// gesture arms on its first step and disarms on a match or an End.
    armed: HashSet<u8>,
    /// Notes a click has already been spent on — the once-only drag
    /// protection. Only drags ever land here: a tap is spent by being
    /// judged, a flick is never spent.
    consumed: HashSet<(u32, u32)>,
    sched: BinaryHeap<Reverse<Sched>>,
    /// Whether to skip lines whose next note is beyond the click search
    /// window — a pure optimization, off only in the equivalence test.
    filter_candidates: bool,
    /// Replays judge a recorded stream; they must not log it again.
    replaying: bool,
}

#[derive(Default)]
struct TouchStatus {
    touches: Vec<Touch>,
    key_delta: i32,
    keys_down: u32,
}

static SUBSCRIBER_ID: Lazy<usize> = Lazy::new(register_input_subscriber);
thread_local! {
    static TOUCHES: RefCell<TouchStatus> = RefCell::default();
    static WHEEL: RefCell<(f32, f32)> = RefCell::default();
}

pub fn take_wheel() -> (f32, f32) {
    WHEEL.with(|it| mem::take(&mut *it.borrow_mut()))
}

impl Judge {
    pub fn new(chart: &Chart) -> Self {
        let notes = chart
            .lines
            .iter()
            .map(|line| {
                let mut idx: Vec<u32> = (0..(line.notes.len() as u32)).filter(|it| !line.notes[*it as usize].fake).collect();
                idx.sort_by_key(|id| line.notes[*id as usize].time.not_nan());
                (idx, 0)
            })
            .collect();
        Self {
            notes,
            last_time: 0.,

            key_down_count: 0,

            inner: JudgeInner::new(chart.lines.iter().map(|it| it.notes.iter().filter(|it| !it.fake).count() as u32).sum()),
            judgements: RefCell::new(Vec::new()),
            hit_events: Vec::new(),
            inputs: Vec::new(),

            judged_until: 0.,
            next_tick: 0,
            collector: InputCollector::default(),
            live: Vec::new(),
            armed: HashSet::new(),
            consumed: HashSet::new(),
            sched: BinaryHeap::new(),
            filter_candidates: true,
            replaying: false,
        }
    }

    pub fn reset(&mut self) {
        self.notes.iter_mut().for_each(|it| it.1 = 0);
        self.inner.reset();
        self.judgements.borrow_mut().clear();
        self.hit_events.clear();
        self.inputs.clear();
        self.judged_until = 0.;
        self.next_tick = 0;
        self.collector.reset();
        self.live.clear();
        self.armed.clear();
        self.consumed.clear();
        self.sched.clear();
    }

    #[cfg(test)]
    pub(crate) fn set_candidate_filter(&mut self, on: bool) {
        self.filter_candidates = on;
    }

    /// Advance note pointers past notes before time `t`, marking them as judged.
    /// Used in exercise mode to skip notes before the exercise range start.
    pub fn advance_to(&mut self, chart: &mut Chart, t: f64) {
        for (line, (idx, st)) in chart.lines.iter_mut().zip(self.notes.iter_mut()) {
            while *st < idx.len() {
                let note = &mut line.notes[idx[*st] as usize];
                if note.time >= t {
                    break;
                }
                note.judge = JudgeStatus::Judged;
                *st += 1;
            }
        }
        self.last_time = t;
        self.judged_until = t;
        self.next_tick = tick_after(t);
        self.sched.clear();
    }

    pub fn commit(&mut self, t: f64, what: Judgement, line_id: u32, note_id: u32, diff: f64) {
        self.judgements.borrow_mut().push((t, line_id, note_id, Ok(what)));
        self.inner.commit(what, diff);
    }

    #[inline]
    pub fn accuracy(&self) -> f64 {
        self.inner.accuracy()
    }

    #[inline]
    pub fn real_time_accuracy(&self) -> f64 {
        self.inner.real_time_accuracy()
    }

    #[inline]
    pub fn score(&self) -> u32 {
        self.inner.score()
    }

    pub(crate) fn on_new_frame() {
        let mut handler = Handler {
            status: TouchStatus::default(),
            wheel: (0., 0.),
        };
        repeat_all_miniquad_input(&mut handler, *SUBSCRIBER_ID);
        handler.finalize();
        TOUCHES.with(|it| {
            *it.borrow_mut() = handler.status;
        });
        WHEEL.with(|it| {
            *it.borrow_mut() = handler.wheel;
        });
    }

    fn touch_transform(flip_x: bool) -> impl Fn(&mut Touch) {
        let vp = get_viewport();
        move |touch| {
            let p = touch.position;
            touch.position = vec2(
                (p.x - vp.0 as f32) / vp.2 as f32 * 2. - 1.,
                ((p.y - (screen_height() - (vp.1 + vp.3) as f32)) / vp.3 as f32 * 2. - 1.) / (vp.2 as f32 / vp.3 as f32),
            );
            if flip_x {
                touch.position.x *= -1.;
            }
        }
    }

    pub fn get_touches() -> Vec<Touch> {
        TOUCHES.with(|it| {
            let guard = it.borrow();
            let tr = Self::touch_transform(false);
            guard
                .touches
                .iter()
                .cloned()
                .map(|mut it| {
                    tr(&mut it);
                    it
                })
                .collect()
        })
    }

    pub fn update(&mut self, res: &mut Resource, chart: &mut Chart, bad_notes: &mut Vec<BadNote>) {
        if res.config.autoplay() {
            self.auto_play_update(res, chart);
            return;
        }
        let spd = res.config.speed as f64;

        let uptime = get_uptime();

        let t = res.time;
        let (events, keys_down, key_delta) = TOUCHES.with(|it| {
            let guard = it.borrow();
            if res.config.use_keyboard {
                (guard.touches.clone(), guard.keys_down, guard.key_delta)
            } else {
                (guard.touches.clone(), 0, 0)
            }
        });
        self.key_down_count = self.key_down_count.saturating_add_signed(key_delta);
        // Everything works in the stream's coordinate space: the
        // `get_touches` space with `y` scaled by the aspect ratio, and x not
        // flipped.
        let aspect = res.aspect_ratio;

        let vp = get_viewport();
        let to_record = |Vec2 { x: px, y: py }| {
            let x = (px - vp.0 as f32) / vp.2 as f32 * 2. - 1.;
            let y = ((py - (screen_height() - (vp.1 + vp.3) as f32)) / vp.3 as f32 * 2. - 1.) / (vp.2 as f32 / vp.3 as f32) * aspect;
            (x, y)
        };
        let mut live: Vec<(u64, f32, f32)> = touches()
            .into_iter()
            .filter(|it| !matches!(it.phase, TouchPhase::Ended | TouchPhase::Cancelled))
            .map(|it| {
                let (x, y) = to_record(it.position);
                (it.id, x, y)
            })
            .collect();
        {
            let btn = MouseButton::Left;
            if is_mouse_button_down(btn) {
                let (x, y) = to_record(mouse_position().into());
                live.push((button_to_id(btn), x, y));
            }
        }
        let raw: Vec<RawTouch> = events
            .iter()
            .map(|it| {
                let (x, y) = to_record(it.position);
                RawTouch {
                    id: it.id,
                    phase: it.phase,
                    time: it.time,
                    x,
                    y,
                }
            })
            .collect();
        let env = JudgeEnv {
            aspect_ratio: res.aspect_ratio,
            speed: spd,
            flip_x: res.config.flip_x(),
        };
        let mut effects = Vec::new();
        self.update_with(
            chart,
            &env,
            RawFrame {
                t,
                uptime,
                speed: spd,
                aspect,
                dpi: res.dpi,
                raw: &raw,
                live: &live,
            },
            keys_down,
            &mut effects,
        );
        self.apply_effects(res, chart, bad_notes, effects);
        self.last_time = t / spd;
    }

    /// One frame of live judging, free of anything macroquad-global: the raw
    /// events and the live touch set arrive already in the stream's
    /// coordinate space, and `uptime` is what converts their OS timestamps.
    /// This is the entry the tests drive directly.
    fn update_with(&mut self, chart: &mut Chart, env: &JudgeEnv, frame: RawFrame, keys_down: u32, effects: &mut Vec<JudgeEffect>) {
        let t = frame.t;
        if t < self.judged_until {
            // The clock jumped back: resuming rewinds about three seconds,
            // the keyboard seeks, and every chart's prelude ends that way
            // too. Everything queued but unconsumed belonged to the
            // abandoned timeline. The clock floor follows the jump right
            // away — `advance` will apply the queued event to the same
            // effect.
            let from = self.judged_until;
            let mut collector = mem::take(&mut self.collector);
            collector.push_seek(from, t, frame.live);
            self.collector = collector;
            self.log(InputEvent::Seek { from, to: t });
            self.judged_until = t;
        }
        {
            let mut collector = mem::take(&mut self.collector);
            collector.feed(frame, self.judged_until);
            self.collector = collector;
        }
        {
            let mut collector = mem::take(&mut self.collector);
            let _ = self.advance(chart, env, &mut collector, t, effects, false);
            self.collector = collector;
        }
        // Keyboard hits are the one input the stream does not carry. They
        // are judged at the frame's time — keyboard plays are never
        // recorded, so they may resolve everything at `t` the way the old
        // per-frame loop did.
        if keys_down > 0 {
            for _ in 0..keys_down {
                self.keyboard_hit(chart, env, t, effects);
            }
            let views = self.line_views(chart, env, t);
            self.pass_miss(chart, env, t, &views, effects);
            self.pass_continuous(chart, env, t, &views, effects);
            self.pass_pre_judged(chart, env, t, &views, effects);
            self.advance_st(chart);
        }
    }

    /// Advances the simulation to `until` (inclusive), evaluation point by
    /// evaluation point: fixed grid ticks, scheduled commits and input
    /// events, in that order at equal times. This — and only this — is what
    /// both live play and replays judge by.
    ///
    /// With `stop_after_seek`, the first [`InputEvent::Seek`] that applies
    /// rewinds the judge's clock floor to its target and returns it instead
    /// of judging on through: the world above — music, notes, effects — has
    /// to jump back with it before judging may continue. `None` otherwise.
    fn advance(
        &mut self,
        chart: &mut Chart,
        env: &JudgeEnv,
        src: &mut dyn InputSource,
        until: f64,
        effects: &mut Vec<JudgeEffect>,
        stop_after_seek: bool,
    ) -> Option<f64> {
        loop {
            let sched = self.peek_sched(chart);
            let tick_time = self.next_tick as f64 / JUDGE_TICK_HZ;
            // A seek rewinds the clock rather than evaluating a point. It
            // applies once every tick and scheduled point up to its `from`
            // has been processed — live, those were processed in earlier
            // frames; a replay has to catch up on them first — and then
            // `until` notwithstanding.
            if src.peek_is_seek() && sched.is_none_or(|it| it.time > src.peek_time().unwrap()) && tick_time > src.peek_time().unwrap() {
                let Some(InputEvent::Seek { to, .. }) = src.pop() else {
                    unreachable!("peek_is_seek lied");
                };
                self.judged_until = to;
                self.next_tick = tick_after(to);
                if stop_after_seek {
                    return Some(to);
                }
                continue;
            }
            let tau = [Some(tick_time), sched.map(|it| it.time), src.peek_time()]
                .into_iter()
                .flatten()
                .reduce(f64::min);
            let Some(tau) = tau else {
                break;
            };
            if tau > until {
                break;
            }
            if sched.is_some_and(|it| it.time == tau) {
                self.sched.pop();
                self.step(chart, env, tau, Cause::Sched, effects);
            } else if tick_time == tau {
                let tick = self.next_tick;
                self.next_tick += 1;
                let points = src.moves_at(tick);
                if !points.is_empty() {
                    self.log(InputEvent::Move {
                        tick,
                        points: points.clone(),
                    });
                }
                self.step(chart, env, tau, Cause::Tick(points), effects);
            } else {
                let event = src.pop().expect("peek_time lied");
                let cause = match event {
                    InputEvent::Start { id, x, y, .. } => {
                        self.log(event);
                        Cause::Start { id, x, y }
                    }
                    InputEvent::Appear { id, x, y, .. } => {
                        self.log(event);
                        Cause::Appear { id, x, y }
                    }
                    InputEvent::End { id, .. } => {
                        self.log(event);
                        Cause::End { id }
                    }
                    InputEvent::Swipe { id, new, x0, y0, x1, y1, .. } => {
                        self.log(event);
                        Cause::Swipe {
                            step: SwipeStep { id, x0, y0, x1, y1 },
                            new,
                        }
                    }
                    InputEvent::Move { .. } | InputEvent::Seek { .. } => unreachable!("moves and seeks do not reach the step"),
                };
                self.step(chart, env, tau, cause, effects);
            }
            self.judged_until = tau;
        }
        None
    }

    /// Processes one evaluation point at `tau`: applies the cause to the
    /// live set, then misses and hold tails, then the click (a Start judges
    /// as one at exactly `tau`), then the continuous checks, then the
    /// pre-judged commits.
    fn step(&mut self, chart: &mut Chart, env: &JudgeEnv, tau: f64, cause: Cause, effects: &mut Vec<JudgeEffect>) {
        let (click, swipe) = match cause {
            Cause::Tick(points) => {
                for point in points {
                    if let Some(entry) = self.live.iter_mut().find(|it| it.0 == point.id) {
                        entry.1 = point.x;
                        entry.2 = point.y;
                    }
                }
                (None, None)
            }
            Cause::Start { id, x, y } | Cause::Appear { id, x, y } => {
                self.live.retain(|it| it.0 != id);
                self.live.push((id, x, y));
                if matches!(cause, Cause::Start { .. }) {
                    (Some(id), None)
                } else {
                    (None, None)
                }
            }
            Cause::End { id } => {
                self.live.retain(|it| it.0 != id);
                self.armed.remove(&id);
                (None, None)
            }
            Cause::Swipe { step, new } => {
                // a swipe is neither a click nor a position update; it only
                // judges flicks, one per gesture
                if new {
                    self.armed.insert(step.id);
                }
                (None, Some(step))
            }
            Cause::Sched => (None, None),
        };
        let views = self.line_views(chart, env, tau);
        self.pass_miss(chart, env, tau, &views, effects);
        if let Some(slot) = click {
            self.click(chart, env, tau, slot, &views, effects);
        }
        if let Some(step) = swipe {
            self.swipe(chart, env, tau, &step, &views, effects);
        }
        self.pass_continuous(chart, env, tau, &views, effects);
        self.pass_pre_judged(chart, env, tau, &views, effects);
        self.advance_st(chart);
    }

    /// The candidate lines at `tau` — those with a note that could still
    /// matter — with their whole ancestor chain seeked to `tau`, and the
    /// live touches mapped into each line's local frame.
    fn line_views(&self, chart: &mut Chart, env: &JudgeEnv, tau: f64) -> Vec<LineView> {
        let mut candidates: Vec<u32> = Vec::new();
        for (id, (idx, st)) in self.notes.iter().enumerate() {
            let Some(&first) = idx.get(*st) else {
                continue;
            };
            if !self.filter_candidates || chart.lines[id].notes[first as usize].time <= tau + LIMIT_BAD * env.speed {
                candidates.push(id as u32);
            }
        }
        // Seek every candidate's whole ancestor chain: a parent left at an
        // older time would distort the child's transform.
        let mut seen: Vec<usize> = Vec::new();
        for &id in &candidates {
            let mut cur = Some(id as usize);
            while let Some(i) = cur {
                if !seen.contains(&i) {
                    seen.push(i);
                    chart.lines[i].object.set_time(tau);
                }
                cur = chart.lines[i].parent;
            }
        }
        candidates
            .iter()
            .map(|&id| {
                let inv = chart.lines[id as usize]
                    .now_transform_with(env.aspect_ratio, &chart.lines)
                    .try_inverse()
                    .unwrap();
                fn ok(f: f32) -> bool {
                    matches!(f.classify(), FpCategory::Zero | FpCategory::Subnormal | FpCategory::Normal)
                }
                let touches = self
                    .live
                    .iter()
                    .map(|&(_, x, y)| {
                        let p = inv.transform_point(&Point::new(if env.flip_x { -x } else { x }, -(y / env.aspect_ratio)));
                        (ok(p.x) && ok(p.y)).then_some(p)
                    })
                    .collect();
                LineView { line: id, inv, touches }
            })
            .collect()
    }

    /// Expired notes miss, and holds approach their end. Positions play no
    /// part, so this runs before the click: whether a note can still be hit
    /// never depends on the grid.
    fn pass_miss(&mut self, chart: &mut Chart, env: &JudgeEnv, tau: f64, views: &[LineView], effects: &mut Vec<JudgeEffect>) {
        let spd = env.speed;
        let mut resolved = Vec::new();
        for view in views {
            let line = &mut chart.lines[view.line as usize];
            let (idx, st) = &self.notes[view.line as usize];
            for id in &idx[*st..] {
                let note = &mut line.notes[*id as usize];
                if let NoteKind::Hold { end_time, .. } = &note.kind {
                    if let JudgeStatus::Hold(.., pre_judge, _) = &mut note.judge {
                        if (*end_time - tau) / spd <= LIMIT_BAD {
                            *pre_judge = true;
                        }
                        continue;
                    }
                }
                if !matches!(note.judge, JudgeStatus::NotJudged) {
                    continue;
                }
                // every kind misses on its own window: taps and hold heads
                // past the good window, drags past their contact window,
                // flicks past their swipe window
                let dt = (tau - note.time) / spd;
                let missed = dt
                    > match note.kind {
                        NoteKind::Click | NoteKind::Hold { .. } => LIMIT_GOOD,
                        NoteKind::Drag => LIMIT_DRAG,
                        NoteKind::Flick => LIMIT_FLICK,
                    };
                if missed {
                    note.judge = JudgeStatus::Judged;
                    resolved.push(Resolved {
                        line: view.line,
                        note: *id,
                        judgement: Judgement::Miss,
                        diff: None,
                        hold_broken: false,
                    });
                    continue;
                }
                if -dt > LIMIT_BAD {
                    break;
                }
            }
        }
        self.flush(chart, env, tau, resolved, effects);
    }

    /// Continuous checks against the positions on the grid: holds whose
    /// finger went away, and drags a held finger touches into place (flicks
    /// only through the keyboard's stand-in for a finger).
    fn pass_continuous(&mut self, chart: &mut Chart, env: &JudgeEnv, tau: f64, views: &[LineView], effects: &mut Vec<JudgeEffect>) {
        let spd = env.speed;
        let mut resolved = Vec::new();
        let mut scheduled = Vec::new();
        for view in views {
            let line = &mut chart.lines[view.line as usize];
            let (idx, st) = &self.notes[view.line as usize];
            for id in &idx[*st..] {
                let note = &mut line.notes[*id as usize];
                if let NoteKind::Hold { .. } = &note.kind {
                    if let JudgeStatus::Hold(.., pre_judge, up_time) = &mut note.judge {
                        if *pre_judge {
                            continue;
                        }
                        let x = {
                            note.object.translation.0.set_time(tau);
                            note.object.translation.0.now()
                        };
                        if self.key_down_count == 0
                            && !view
                                .touches
                                .iter()
                                .any(|it| it.is_some_and(|it| (it.x - x).abs() as f64 / note.judge_area as f64 <= CLICK_WIDTH))
                        {
                            if tau > *up_time + UP_TOLERANCE {
                                note.judge = JudgeStatus::Judged;
                                // The head of this hold was hit; the player
                                // let go too early. Flagged so it can be told
                                // apart from a hold that was never touched.
                                resolved.push(Resolved {
                                    line: view.line,
                                    note: *id,
                                    judgement: Judgement::Miss,
                                    diff: None,
                                    hold_broken: true,
                                });
                            } else if up_time.is_infinite() {
                                *up_time = tau;
                            }
                        } else {
                            *up_time = f64::INFINITY;
                        }
                        continue;
                    }
                }
                if !matches!(note.judge, JudgeStatus::NotJudged) {
                    continue;
                }
                let dt = (tau - note.time) / spd;
                if -dt > LIMIT_BAD {
                    break;
                }
                // drags are touched into place by a finger; flicks only get
                // this fallback when a key stands in for the finger
                let window = match note.kind {
                    NoteKind::Drag => LIMIT_DRAG,
                    NoteKind::Flick if self.key_down_count != 0 => LIMIT_FLICK,
                    _ => continue,
                };
                if dt.abs() > window {
                    continue;
                }
                let x = {
                    note.object.translation.0.set_time(tau);
                    note.object.translation.0.now()
                };
                if self.key_down_count != 0
                    || view
                        .touches
                        .iter()
                        .any(|it| it.is_some_and(|it| (it.x - x).abs() as f64 / note.judge_area as f64 <= CONTACT_WIDTH))
                {
                    note.judge = JudgeStatus::PreJudge;
                    scheduled.push((view.line, *id, note.time));
                }
            }
        }
        for (line, note, time) in scheduled {
            self.push_sched(tau, line, note, time);
        }
        self.flush(chart, env, tau, resolved, effects);
    }

    /// Commits pre-judged notes once their time has come, and silently
    /// resolves clicks that stayed pre-judged (an early bad) past their
    /// window.
    fn pass_pre_judged(&mut self, chart: &mut Chart, env: &JudgeEnv, tau: f64, views: &[LineView], effects: &mut Vec<JudgeEffect>) {
        let mut resolved = Vec::new();
        for view in views {
            let line = &mut chart.lines[view.line as usize];
            let (idx, st) = &self.notes[view.line as usize];
            for id in &idx[*st..] {
                let note = &mut line.notes[*id as usize];
                if let JudgeStatus::Hold(perfect, .., diff, true, _) = note.judge {
                    if let NoteKind::Hold { end_time, .. } = &note.kind {
                        if *end_time <= tau {
                            note.judge = JudgeStatus::Judged;
                            resolved.push(Resolved {
                                line: view.line,
                                note: *id,
                                judgement: if perfect { Judgement::Perfect } else { Judgement::Good },
                                diff: Some(diff),
                                hold_broken: false,
                            });
                            continue;
                        }
                    }
                }
                // TODO adjust
                let ghost_t = tau + LIMIT_GOOD;
                if matches!(note.kind, NoteKind::Click) {
                    if ghost_t < note.time {
                        break;
                    }
                } else if tau < note.time {
                    continue;
                }
                if matches!(note.judge, JudgeStatus::PreJudge) {
                    let diff = if let JudgeStatus::Hold(.., diff, _, _) = note.judge {
                        Some(diff)
                    } else {
                        None
                    };
                    note.judge = JudgeStatus::Judged;
                    if !matches!(note.kind, NoteKind::Click) {
                        resolved.push(Resolved {
                            line: view.line,
                            note: *id,
                            judgement: Judgement::Perfect,
                            diff,
                            hold_broken: false,
                        });
                    }
                }
            }
        }
        self.flush(chart, env, tau, resolved, effects);
    }

    /// A touch press judged as a click at exactly `tau`: the candidates —
    /// every note still click-matchable within
    /// the window and the lateral reach — are walked in global time order,
    /// so the earliest acceptable note wins unless a simultaneous one sits
    /// closer (`dt >= best + 10ms` skips the rest), or it is already a Good
    /// away and a later one is nearer in time. That ordering is what
    /// makes a drag eat an early press aimed at a later tap (once), and a
    /// click on a flick harmless.
    fn click(&mut self, chart: &mut Chart, env: &JudgeEnv, tau: f64, slot: u8, views: &[LineView], effects: &mut Vec<JudgeEffect>) {
        let spd = env.speed;
        let Some(ti) = self.live.iter().position(|it| it.0 == slot) else {
            return;
        };
        #[derive(Clone, Copy)]
        struct Cand {
            time: f64,
            line: u32,
            note: u32,
            dt: f64,
            dx: f64,
            weighted: f64,
            timed: bool,
        }
        let mut cands: Vec<Cand> = Vec::new();
        for view in views {
            let Some(pos) = view.touches[ti] else {
                continue;
            };
            let line = &mut chart.lines[view.line as usize];
            let (idx, st) = &self.notes[view.line as usize];
            for id in &idx[*st..] {
                let note = &mut line.notes[*id as usize];
                let dt = (note.time - tau) / spd;
                if dt <= -LIMIT_GOOD || dt >= LIMIT_BAD {
                    continue;
                }
                // still click-matchable: taps and holds until judged (a bad'd
                // tap keeps falling but is spent), drags until judged or
                // clicked, flicks until judged however often they are clicked
                let spent = match note.kind {
                    NoteKind::Click | NoteKind::Hold { .. } => !matches!(note.judge, JudgeStatus::NotJudged),
                    NoteKind::Drag => matches!(note.judge, JudgeStatus::Judged) || self.consumed.contains(&(view.line, *id)),
                    NoteKind::Flick => matches!(note.judge, JudgeStatus::Judged),
                };
                if spent {
                    continue;
                }
                let x = &mut note.object.translation.0;
                x.set_time(tau);
                let dx = (x.now() - pos.x).abs() as f64 / note.judge_area as f64;
                if dx >= CLICK_WIDTH {
                    continue;
                }
                cands.push(Cand {
                    time: note.time,
                    line: view.line,
                    note: *id,
                    dt,
                    dx,
                    weighted: dx + pos.y.abs() as f64 / 2.2,
                    timed: matches!(note.kind, NoteKind::Click | NoteKind::Hold { .. }),
                });
            }
        }
        cands.sort_by(|a, b| a.time.total_cmp(&b.time).then(a.line.cmp(&b.line)).then(a.note.cmp(&b.note)));
        let mut best: Option<Cand> = None;
        let mut best_abs_dt = f64::INFINITY;
        for cand in &cands {
            if cand.dt >= best_abs_dt + SAME_TIME {
                continue;
            }
            // the bad window shrinks towards the lateral edge
            let bad_limit = LIMIT_BAD - (cand.dx - 0.9 * WU).max(0.) / WU * LIMIT_PERFECT * 0.5;
            if cand.dt > bad_limit {
                continue;
            }
            let replace = match best {
                None => true,
                // a tap or a hold is never displaced by a drag or a flick
                Some(best) if !best.timed => true,
                Some(_) if !cand.timed => false,
                Some(best) if (best.time - cand.time).abs() / spd <= SAME_TIME => cand.weighted < best.weighted,
                // the earlier note keeps its claim only while the press would
                // still be a Perfect on it; past that, a note closer in time
                // wins. Otherwise one note left behind in a stream takes the
                // next press as a late Good, leaving its own note behind for
                // the press after, and so on down the whole stream
                Some(best) => best.dt.abs() >= LIMIT_PERFECT && cand.dt.abs() < best.dt.abs(),
            };
            if replace {
                best_abs_dt = cand.dt.abs();
                best = Some(*cand);
            }
        }
        let Some(best) = best else {
            return;
        };
        let note = &mut chart.lines[best.line as usize].notes[best.note as usize];
        match note.kind {
            NoteKind::Flick => {
                // the press is spent, and the flick is none the worse
                debug!("click rejected by flick");
            }
            NoteKind::Drag => {
                // the once-only protection: this drag eats the press
                debug!("click consumed by drag");
                self.consumed.insert((best.line, best.note));
            }
            NoteKind::Click => {
                let dt = best.dt.abs();
                if dt < LIMIT_GOOD {
                    note.judge = JudgeStatus::Judged;
                    self.flush_one(
                        chart,
                        env,
                        tau,
                        Resolved {
                            line: best.line,
                            note: best.note,
                            judgement: if dt < LIMIT_PERFECT { Judgement::Perfect } else { Judgement::Good },
                            diff: Some(tau),
                            hold_broken: false,
                        },
                        effects,
                    );
                } else {
                    // early only: the note keeps falling, but is spent
                    note.judge = JudgeStatus::PreJudge;
                    self.consumed.insert((best.line, best.note));
                    self.flush_one(
                        chart,
                        env,
                        tau,
                        Resolved {
                            line: best.line,
                            note: best.note,
                            judgement: Judgement::Bad,
                            diff: Some(tau),
                            hold_broken: false,
                        },
                        effects,
                    );
                }
            }
            NoteKind::Hold { end_time, .. } => {
                let perfect = best.dt.abs() < LIMIT_PERFECT;
                self.judgements.borrow_mut().push((tau, best.line, best.note, Err(perfect)));
                self.push_sched(tau, best.line, best.note, end_time);
                note.judge = JudgeStatus::Hold(perfect, tau, tau, false, f64::INFINITY);
                effects.push(JudgeEffect {
                    time: tau,
                    line: best.line,
                    note: best.note,
                    kind: JudgeEffectKind::HoldHead,
                });
            }
        }
    }

    /// One swipe step judged at exactly `tau`: an
    /// armed gesture claims the earliest unjudged flick within reach of the
    /// step's segment — the segment test is what lets a very fast swipe be
    /// seen even when it crosses the whole judgement band between two
    /// samples (#863) — and one gesture judges at most one flick.
    fn swipe(&mut self, chart: &mut Chart, env: &JudgeEnv, tau: f64, step: &SwipeStep, views: &[LineView], effects: &mut Vec<JudgeEffect>) {
        let SwipeStep { id: slot, x0, y0, x1, y1 } = *step;
        if !self.armed.contains(&slot) {
            return;
        }
        let spd = env.speed;
        let to_local = |inv: &Matrix, x: f32, y: f32| inv.transform_point(&Point::new(if env.flip_x { -x } else { x }, -(y / env.aspect_ratio)));
        #[derive(Clone, Copy)]
        struct Cand {
            time: f64,
            line: u32,
            note: u32,
            dt: f64,
            weighted: f64,
        }
        let mut cands: Vec<Cand> = Vec::new();
        for view in views {
            let p0 = to_local(&view.inv, x0, y0);
            let p1 = to_local(&view.inv, x1, y1);
            fn ok(f: f32) -> bool {
                matches!(f.classify(), FpCategory::Zero | FpCategory::Subnormal | FpCategory::Normal)
            }
            if !ok(p0.x) || !ok(p0.y) || !ok(p1.x) || !ok(p1.y) {
                continue;
            }
            let line = &mut chart.lines[view.line as usize];
            let (idx, st) = &self.notes[view.line as usize];
            for id in &idx[*st..] {
                let note = &mut line.notes[*id as usize];
                if !matches!(note.kind, NoteKind::Flick) || !matches!(note.judge, JudgeStatus::NotJudged) {
                    continue;
                }
                let dt = (note.time - tau) / spd;
                if dt.abs() >= LIMIT_FLICK {
                    continue;
                }
                let x = &mut note.object.translation.0;
                x.set_time(tau);
                let x = x.now();
                // the segment's x extent against the note's vertical band:
                // full contact if it spans the note, else the nearer end
                let lo = p0.x.min(p1.x);
                let hi = p0.x.max(p1.x);
                let (dx, dy) = if x >= lo && x <= hi {
                    // the point of the segment above the note
                    let r = if p1.x == p0.x { 0.5 } else { ((x - p0.x) / (p1.x - p0.x)).clamp(0., 1.) };
                    (0., (p0.y + (p1.y - p0.y) * r).abs() as f64)
                } else {
                    let nearer = if (x - p0.x).abs() < (x - p1.x).abs() { p0 } else { p1 };
                    ((x - nearer.x).abs() as f64, nearer.y.abs() as f64)
                };
                let dx = dx / note.judge_area as f64;
                if dx >= CONTACT_WIDTH {
                    continue;
                }
                cands.push(Cand {
                    time: note.time,
                    line: view.line,
                    note: *id,
                    dt,
                    weighted: dx + dy / 2.2,
                });
            }
        }
        cands.sort_by(|a, b| a.time.total_cmp(&b.time).then(a.line.cmp(&b.line)).then(a.note.cmp(&b.note)));
        let mut best: Option<Cand> = None;
        let mut best_abs_dt = f64::INFINITY;
        for cand in &cands {
            if cand.dt >= best_abs_dt + SAME_TIME {
                continue;
            }
            let replace = match best {
                None => true,
                Some(best) => (best.time - cand.time).abs() / spd <= SAME_TIME && cand.weighted < best.weighted,
            };
            if replace {
                best_abs_dt = cand.dt.abs();
                best = Some(*cand);
            }
        }
        let Some(best) = best else {
            return;
        };
        self.armed.remove(&slot);
        let note = &mut chart.lines[best.line as usize].notes[best.note as usize];
        if best.time <= tau {
            note.judge = JudgeStatus::Judged;
            self.flush_one(
                chart,
                env,
                tau,
                Resolved {
                    line: best.line,
                    note: best.note,
                    judgement: Judgement::Perfect,
                    diff: None,
                    hold_broken: false,
                },
                effects,
            );
        } else {
            note.judge = JudgeStatus::PreJudge;
            self.push_sched(tau, best.line, best.note, best.time);
        }
    }

    /// One keyboard hit at the frame's time: the earliest click or hold
    /// note not yet judged, regardless of position. Keyboard plays are
    /// never recorded, so this only has to be reasonable, not faithful.
    fn keyboard_hit(&mut self, chart: &mut Chart, env: &JudgeEnv, t: f64, effects: &mut Vec<JudgeEffect>) {
        let spd = env.speed;
        let Some((line_id, id)) = chart
            .lines
            .iter()
            .zip(self.notes.iter())
            .enumerate()
            .filter_map(|(line_id, (line, (idx, st)))| {
                idx[*st..]
                    .iter()
                    .cloned()
                    .find(|id| {
                        let note = &line.notes[*id as usize];
                        matches!(note.judge, JudgeStatus::NotJudged) && matches!(note.kind, NoteKind::Click | NoteKind::Hold { .. })
                    })
                    .map(|id| (line_id as u32, id))
            })
            .min_by_key(|(line_id, id)| chart.lines[*line_id as usize].notes[*id as usize].time.not_nan())
        else {
            return;
        };
        enum Pressed {
            Hit(Judgement),
            HoldHead,
        }
        let outcome = {
            let note = &mut chart.lines[line_id as usize].notes[id as usize];
            let dt = (t - note.time).abs() / spd;
            if dt >= LIMIT_GOOD {
                return;
            }
            match note.kind {
                NoteKind::Click => {
                    note.judge = JudgeStatus::Judged;
                    Pressed::Hit(if dt < LIMIT_PERFECT { Judgement::Perfect } else { Judgement::Good })
                }
                NoteKind::Hold { end_time, .. } => {
                    let perfect = dt < LIMIT_PERFECT;
                    self.judgements.borrow_mut().push((t, line_id, id, Err(perfect)));
                    self.push_sched(t, line_id, id, end_time);
                    note.judge = JudgeStatus::Hold(perfect, t, t, false, f64::INFINITY);
                    Pressed::HoldHead
                }
                _ => unreachable!(),
            }
        };
        match outcome {
            Pressed::HoldHead => {
                effects.push(JudgeEffect {
                    time: t,
                    line: line_id,
                    note: id,
                    kind: JudgeEffectKind::HoldHead,
                });
            }
            Pressed::Hit(judgement) => {
                self.flush_one(
                    chart,
                    env,
                    t,
                    Resolved {
                        line: line_id,
                        note: id,
                        judgement,
                        diff: Some(t),
                        hold_broken: false,
                    },
                    effects,
                );
            }
        }
    }

    /// Turns the core's effects into particles, sounds and bad notes, with
    /// everything seeked back to the moment each effect happened.
    fn apply_effects(&mut self, res: &mut Resource, chart: &mut Chart, bad_notes: &mut Vec<BadNote>, effects: Vec<JudgeEffect>) {
        for effect in effects {
            let (line_id, note_id, tau) = (effect.line, effect.note, effect.time);
            Self::seek_chain(chart, line_id, tau);
            chart.lines[line_id as usize].notes[note_id as usize].object.set_time(tau);
            match effect.kind {
                JudgeEffectKind::HoldHead => {
                    chart.lines[line_id as usize].notes[note_id as usize].hitsound.play(res);
                }
                JudgeEffectKind::Commit(judgement) => {
                    if matches!(chart.lines[line_id as usize].notes[note_id as usize].kind, NoteKind::Hold { .. }) {
                        continue;
                    }
                    let line = &chart.lines[line_id as usize];
                    let line_tr = line.now_transform_with(res.aspect_ratio, &chart.lines);
                    let note = &line.notes[note_id as usize];
                    match judgement {
                        Judgement::Perfect => {
                            res.with_model(line_tr * note.object.now(res), |res| {
                                res.emit_at_origin(note.rotation(line), note.fx_color.unwrap_or_else(|| res.res_pack.info.fx_perfect()))
                            });
                            note.hitsound.play(res);
                        }
                        Judgement::Good => {
                            res.with_model(line_tr * note.object.now(res), |res| {
                                res.emit_at_origin(note.rotation(line), note.fx_color.unwrap_or_else(|| res.res_pack.info.fx_good()))
                            });
                            note.hitsound.play(res);
                        }
                        Judgement::Bad => {
                            bad_notes.push(BadNote {
                                time: tau,
                                kind: note.kind.clone(),
                                matrix: {
                                    let mut mat = line_tr;
                                    if !note.above {
                                        mat.append_nonuniform_scaling_mut(&Vector::new(1., -1.));
                                    }
                                    let incline_sin = line.incline.now_opt().map(|it| it.to_radians().sin()).unwrap_or_default();
                                    mat *= note.now_transform(
                                        res,
                                        &line.ctrl_obj.borrow_mut(),
                                        ((note.height - line.height.now() as f64) / res.aspect_ratio as f64 * note.speed) as f32,
                                        incline_sin,
                                    );
                                    mat
                                },
                            });
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    /// Seek a line's whole ancestor chain — object, height and incline — to
    /// `tau`.
    fn seek_chain(chart: &mut Chart, line_id: u32, tau: f64) {
        let mut chain = Vec::new();
        let mut cur = Some(line_id as usize);
        while let Some(i) = cur {
            chain.push(i);
            cur = chart.lines[i].parent;
        }
        for i in chain {
            let line = &mut chart.lines[i];
            line.object.set_time(tau);
            line.height.set_time(tau);
            line.incline.set_time(tau);
        }
    }

    fn push_sched(&mut self, tau: f64, line: u32, note: u32, time: f64) {
        if time.is_finite() && time > tau {
            self.sched.push(Reverse(Sched { time, line, note }));
        }
    }

    /// The next scheduled point that is still live: its note must still be
    /// in the state that scheduled it, and its time not yet passed.
    fn peek_sched(&mut self, chart: &Chart) -> Option<Sched> {
        while let Some(&Reverse(s)) = self.sched.peek() {
            let valid = s.time > self.judged_until && {
                let note = &chart.lines[s.line as usize].notes[s.note as usize];
                match &note.judge {
                    JudgeStatus::PreJudge => !matches!(note.kind, NoteKind::Click | NoteKind::Hold { .. }),
                    JudgeStatus::Hold(..) => true,
                    _ => false,
                }
            };
            if valid {
                return Some(s);
            }
            self.sched.pop();
        }
        None
    }

    fn advance_st(&mut self, chart: &Chart) {
        for (line, (idx, st)) in chart.lines.iter().zip(self.notes.iter_mut()) {
            while idx
                .get(*st)
                .is_some_and(|&id| matches!(line.notes[id as usize].judge, JudgeStatus::Judged))
            {
                *st += 1;
            }
        }
    }

    fn flush(&mut self, chart: &Chart, env: &JudgeEnv, tau: f64, resolved: Vec<Resolved>, effects: &mut Vec<JudgeEffect>) {
        for it in resolved {
            self.flush_one(chart, env, tau, it, effects);
        }
    }

    fn flush_one(&mut self, chart: &Chart, env: &JudgeEnv, tau: f64, resolved: Resolved, effects: &mut Vec<JudgeEffect>) {
        let Resolved {
            line,
            note: id,
            judgement,
            diff,
            hold_broken,
        } = resolved;
        let note = &chart.lines[line as usize].notes[id as usize];
        // A miss has no observed hit time, and drags and flicks are judged
        // by contact rather than by when it happened.
        let error = if matches!(judgement, Judgement::Miss) || matches!(note.kind, NoteKind::Drag | NoteKind::Flick) {
            None
        } else {
            Some((diff.unwrap_or(tau) - note.time) / env.speed)
        };
        self.hit_events.push(HitEvent {
            line_id: line,
            note_id: id,
            judgement,
            diff: error,
            hold_broken,
        });
        // `inner` keeps its own sentinel convention: misses count as very
        // late, and untimed notes as exactly on time.
        self.commit(
            tau,
            judgement,
            line,
            id,
            match error {
                _ if matches!(judgement, Judgement::Miss) => 0.25,
                Some(error) => error,
                None => 0.,
            },
        );
        effects.push(JudgeEffect {
            time: tau,
            line,
            note: id,
            kind: JudgeEffectKind::Commit(judgement),
        });
    }

    fn log(&mut self, event: InputEvent) {
        if !self.replaying {
            self.inputs.push(event);
        }
    }

    /// Re-judges a recorded input stream against a chart. The returned
    /// judge holds the reproduced `hit_events` and result — everything but
    /// keyboard hits, which the stream does not carry. Flicks replay through
    /// their recorded swipe steps; only their *detection* is taken on trust
    /// from the client that recorded them.
    pub fn replay(chart: &mut Chart, env: &JudgeEnv, inputs: &[InputEvent], end: f64) -> Judge {
        let mut judge = Judge::new(chart);
        judge.replaying = true;
        let mut source = ReplaySource::new(inputs);
        let mut effects = Vec::new();
        let _ = judge.advance(chart, env, &mut source, end, &mut effects, false);
        judge
    }

    /// Marks this judge as replaying a recorded stream, so that `log`
    /// records nothing. `reset` deliberately leaves the flag alone: a
    /// replayed run stays one across the resets of seeking and retrying.
    pub fn set_replaying(&mut self, on: bool) {
        self.replaying = on;
    }

    /// One frame of a replay: re-judges the recorded stream up to the
    /// current chart time — the counterpart of [`Judge::update`] — and
    /// applies the effects the same way. Returns the target of the recorded
    /// seek the replay just reached, if any: the caller must jump its clock
    /// and music there before the next frame, as the original run's pause
    /// did.
    pub fn replay_update(&mut self, res: &mut Resource, chart: &mut Chart, bad_notes: &mut Vec<BadNote>, player: &mut ReplayPlayer) -> Option<f64> {
        let mut effects = Vec::new();
        let seek = self.replay_update_with(chart, player, res.time, &mut effects);
        self.apply_effects(res, chart, bad_notes, effects);
        seek
    }

    /// One frame of replay judging, free of anything macroquad-global: the
    /// entry the tests drive directly, mirroring [`Judge::update_with`].
    fn replay_update_with(&mut self, chart: &mut Chart, player: &mut ReplayPlayer, t: f64, effects: &mut Vec<JudgeEffect>) -> Option<f64> {
        // The cursor lives in the player between frames; each frame builds
        // its source from it and writes back where it got to.
        let mut source = ReplaySource::with_cursor(&player.record.inputs, player.cursor);
        let seek = self.advance(chart, &player.env, &mut source, t, effects, true);
        player.cursor = source.cursor();
        self.last_time = t / player.env.speed;
        seek
    }

    /// Jumps a replay to chart time `t` — a drag of the progress bar.
    /// Everything judged so far is undone and the stream re-judged from the
    /// top, stopping inside the first segment that contains `t` (the seeks
    /// recorded in the stream apply as they are reached, which lands there
    /// by construction). The skipped-over effects are dropped: nothing
    /// happened on screen at times the play jumped past. Resetting the
    /// chart is the caller's to do.
    pub fn replay_seek(&mut self, chart: &mut Chart, player: &mut ReplayPlayer, t: f64) {
        self.reset();
        player.restart();
        let mut source = ReplaySource::new(&player.record.inputs);
        let mut effects = Vec::new();
        let _ = self.advance(chart, &player.env, &mut source, t, &mut effects, false);
        player.cursor = source.cursor();
        self.last_time = t / player.env.speed;
    }

    /// The touches the judge currently holds, in the stream's coordinate
    /// space — slot, x, and y scaled by the aspect ratio. A replay draws
    /// them to show the recorded run's fingers where they were.
    pub fn live_touches(&self) -> impl Iterator<Item = (u8, f32, f32)> + '_ {
        self.live.iter().copied()
    }

    /// [`Self::live_touches`] at chart time `t` of a replay, each finger slid
    /// toward where the stream has it next. Recorded positions change only
    /// on the tick grid, which drawn as is looks choppy; this interpolates
    /// linearly over the tick interval leading up to the next recorded move.
    /// For display only — judging never sees it.
    pub fn replay_touches_at(&self, player: &ReplayPlayer, t: f64) -> Vec<(u8, f32, f32)> {
        let step = 1. / JUDGE_TICK_HZ;
        let ahead = player.record.inputs.get(player.cursor..).unwrap_or_default();
        let next_move = |id: u8| {
            for event in ahead {
                let time = match event {
                    InputEvent::Start { time, .. }
                    | InputEvent::Appear { time, .. }
                    | InputEvent::End { time, .. }
                    | InputEvent::Swipe { time, .. } => *time,
                    InputEvent::Move { tick, .. } => *tick as f64 / JUDGE_TICK_HZ,
                    InputEvent::Seek { from, .. } => *from,
                };
                if time > t + step {
                    break;
                }
                match event {
                    InputEvent::Move { points, .. } => {
                        if let Some(point) = points.iter().find(|it| it.id == id) {
                            return Some((time, point.x, point.y));
                        }
                    }
                    // The finger lifts or the timeline jumps first: nothing
                    // to slide toward.
                    InputEvent::Start { id: other, .. } | InputEvent::Appear { id: other, .. } | InputEvent::End { id: other, .. }
                        if *other == id =>
                    {
                        break
                    }
                    InputEvent::Seek { .. } => break,
                    _ => {}
                }
            }
            None
        };
        self.live
            .iter()
            .map(|&(id, x, y)| match next_move(id) {
                Some((time, nx, ny)) => {
                    let f = ((t - (time - step)) / step).clamp(0., 1.) as f32;
                    (id, x + (nx - x) * f, y + (ny - y) * f)
                }
                None => (id, x, y),
            })
            .collect()
    }

    fn auto_play_update(&mut self, res: &mut Resource, chart: &mut Chart) {
        let t = res.time;
        let mut judgements = Vec::new();
        for (line_id, (line, (idx, st))) in chart.lines.iter_mut().zip(self.notes.iter_mut()).enumerate() {
            for id in &idx[*st..] {
                let note = &mut line.notes[*id as usize];
                if let JudgeStatus::Hold(..) = note.judge {
                    if let NoteKind::Hold { end_time, .. } = note.kind {
                        if t >= end_time {
                            note.judge = JudgeStatus::Judged;
                            judgements.push((line_id, *id));
                            continue;
                        }
                    }
                }
                if !matches!(note.judge, JudgeStatus::NotJudged) {
                    continue;
                }
                if note.time > t {
                    break;
                }
                note.judge = if matches!(note.kind, NoteKind::Hold { .. }) {
                    note.hitsound.play(res);
                    self.judgements.borrow_mut().push((t, line_id as _, *id, Err(true)));
                    JudgeStatus::Hold(true, t, t, false, f64::INFINITY)
                } else {
                    judgements.push((line_id, *id));
                    JudgeStatus::Judged
                };
            }
            while idx
                .get(*st)
                .is_some_and(|id| matches!(line.notes[*id as usize].judge, JudgeStatus::Judged))
            {
                *st += 1;
            }
        }
        for (line_id, id) in judgements.into_iter() {
            self.commit(t, Judgement::Perfect, line_id as _, id, 0.);
            let (note_transform, note_hitsound) = {
                let line = &mut chart.lines[line_id];
                let note = &mut line.notes[id as usize];
                let nt = if matches!(note.kind, NoteKind::Hold { .. }) { t } else { note.time };
                line.object.set_time(nt);
                note.object.set_time(nt);
                (note.object.now(res), note.hitsound.clone())
            };
            let line = &chart.lines[line_id];
            res.with_model(line.now_transform(res, &chart.lines) * note_transform, |res| {
                res.emit_at_origin(line.notes[id as usize].rotation(line), res.res_pack.info.fx_perfect())
            });
            if !matches!(chart.lines[line_id].notes[id as usize].kind, NoteKind::Hold { .. }) {
                note_hitsound.play(res);
            }
        }
    }

    #[inline]
    pub fn result(&self) -> PlayResult {
        self.inner.result()
    }

    #[inline]
    pub fn combo(&self) -> u32 {
        self.inner.combo()
    }

    #[inline]
    pub fn counts(&self) -> [u32; 4] {
        self.inner.counts()
    }
}

/// A recorded play being played back frame by frame: owns the stream cursor
/// between frames, and the [`JudgeEnv`] the stream was recorded under — the
/// one it must be replayed under. [`Judge::replay_update`] advances through
/// it; [`Judge::replay_seek`] rewinds it to the top.
pub struct ReplayPlayer {
    record: Arc<PlayRecord>,
    cursor: usize,
    env: JudgeEnv,
}

impl ReplayPlayer {
    pub fn new(record: Arc<PlayRecord>) -> Self {
        let meta = &record.meta;
        Self {
            env: JudgeEnv {
                aspect_ratio: meta.aspect_ratio,
                speed: meta.speed as f64,
                flip_x: meta.mods.contains(Mods::FLIP_X),
            },
            record,
            cursor: 0,
        }
    }

    /// The record being played back.
    pub fn record(&self) -> &Arc<PlayRecord> {
        &self.record
    }

    /// The env the stream was recorded under.
    pub fn env(&self) -> &JudgeEnv {
        &self.env
    }

    /// Back to the top of the stream, for playing the record again.
    pub fn restart(&mut self) {
        self.cursor = 0;
    }
}

struct Handler {
    status: TouchStatus,
    wheel: (f32, f32),
}
impl Handler {
    fn finalize(&mut self) {
        if is_mouse_button_down(MouseButton::Left) {
            self.status.touches.push(Touch {
                id: button_to_id(MouseButton::Left),
                phase: TouchPhase::Moved,
                position: mouse_position().into(),
                time: f64::NEG_INFINITY,
            });
        }
    }
}

fn button_to_id(button: MouseButton) -> u64 {
    u64::MAX
        - match button {
            MouseButton::Left => 0,
            MouseButton::Middle => 1,
            MouseButton::Right => 2,
            MouseButton::Unknown => 3,
        }
}

impl EventHandler for Handler {
    fn update(&mut self, _: &mut miniquad::Context) {}
    fn draw(&mut self, _: &mut miniquad::Context) {}
    fn touch_event(&mut self, _: &mut miniquad::Context, phase: miniquad::TouchPhase, id: u64, x: f32, y: f32, time: f64) {
        self.status.touches.push(Touch {
            id,
            phase: phase.into(),
            position: vec2(x, y),
            time,
        });
    }

    fn mouse_wheel_event(&mut self, _ctx: &mut miniquad::Context, x: f32, y: f32) {
        self.wheel.0 += x;
        self.wheel.1 += y;
    }

    fn mouse_button_down_event(&mut self, _ctx: &mut miniquad::Context, button: MouseButton, x: f32, y: f32) {
        self.status.touches.push(Touch {
            id: button_to_id(button),
            phase: TouchPhase::Started,
            position: vec2(x, y),
            time: f64::NEG_INFINITY,
        });
    }

    fn mouse_button_up_event(&mut self, _ctx: &mut miniquad::Context, button: MouseButton, x: f32, y: f32) {
        self.status.touches.push(Touch {
            id: button_to_id(button),
            phase: TouchPhase::Ended,
            position: vec2(x, y),
            time: f64::NEG_INFINITY,
        });
    }

    fn key_down_event(&mut self, _ctx: &mut miniquad::Context, _keycode: KeyCode, _keymods: miniquad::KeyMods, repeat: bool) {
        if !repeat {
            self.status.key_delta += 1;
            self.status.keys_down += 1;
        }
    }

    fn key_up_event(&mut self, _ctx: &mut miniquad::Context, _keycode: KeyCode, _keymods: miniquad::KeyMods) {
        self.status.key_delta -= 1;
    }
}

#[derive(Default)]
pub struct PlayResult {
    pub score: u32,
    pub accuracy: f64,
    pub max_combo: u32,
    pub num_of_notes: u32,
    pub counts: [u32; 4],
    pub early: u32,
    pub late: u32,
    pub std: f32,
    pub early_kind: [u32; 4],
    pub late_kind: [u32; 4],
}

pub fn icon_index(score: u32, full_combo: bool) -> usize {
    match (score, full_combo) {
        (x, _) if x < 700000 => 0,
        (x, _) if x < 820000 => 1,
        (x, _) if x < 880000 => 2,
        (x, _) if x < 920000 => 3,
        (x, _) if x < 960000 => 4,
        (1000000, _) => 7,
        (_, false) => 5,
        (_, true) => 6,
    }
}
