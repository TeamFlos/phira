//! The canonical input stream the judge consumes.
//!
//! Judgement results depend on nothing but this stream: live play turns raw
//! OS input into [`InputEvent`]s through [`InputCollector`], judges them, and
//! appends the very same events to [`Judge::inputs`](crate::judge::Judge::inputs);
//! a replay feeds the recorded events back through the same `advance` code
//! path (via [`ReplaySource`]) and reproduces the judgements note for note.
//! Keyboard hits are the one exception — they never enter the stream. Flick
//! *judgement* is reproducible through the recorded [`InputEvent::Swipe`]
//! steps; how those steps were detected, however, is something only the
//! client that produced them can vouch for.
//!
//! # Time base
//!
//! Times are chart seconds, as `f64` so that ordering against the fixed tick
//! grid ([`JUDGE_TICK_HZ`](crate::judge::JUDGE_TICK_HZ)) is never ambiguous.
//! The stream is monotonic within a segment; a [`InputEvent::Seek`] marks a
//! discontinuity where the clock jumped (pause with rewind, a keyboard seek).
//!
//! # Coordinate space
//!
//! The same space [`Judge::get_touches`](crate::judge::Judge::get_touches)
//! normalizes to, with the y axis scaled by the aspect ratio so that both
//! axes span `[-1, 1]`, and x never flipped: `(x, y * aspect_ratio)`. The
//! judge converts back per use: `x' = if flip_x { -x } else { x }`,
//! `y' = y / aspect_ratio` (f32 arithmetic, identically on both sides).
//!
//! # Ordering
//!
//! Events are processed in stream order, which is time order within each
//! segment. At equal times the judge core evaluates ticks and scheduled
//! points before input events.

use crate::judge::{JUDGE_TICK_HZ, MOVE_DEADBAND};
use macroquad::prelude::TouchPhase;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

/// One event of the canonical input stream. Everything the judge evaluates a
/// play against is here, at exactly the precision it was judged with.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "camelCase")]
pub enum InputEvent {
    /// A touch was pressed. Counts as a click at `time`.
    Start { id: u8, time: f64, x: f32, y: f32 },
    /// A touch first seen already pressed — it began before recording started
    /// or while the judge wasn't running. Occupies the finger but never
    /// counts as a click.
    Appear { id: u8, time: f64, x: f32, y: f32 },
    /// A touch was lifted (or reconciled away as gone).
    End { id: u8, time: f64 },
    /// Touches whose emitted position changed at tick `tick` (time
    /// `tick / JUDGE_TICK_HZ`). Omitted entirely when nothing moved beyond
    /// [`MOVE_DEADBAND`].
    Move { tick: u32, points: Vec<MovePoint> },
    /// One step of a swipe gesture, from `(x0, y0)` to `(x1, y1)` in the
    /// stream's space, sampled on the fixed 60 Hz grid (`time`). `new` marks
    /// the first step of a gesture; the steps of one gesture judge at most
    /// one flick. Detected live by the swipe trigger; a
    /// replay trusts the recorded steps.
    Swipe {
        id: u8,
        time: f64,
        new: bool,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
    },
    /// The clock jumped from `from` to `to` (pause with rewind, a keyboard
    /// seek). Everything up to and including `from` is processed first, then
    /// the clock becomes `to`.
    Seek { from: f64, to: f64 },
}

/// A touch position sampled on the tick grid, in the stream's coordinate
/// space.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MovePoint {
    pub id: u8,
    pub x: f32,
    pub y: f32,
}

/// Where the next event of an input stream lies, and how to consume it.
/// Implemented by the live [`InputCollector`] and by [`ReplaySource`].
pub(crate) trait InputSource {
    /// Whether the next event is a [`InputEvent::Seek`]. A seek rewinds the
    /// clock rather than evaluating a point, so it is applied as soon as it
    /// reaches the head of the stream, `until` notwithstanding.
    fn peek_is_seek(&self) -> bool;

    /// Time of the next Start/Appear/End/Seek, ignoring any Move events (a
    /// Seek reports its `from`). `None` when the stream is exhausted.
    fn peek_time(&self) -> Option<f64>;

    /// Consumes and returns the next non-Move event.
    fn pop(&mut self) -> Option<InputEvent>;

    /// Positions to apply at grid tick `tick`, consuming any Move event the
    /// stream holds for it.
    fn moves_at(&mut self, tick: u32) -> Vec<MovePoint>;
}

/// Feeds a recorded [`InputEvent`] slice back into the judge core. Moves are
/// consumed by tick through [`InputSource::moves_at`]; everything else in
/// stream order.
pub(crate) struct ReplaySource<'a> {
    events: &'a [InputEvent],
    cursor: usize,
}

