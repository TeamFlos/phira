//! Detailed recording of a single play.
//!
//! [`PlayRecord`] is the minimal self-contained representation of a play: the
//! metadata needed to interpret it, one entry per judged note, and the
//! canonical input stream the judge consumed. It deliberately stores no
//! aggregates — everything derivable lives in
//! [`PlayStats`](crate::play_stats::PlayStats), which is computed from a
//! `PlayRecord` alone. New metrics can therefore be added later without
//! having to re-record anything.

use crate::{
    config::Mods,
    core::{Chart, NoteKind},
    judge::{get_uptime, HitEvent, InputEvent, Judge, Judgement},
};
use bitflags::bitflags;
use macroquad::prelude::{Touch, TouchPhase};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const PLAY_RECORD_VERSION: u8 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum NoteKindTag {
    Click = 0,
    Hold = 1,
    Flick = 2,
    Drag = 3,
}

impl NoteKindTag {
    pub const COUNT: usize = 4;

    /// Whether a note of this kind has a meaningful timing error. Drags and
    /// flicks are judged by contact, not by when the contact happened.
    pub fn is_timed(self) -> bool {
        matches!(self, Self::Click | Self::Hold)
    }
}

impl From<&NoteKind> for NoteKindTag {
    fn from(kind: &NoteKind) -> Self {
        match kind {
            NoteKind::Click => Self::Click,
            NoteKind::Hold { .. } => Self::Hold,
            NoteKind::Flick => Self::Flick,
            NoteKind::Drag => Self::Drag,
        }
    }
}

impl From<NoteKindTag> for u8 {
    fn from(kind: NoteKindTag) -> Self {
        kind as Self
    }
}

impl TryFrom<u8> for NoteKindTag {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Ok(match value {
            0 => Self::Click,
            1 => Self::Hold,
            2 => Self::Flick,
            3 => Self::Drag,
            _ => return Err("invalid note kind"),
        })
    }
}

impl Serialize for NoteKindTag {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(*self as u8)
    }
}

impl<'de> Deserialize<'de> for NoteKindTag {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(u8::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(transparent)]
    pub struct NoteFlags: u8 {
        /// The note shares its time with at least one other note (`multiple_hint`).
        const MULTIPLE = 0b0001;
        /// A hold whose head was hit but which was released before the end.
        /// Only ever set together with [`Judgement::Miss`].
        const HOLD_BROKEN = 0b0010;
    }
}

/// One judged note. Self-contained: no chart lookup is needed to derive
/// statistics from it.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteRecord {
    pub line: u16,
    /// Index of the note within its line, telling same-time stacked notes
    /// on one line apart.
    pub note: u32,
    /// Head time of the note, in chart seconds.
    pub time: f32,
    pub kind: NoteKindTag,
    pub judgement: Judgement,
    /// Timing error in seconds, negative meaning early. Absent when there is
    /// none to speak of: misses, and every drag/flick.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff: Option<f32>,
    #[serde(default, skip_serializing_if = "NoteFlags::is_empty")]
    pub flags: NoteFlags,
}

/// Everything needed to interpret the numbers in a [`PlayRecord`].
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayMeta {
    /// The [`JUDGE_VERSION`](crate::judge::JUDGE_VERSION) the play was judged
    /// under. A re-judge must follow the same rules to reproduce the notes.
    pub judge_version: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chart_id: Option<i32>,
    /// Lowercase hex SHA-256 of the raw chart file, matching the digest the
    /// `verify-cksum` endpoint expects.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chart_hash: Option<String>,
    pub mods: Mods,
    /// Playback speed, already including the multiplier applied by NIGHTCORE.
    pub speed: f32,
    /// Total offset applied to the audio clock (chart + player setting + the
    /// per-chart tweak). Hit errors are measured against this adjusted clock,
    /// so their mean is the *residual* offset.
    pub offset: f32,
    /// The player's global offset setting on its own, so that a corrected value
    /// can be suggested from the residual above.
    pub player_offset: f32,
    pub use_keyboard: bool,
    pub aspect_ratio: f32,
    /// Length of the track in seconds.
    pub track_length: f32,
    /// Average frame rate over the play. The input stream is event-driven
    /// and does not depend on it; this is context for reading a record, and
    /// a hint at how trustworthy its timestamps are.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avg_fps: Option<f32>,
    /// The screen dpi the swipe trigger scaled by, as it ran live — an
    /// unreported one already replaced by the fallback, an out-of-band one
    /// already clamped ([`swipe_dpi`](crate::judge::swipe_dpi)). Context
    /// only: a replay consumes the recorded swipe steps without re-running
    /// detection, so it does not depend on this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dpi: Option<u32>,
}