impl<'a> ReplaySource<'a> {
    pub fn new(events: &'a [InputEvent]) -> Self {
        Self { events, cursor: 0 }
    }

    fn peek(&self) -> Option<&InputEvent> {
        self.events.get(self.cursor).filter(|it| !matches!(it, InputEvent::Move { .. }))
    }
}

impl InputSource for ReplaySource<'_> {
    fn peek_is_seek(&self) -> bool {
        matches!(self.peek(), Some(InputEvent::Seek { .. }))
    }

    fn peek_time(&self) -> Option<f64> {
        self.peek().map(|it| match it {
            InputEvent::Start { time, .. }
            | InputEvent::Appear { time, .. }
            | InputEvent::End { time, .. }
            | InputEvent::Swipe { time, .. }
            | InputEvent::Seek { from: time, .. } => *time,
            InputEvent::Move { .. } => unreachable!(),
        })
    }

    fn pop(&mut self) -> Option<InputEvent> {
        let event = self.peek().cloned()?;
        self.cursor += 1;
        Some(event)
    }

    fn moves_at(&mut self, tick: u32) -> Vec<MovePoint> {
        // A move left over from an earlier tick can only mean the live side
        // seeked between ticks; drop it rather than applying it late.
        while let Some(InputEvent::Move { tick: recorded, points }) = self.events.get(self.cursor) {
            if *recorded > tick {
                break;
            }
            self.cursor += 1;
            if *recorded == tick {
                return points.clone();
            }
        }
        Vec::new()
    }
}

/// A raw touch event of one frame, as the OS delivered it, with the position
/// already mapped into the stream's coordinate space. `time` is the OS
/// timestamp (monotonic uptime seconds); infinite means "none" (synthesized
/// mouse events), in which case the frame time is used instead.
pub(crate) struct RawTouch {
    pub id: u64,
    pub phase: TouchPhase,
    pub time: f64,
    pub x: f32,
    pub y: f32,
}

/// One frame of raw input, already in the stream's coordinate space, with
/// everything needed to canonicalize it.
pub(crate) struct RawFrame<'a> {
    /// The frame's chart time.
    pub t: f64,
    /// The uptime sampled once for this frame, converting OS timestamps.
    pub uptime: f64,
    /// The play's speed, part of that conversion.
    pub speed: f64,
    /// The viewport's width over height, for converting swipe displacements
    /// into screen-height units.
    pub aspect: f32,
    pub raw: &'a [RawTouch],
    /// The touches the OS reports as alive: id and position.
    pub live: &'a [(u64, f32, f32)],
}

/// Turns per-frame raw input into the canonical stream, live only. Owns the
/// slot allocation (OS touch id to `u8`), the reconciliation against the set
/// of touches the OS reports as alive, and the sampling of move positions
/// onto the tick grid.
#[derive(Default)]
pub struct InputCollector {
    /// OS touch id -> slot.
    map: HashMap<u64, u8>,
    /// Last position emitted into the stream, per slot. This — never a
    /// fresher sample — is what the judge's continuous checks see.
    last: HashMap<u8, (f32, f32)>,
    /// Position samples not yet attributed to a tick, per slot.
    pending: HashMap<u8, Vec<(f64, f32, f32)>>,
    /// Raw positions of every mapped slot, press included, for the swipe
    /// detector.
    trace: HashMap<u8, Vec<(f64, f32, f32)>>,
    /// Swipe detection state, per slot.
    swipes: HashMap<u8, SwipeState>,
    /// Next 60 Hz grid point per slot that the detector has not processed.
    swipe_next: HashMap<u8, u32>,
    /// Start/Appear/End/Swipe/Seek events waiting to be consumed by
    /// `advance`.
    queue: VecDeque<InputEvent>,
    /// Monotonic floor for event times: the maximum of `judged_until` and
    /// every time emitted into the queue so far.
    clock: f64,
}

/// Rate of the fixed grid the swipe detector samples positions on.
pub(crate) const SWIPE_HZ: f64 = 60.;
// TODO: use the real screen dpi (res.dpi) once it is reliable across
// platforms; record it in PlayMeta then.
const SWIPE_DPI: f64 = 275.;
/// The stillness threshold of the swipe trigger, in world units per
/// 1/60 s step.
const SWIPE_U: f64 = 0.06 / 380. * SWIPE_DPI;