/// A single touch sample, in the space [`Judge::get_touches`] normalizes to,
/// with `y` scaled by the aspect ratio so that both axes span `[-1, 1]`.
///
/// That space is relative to the viewport bound when the sample was taken, so
/// see [`TouchRecorder::poll`] for which one that is.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TouchPoint {
    /// A non-negative id starts or moves a touch; its bitwise negation ends it.
    pub id: i8,
    pub x: f32,
    pub y: f32,
    /// Precise chart time of the event, converted from the OS timestamp the
    /// way the judge does it. Present on starts and ends only — the moments a
    /// re-judge is decided by — and absent for moves and for touches without
    /// a timestamp (keyboard-synthesized ones), which fall back to the
    /// frame's time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayRecord {
    pub version: u8,
    pub meta: PlayMeta,
    pub notes: Vec<NoteRecord>,
    /// The canonical input stream the judge consumed, exactly as it consumed
    /// it — replaying this reproduces the play note for note, flicks and
    /// keyboard hits excepted.
    pub inputs: Vec<InputEvent>,
}

/// How densely a [`TouchRecorder`] samples moving touches. Starts and ends are
/// always recorded regardless.
#[derive(Clone, Copy, Debug)]
pub struct TouchSampling {
    /// Minimum interval between two recorded moves of the same touch.
    pub move_interval: f64,
}

impl TouchSampling {
    /// The multiplayer live stream, where bandwidth matters most.
    pub const LIVE: Self = Self { move_interval: 1. / 20. };

    /// Tolerance on [`Self::move_interval`]. Samples land on frame boundaries,
    /// and when the interval is a whole number of frames (1/30s is two frames
    /// at 60fps) a frame arriving a hair early would otherwise be skipped,
    /// silently stretching the interval by a frame — 30Hz becoming 20Hz.
    const SLACK: f64 = 0.004;

    fn throttles(&self, last: f32, time: f64) -> bool {
        time < last as f64 + self.move_interval - Self::SLACK
    }
}

/// Turns the raw per-frame touch state into a compact stream of changes, for
/// the multiplayer live stream. The play record no longer goes through here:
/// it stores the judge's own canonical input stream instead.
pub struct TouchRecorder {
    sampling: TouchSampling,
    ids: HashMap<u64, i8>,
    last_update: HashMap<i8, f32>,
}

impl TouchRecorder {
    pub fn new(sampling: TouchSampling) -> Self {
        Self {
            sampling,
            ids: HashMap::new(),
            last_update: HashMap::new(),
        }
    }

    pub fn reset(&mut self) {
        self.ids.clear();
        self.last_update.clear();
    }

    /// Allocates the lowest unused slot id, or `None` past `i8::MAX` live touches.
    fn alloc_id(&self) -> Option<i8> {
        let used = self.ids.values().fold(0_u128, |mask, id| mask | 1 << id);
        let id = (!used).trailing_zeros();
        (id <= i8::MAX as u32).then_some(id as i8)
    }

    /// Samples this frame's touches, returning only the ones worth recording.
    ///
    /// [`Judge::get_touches`] normalizes against whichever viewport is bound,
    /// so the caller decides the coordinate space; the multiplayer live
    /// stream samples with none bound, i.e. in full-screen space, which is
    /// what its consumer has always expected.
    /// `speed` converts OS event timestamps into chart seconds, with the same
    /// formula the judge itself uses, so that a start or end carries the
    /// moment the finger actually acted rather than the frame it was polled
    /// in — up to one frame (8-16ms) apart, which is a third of a Perfect
    /// window.
    pub fn poll(&mut self, time: f64, aspect_ratio: f32, speed: f64) -> Vec<TouchPoint> {
        self.poll_with(Judge::get_touches(), time, aspect_ratio, speed)
    }

    fn poll_with(&mut self, touches: Vec<Touch>, time: f64, aspect_ratio: f32, speed: f64) -> Vec<TouchPoint> {
        touches
            .into_iter()
            .filter_map(|touch| {
                if matches!(touch.phase, TouchPhase::Stationary) {
                    return None;
                }
                let ended = matches!(touch.phase, TouchPhase::Ended | TouchPhase::Cancelled);
                let id = match self.ids.get(&touch.id).copied() {
                    Some(id) => id,
                    // A touch we never saw begin — it started before recording
                    // or while paused. Emitting its end would leave the
                    // consumer with an id that was never opened.
                    None if ended => return None,
                    None => {
                        let id = self.alloc_id()?;
                        self.ids.insert(touch.id, id);
                        id
                    }
                };
                if matches!(touch.phase, TouchPhase::Moved) && self.last_update.get(&id).is_some_and(|last| self.sampling.throttles(*last, time)) {
                    return None;
                }
                // Only starts and ends earn an exact timestamp; a move is
                // happy with the frame's.
                let event_time = if matches!(touch.phase, TouchPhase::Started | TouchPhase::Ended | TouchPhase::Cancelled) && touch.time.is_finite() {
                    Some((time - (get_uptime() - touch.time) * speed) as f32)
                } else {
                    None
                };
                let time = time as f32;
                if ended {
                    self.ids.remove(&touch.id);
                    self.last_update.remove(&id);
                } else {
                    self.last_update.insert(id, time);
                }
                Some(TouchPoint {
                    id: if ended { !id } else { id },
                    x: touch.position.x,
                    y: touch.position.y * aspect_ratio,
                    time: event_time,
                })
            })
            .collect()
    }
}

impl PlayRecord {
    /// Joins the judge's input log with the judgements it produced.
    pub fn finish(meta: PlayMeta, chart: &Chart, hit_events: &[HitEvent], inputs: Vec<InputEvent>) -> Self {
        Self {
            version: PLAY_RECORD_VERSION,
            meta,
            notes: collect_notes(chart, hit_events),
            inputs,
        }
    }
}