/// Where one slot's swipe gesture currently stands.
struct SwipeState {
    stopped: bool,
    in_gesture: bool,
    /// Previous step's displacement, in world units per 1/60 s.
    d0: (f64, f64),
    /// Position of the last processed grid point.
    pos: (f32, f32),
    /// Time of the last processed grid point — the press itself for the
    /// first step, whose length is `g - press`, not 1/60.
    prev_t: f64,
}

impl SwipeState {
    fn new(time: f64, pos: (f32, f32)) -> Self {
        Self {
            stopped: true,
            in_gesture: false,
            d0: (0., 0.),
            pos,
            prev_t: time,
        }
    }
}

/// The first swipe grid point strictly after `t`.
fn swipe_grid_after(t: f64) -> u32 {
    let mut k = ((t * SWIPE_HZ).floor() + 1.).max(0.) as u32;
    while (k as f64) / SWIPE_HZ <= t {
        k += 1;
    }
    k
}

/// The traced position at `g`, linearly interpolated between the raw samples
/// around it; held at the ends when `g` is before the first or after the
/// last sample (a finger that stops reporting moves).
fn sample_at(samples: &[(f64, f32, f32)], g: f64) -> (f32, f32) {
    if g <= samples[0].0 {
        return (samples[0].1, samples[0].2);
    }
    for w in samples.windows(2) {
        if g <= w[1].0 {
            let (t0, x0, y0) = w[0];
            let (t1, x1, y1) = w[1];
            let r = ((g - t0) / (t1 - t0).max(1e-9)) as f32;
            return (x0 + (x1 - x0) * r, y0 + (y1 - y0) * r);
        }
    }
    let last = samples[samples.len() - 1];
    (last.1, last.2)
}

/// More than this many simultaneous touches cannot be represented and are
/// dropped.
const MAX_SLOTS: u32 = 128;

impl InputCollector {
    pub(crate) fn reset(&mut self) {
        self.map.clear();
        self.last.clear();
        self.pending.clear();
        self.trace.clear();
        self.swipes.clear();
        self.swipe_next.clear();
        self.queue.clear();
        self.clock = 0.;
    }

    fn alloc_slot(&self) -> Option<u8> {
        let used = self.map.values().fold(0_u128, |mask, id| mask | 1 << *id);
        let slot = (!used).trailing_zeros();
        (slot < MAX_SLOTS).then_some(slot as u8)
    }

    fn free_slot(&mut self, slot: u8) {
        self.last.remove(&slot);
        self.pending.remove(&slot);
        self.trace.remove(&slot);
        self.swipes.remove(&slot);
        self.swipe_next.remove(&slot);
    }

    /// Emits a Seek: everything queued but unconsumed belongs to the
    /// abandoned timeline and is dropped, surviving touches get their current
    /// position re-sampled at the first tick after the seek.
    pub(crate) fn push_seek(&mut self, from: f64, to: f64, live: &[(u64, f32, f32)]) {
        self.queue.clear();
        self.queue.push_back(InputEvent::Seek { from, to });
        self.pending.clear();
        for &(os, x, y) in live {
            if let Some(&slot) = self.map.get(&os) {
                self.pending.entry(slot).or_default().push((to, x, y));
                self.trace.insert(slot, vec![(to, x, y)]);
                self.swipes.insert(slot, SwipeState::new(to, (x, y)));
                self.swipe_next.insert(slot, swipe_grid_after(to));
            }
        }
        self.clock = to;
    }