fn collect_notes(chart: &Chart, events: &[HitEvent]) -> Vec<NoteRecord> {
    events
        .iter()
        .filter_map(|event| {
            let note = chart.lines.get(event.line_id as usize)?.notes.get(event.note_id as usize)?;
            let mut flags = NoteFlags::empty();
            flags.set(NoteFlags::MULTIPLE, note.multiple_hint);
            flags.set(NoteFlags::HOLD_BROKEN, event.hold_broken);
            let kind = NoteKindTag::from(&note.kind);
            // Guard the invariant here rather than trusting every judge path:
            // only timed kinds that were actually hit may carry an error.
            let timed = kind.is_timed() && !matches!(event.judgement, Judgement::Miss);
            Some(NoteRecord {
                line: event.line_id as u16,
                note: event.note_id,
                time: note.time as f32,
                kind,
                judgement: event.judgement,
                diff: event.diff.filter(|_| timed).map(|it| it as f32),
                flags,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use macroquad::prelude::Vec2;

    fn touch(id: u64, phase: TouchPhase) -> Touch {
        Touch {
            id,
            phase,
            position: Vec2::ZERO,
            time: f64::INFINITY,
        }
    }

    fn ids(points: &[TouchPoint]) -> Vec<i8> {
        points.iter().map(|it| it.id).collect()
    }

    #[test]
    fn touch_ids_are_reused_without_colliding_with_live_ones() {
        let mut recorder = TouchRecorder::new(TouchSampling::LIVE);
        assert_eq!(ids(&recorder.poll_with(vec![touch(10, TouchPhase::Started), touch(11, TouchPhase::Started)], 0., 1., 1.)), [0, 1]);
        // Slot 0 is released...
        assert_eq!(ids(&recorder.poll_with(vec![touch(10, TouchPhase::Ended)], 1., 1., 1.)), [!0]);
        // ...so the next touch must take slot 0, not collide with the live 1.
        assert_eq!(ids(&recorder.poll_with(vec![touch(12, TouchPhase::Started)], 2., 1., 1.)), [0]);
        assert_eq!(ids(&recorder.poll_with(vec![touch(11, TouchPhase::Ended), touch(12, TouchPhase::Ended)], 3., 1., 1.)), [!1, !0]);
    }

    #[test]
    fn an_end_without_a_start_is_dropped() {
        let mut recorder = TouchRecorder::new(TouchSampling::LIVE);
        // A touch held down since before recording began: emitting its end
        // would give the consumer an id it never saw opened.
        assert!(recorder.poll_with(vec![touch(7, TouchPhase::Ended)], 0., 1., 1.).is_empty());
        assert!(recorder.ids.is_empty());
    }

    #[test]
    fn moves_are_throttled_but_starts_and_ends_are_not() {
        let mut recorder = TouchRecorder::new(TouchSampling::LIVE);
        recorder.poll_with(vec![touch(1, TouchPhase::Started)], 0., 1., 1.);
        assert!(recorder.poll_with(vec![touch(1, TouchPhase::Moved)], 0.01, 1., 1.).is_empty());
        assert_eq!(ids(&recorder.poll_with(vec![touch(1, TouchPhase::Moved)], 0.2, 1., 1.)), [0]);
        // An end always gets through, however soon it arrives.
        assert_eq!(ids(&recorder.poll_with(vec![touch(1, TouchPhase::Ended)], 0.21, 1., 1.)), [!0]);
    }

    #[test]
    fn a_reused_slot_is_not_throttled_by_its_previous_owner() {
        let mut recorder = TouchRecorder::new(TouchSampling::LIVE);
        recorder.poll_with(vec![touch(1, TouchPhase::Started)], 0., 1., 1.);
        recorder.poll_with(vec![touch(1, TouchPhase::Ended)], 0., 1., 1.);
        assert!(recorder.last_update.is_empty(), "the throttle entry must be cleaned up");
        // A touch whose first recorded event is a move — it began while we
        // weren't recording. It inherits the freed slot, and a leftover
        // timestamp there would silently swallow this sample.
        assert_eq!(ids(&recorder.poll_with(vec![touch(2, TouchPhase::Moved)], 0.02, 1., 1.)), [0]);
    }

    #[test]
    fn stationary_touches_are_not_recorded() {
        let mut recorder = TouchRecorder::new(TouchSampling::LIVE);
        recorder.poll_with(vec![touch(1, TouchPhase::Started)], 0., 1., 1.);
        assert!(recorder.poll_with(vec![touch(1, TouchPhase::Stationary)], 5., 1., 1.).is_empty());
    }

    /// Drives one touch through a run of frames, returning which frames
    /// recorded a move.
    fn recorded_moves(recorder: &mut TouchRecorder, frames: impl IntoIterator<Item = (usize, f64)>) -> Vec<usize> {
        frames
            .into_iter()
            .filter(|&(_, time)| !recorder.poll_with(vec![touch(1, TouchPhase::Moved)], time, 1., 1.).is_empty())
            .map(|(frame, _)| frame)
            .collect()
    }

    #[test]
    fn frame_jitter_does_not_stretch_the_interval() {
        let sampling = TouchSampling { move_interval: 1. / 30. };
        let mut recorder = TouchRecorder::new(sampling);
        recorder.poll_with(vec![touch(1, TouchPhase::Started)], 0., 1., 1.);
        // 60fps, with every other even frame arriving 2ms early. Without the
        // slack those frames are skipped and the touch drops to 20Hz.
        let frames = (1..=60).map(|i| (i, i as f64 / 60. - if i % 4 == 2 { 0.002 } else { 0. }));
        assert_eq!(recorded_moves(&mut recorder, frames), (2..=60).step_by(2).collect::<Vec<_>>());
    }

    #[test]
    fn starts_and_ends_carry_the_event_time() {
        let mut recorder = TouchRecorder::new(TouchSampling::LIVE);
        let mut start = touch(1, TouchPhase::Started);
        start.time = get_uptime() - 0.25;
        let points = recorder.poll_with(vec![start], 10., 1., 1.);
        // 10 - 0.25 * 1, give or take the microseconds between the two
        // uptime reads.
        assert!((points[0].time.unwrap() - 9.75).abs() < 0.05, "{:?}", points[0].time);
        let mut end = touch(1, TouchPhase::Ended);
        end.time = get_uptime() - 0.5;
        let points = recorder.poll_with(vec![end], 11., 1., 1.5);
        // 11 - 0.5 * 1.5.
        assert!((points[0].time.unwrap() - 10.25).abs() < 0.05, "{:?}", points[0].time);
    }

    #[test]
    fn moves_and_untimestamped_touches_have_no_event_time() {
        let mut recorder = TouchRecorder::new(TouchSampling::LIVE);
        recorder.poll_with(vec![touch(1, TouchPhase::Started)], 0., 1., 1.);
        let mut moved = touch(1, TouchPhase::Moved);
        moved.time = get_uptime();
        assert_eq!(recorder.poll_with(vec![moved], 1., 1., 1.)[0].time, None);
        // No OS timestamp (the helper leaves it infinite, like a synthesized
        // touch): the frame's time has to do.
        assert_eq!(recorder.poll_with(vec![touch(2, TouchPhase::Started)], 5., 1., 1.)[0].time, None);
    }

    #[test]
    fn y_is_scaled_into_a_resolution_independent_space() {
        let mut recorder = TouchRecorder::new(TouchSampling::LIVE);
        let mut it = touch(1, TouchPhase::Started);
        it.position = Vec2::new(0.5, 0.25);
        let points = recorder.poll_with(vec![it], 0., 2., 1.);
        assert_eq!((points[0].x, points[0].y), (0.5, 0.5));
    }

    #[test]
    fn note_kind_tag_round_trips() {
        for kind in [NoteKindTag::Click, NoteKindTag::Hold, NoteKindTag::Flick, NoteKindTag::Drag] {
            assert_eq!(NoteKindTag::try_from(u8::from(kind)), Ok(kind));
        }
        assert!(NoteKindTag::try_from(4).is_err());
    }

    #[test]
    fn note_kind_tag_serializes_as_a_number() {
        assert_eq!(serde_json::to_string(&NoteKindTag::Flick).unwrap(), "2");
        assert_eq!(serde_json::from_str::<NoteKindTag>("1").unwrap(), NoteKindTag::Hold);
    }

    #[test]
    fn only_click_and_hold_are_timed() {
        assert!(NoteKindTag::Click.is_timed());
        assert!(NoteKindTag::Hold.is_timed());
        assert!(!NoteKindTag::Flick.is_timed());
        assert!(!NoteKindTag::Drag.is_timed());
    }

    #[test]
    fn absent_fields_are_omitted() {
        let note = NoteRecord {
            line: 0,
            note: 0,
            time: 1.,
            kind: NoteKindTag::Click,
            judgement: Judgement::Perfect,
            diff: None,
            flags: NoteFlags::empty(),
        };
        let json = serde_json::to_string(&note).unwrap();
        assert!(!json.contains("flags"), "{json}");
        assert!(!json.contains("diff"), "{json}");
    }
}