    /// Processes one frame: converts the raw events, runs the swipe detector
    /// over the grid points this frame covers, reconciles against the
    /// touches reported alive, and appends the resulting canonical events to
    /// the queue in processing order. `aspect` is the viewport's width over
    /// height, for converting displacements into screen-height units.
    pub(crate) fn feed(&mut self, frame: RawFrame, judged_until: f64) {
        let RawFrame {
            t,
            uptime,
            speed,
            aspect,
            raw,
            live,
        } = frame;
        self.clock = self.clock.max(judged_until);
        let clock = self.clock;
        let mut frame: Vec<(f64, usize, InputEvent)> = Vec::new();
        let mut started: Vec<u64> = Vec::new();
        for (seq, touch) in raw.iter().enumerate() {
            let time = if touch.time.is_finite() {
                (t - (uptime - touch.time) * speed).max(clock)
            } else {
                t
            };
            match touch.phase {
                TouchPhase::Started => {
                    started.push(touch.id);
                    // The End of a previous life of this OS id was lost; close
                    // the slot before opening a new one.
                    if let Some(slot) = self.map.remove(&touch.id) {
                        frame.push((time, seq, InputEvent::End { id: slot, time }));
                        self.free_slot(slot);
                    }
                    if let Some(slot) = self.alloc_slot() {
                        self.map.insert(touch.id, slot);
                        self.last.insert(slot, (touch.x, touch.y));
                        self.trace.insert(slot, vec![(time, touch.x, touch.y)]);
                        self.swipes.insert(slot, SwipeState::new(time, (touch.x, touch.y)));
                        self.swipe_next.insert(slot, swipe_grid_after(time));
                        frame.push((
                            time,
                            seq,
                            InputEvent::Start {
                                id: slot,
                                time,
                                x: touch.x,
                                y: touch.y,
                            },
                        ));
                    }
                }
                TouchPhase::Ended | TouchPhase::Cancelled => {
                    if let Some(slot) = self.map.remove(&touch.id) {
                        frame.push((time, seq, InputEvent::End { id: slot, time }));
                        self.free_slot(slot);
                    }
                }
                TouchPhase::Moved | TouchPhase::Stationary => {
                    if let Some(&slot) = self.map.get(&touch.id) {
                        self.pending.entry(slot).or_default().push((time, touch.x, touch.y));
                        let trace = self.trace.entry(slot).or_default();
                        match trace.last() {
                            Some(last) if last.0 <= time => trace.push((time, touch.x, touch.y)),
                            _ => {
                                // a late sample lands where it happened
                                let at = trace.partition_point(|it| it.0 <= time);
                                trace.insert(at, (time, touch.x, touch.y));
                            }
                        }
                    }
                }
            }
        }
        self.run_swipe_grid(t, aspect, &mut frame);
        // Reconciliation: mapped touches the OS no longer reports have ended;
        // alive touches with no slot (they began while the judge wasn't
        // looking) appear. Ends first, so their slots can be reused.
        let clock = frame.last().map_or(clock, |it| it.0.max(clock));
        let mut dead: Vec<(u8, u64)> = self
            .map
            .iter()
            .filter(|(os, _)| !live.iter().any(|it| it.0 == **os))
            .map(|(os, slot)| (*slot, *os))
            .collect();
        dead.sort_unstable();
        for (slot, os) in dead {
            self.map.remove(&os);
            self.free_slot(slot);
            frame.push((clock, usize::MAX, InputEvent::End { id: slot, time: clock }));
        }
        for &(os, x, y) in live {
            if self.map.contains_key(&os) || started.contains(&os) {
                continue;
            }
            if let Some(slot) = self.alloc_slot() {
                self.map.insert(os, slot);
                self.last.insert(slot, (x, y));
                self.trace.insert(slot, vec![(clock, x, y)]);
                self.swipes.insert(slot, SwipeState::new(clock, (x, y)));
                self.swipe_next.insert(slot, swipe_grid_after(clock));
                frame.push((clock, usize::MAX, InputEvent::Appear { id: slot, time: clock, x, y }));
            }
        }
        frame.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        self.clock = frame.last().map_or(clock, |it| it.0.max(clock));
        // Merge with what is left over from earlier frames (events stamped
        // into the future of their frame); older entries win ties. A queued
        // Seek is a barrier: it belongs to the older timeline, and everything
        // from this frame — stamped after the clock it rewound to — sorts
        // behind it regardless of time.
        if self.queue.is_empty() {
            self.queue.extend(frame.into_iter().map(|it| it.2));
        } else {
            let mut rest = frame.into_iter().map(|it| it.2).peekable();
            let mut merged = VecDeque::with_capacity(self.queue.len() + rest.len());
            while let Some(front) = self.queue.front() {
                match rest.peek() {
                    Some(next) if !matches!(front, InputEvent::Seek { .. }) && event_time(next) < event_time(front) => {
                        merged.push_back(rest.next().unwrap())
                    }
                    _ => merged.push_back(self.queue.pop_front().unwrap()),
                }
            }
            merged.extend(rest);
            self.queue = merged;
        }
    }

    /// The swipe trigger runs on the fixed 60 Hz grid: whatever the frame
    /// rate, the finger is judged as if it had been tracked at 60 fps. When
    /// the movement stays slow relative to the previous step (`v_rel < u`),
    /// or the finger had stopped, a step at least `5u` fast starts a new
    /// gesture; otherwise a running gesture simply continues. Each step
    /// becomes a [`InputEvent::Swipe`] covering exactly that step.
    fn run_swipe_grid(&mut self, t: f64, aspect: f32, out: &mut Vec<(f64, usize, InputEvent)>) {
        let mut slots: Vec<u8> = self.swipes.keys().copied().collect();
        slots.sort_unstable();
        for slot in slots {
            while let Some(&k) = self.swipe_next.get(&slot) {
                let g = k as f64 / SWIPE_HZ;
                if g > t {
                    break;
                }
                let Some(samples) = self.trace.get_mut(&slot) else {
                    break;
                };
                let pos = sample_at(samples, g);
                // samples before the anchor of the next step can no longer
                // matter; the trace stays as long as one frame's samples
                let anchor = samples.partition_point(|it| it.0 <= g).saturating_sub(1);
                samples.drain(..anchor);
                let state = self.swipes.get_mut(&slot).unwrap();
                let (x0, y0) = state.pos;
                // world units (screen heights / 10) per 1/60 s. A step shorter
                // than that (the first one after a press) is not scaled up:
                // a pixel of jitter a millisecond after touching down would
                // otherwise read as a flick. A fast flick off the press is
                // still caught on the next step, from nearly the press point.
                let scale = (1. / SWIPE_HZ) / (g - state.prev_t).max(1. / SWIPE_HZ);
                let d1 = ((pos.0 - x0) as f64 * aspect as f64 / 2. / 0.1 * scale, (pos.1 - y0) as f64 / 2. / 0.1 * scale);
                let d0 = state.d0;
                let d0_mag = (d0.0 * d0.0 + d0.1 * d0.1).sqrt();
                let v_rel = if d0_mag > 0.1 { (d0.0 * d1.0 + d0.1 * d1.1) / d0_mag } else { 0. };
                let speed = (d1.0 * d1.0 + d1.1 * d1.1).sqrt();
                if v_rel < SWIPE_U || state.stopped {
                    if speed >= 5. * SWIPE_U {
                        state.stopped = false;
                        state.in_gesture = true;
                        out.push((
                            g,
                            usize::MAX - 1,
                            InputEvent::Swipe {
                                id: slot,
                                time: g,
                                new: true,
                                x0,
                                y0,
                                x1: pos.0,
                                y1: pos.1,
                            },
                        ));
                    } else {
                        state.stopped = true;
                        state.in_gesture = false;
                    }
                } else if state.in_gesture {
                    out.push((
                        g,
                        usize::MAX - 1,
                        InputEvent::Swipe {
                            id: slot,
                            time: g,
                            new: false,
                            x0,
                            y0,
                            x1: pos.0,
                            y1: pos.1,
                        },
                    ));
                }
                state.d0 = d1;
                state.pos = pos;
                state.prev_t = g;
                self.swipe_next.insert(slot, k + 1);
            }
        }
    }

    /// Samples the move positions of tick `tick` (time `tick /
    /// JUDGE_TICK_HZ`): per live slot, the latest pending sample not after
    /// the tick, kept only when it moved beyond [`MOVE_DEADBAND`] from the
    /// last emitted position. Samples after the tick stay for later.
    fn moves_at_tick(&mut self, tick: u32) -> Vec<MovePoint> {
        let g = tick as f64 / JUDGE_TICK_HZ;
        let mut slots: Vec<u8> = self.pending.keys().copied().collect();
        slots.sort_unstable();
        let mut points = Vec::new();
        for slot in slots {
            let Some(samples) = self.pending.get_mut(&slot) else {
                continue;
            };
            let mut best: Option<(f64, f32, f32)> = None;
            samples.retain(|&(time, x, y)| {
                if time <= g {
                    if best.is_none_or(|(bt, _, _)| bt <= time) {
                        best = Some((time, x, y));
                    }
                    false
                } else {
                    true
                }
            });
            if samples.is_empty() {
                self.pending.remove(&slot);
            }
            if let Some((_, x, y)) = best {
                if let Some(&(lx, ly)) = self.last.get(&slot) {
                    if (x - lx).abs() > MOVE_DEADBAND || (y - ly).abs() > MOVE_DEADBAND {
                        points.push(MovePoint { id: slot, x, y });
                        self.last.insert(slot, (x, y));
                    }
                }
            }
        }
        points
    }
}

fn event_time(event: &InputEvent) -> f64 {
    match event {
        InputEvent::Start { time, .. }
        | InputEvent::Appear { time, .. }
        | InputEvent::End { time, .. }
        | InputEvent::Swipe { time, .. }
        | InputEvent::Seek { from: time, .. } => *time,
        InputEvent::Move { .. } => f64::NEG_INFINITY,
    }
}

impl InputSource for InputCollector {
    fn peek_is_seek(&self) -> bool {
        matches!(self.queue.front(), Some(InputEvent::Seek { .. }))
    }

    fn peek_time(&self) -> Option<f64> {
        self.queue.front().map(event_time)
    }

    fn pop(&mut self) -> Option<InputEvent> {
        self.queue.pop_front()
    }

    fn moves_at(&mut self, tick: u32) -> Vec<MovePoint> {
        self.moves_at_tick(tick)
    }
}
