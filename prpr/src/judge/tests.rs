//! Tests for the precise judge: live-vs-replay equality, frame-rate
//! independence, and the input collector's canonicalization. Charts are
//! built inline with the pure parsers; the live path is driven through
//! `update_with`, which needs no graphics context.
//!
//! All times in these tests are dyadic rationals and the synthetic uptime
//! starts at zero, so `t - (uptime - stamp)` is computed exactly and event
//! times are bit-identical no matter which frame delivered them.

use super::{
    input::{InputEvent, RawFrame, RawTouch},
    swipe_dpi, Judge, JudgeEnv, Judgement, MovePoint, ReplayPlayer, JUDGE_TICK_HZ, JUDGE_VERSION, LIMIT_BAD, LIMIT_GOOD, LIMIT_PERFECT,
    MOVE_DEADBAND,
};
use crate::{
    core::{Chart, ChartExtra, Point},
    parse::parse_phigros,
    play_record::{PlayMeta, PlayRecord, PLAY_RECORD_VERSION},
};
use macroquad::prelude::TouchPhase;
use std::{iter::once, sync::Arc, time::Instant};

const ASPECT: f32 = 16. / 9.;

fn env() -> JudgeEnv {
    JudgeEnv {
        aspect_ratio: ASPECT,
        speed: 1.,
        flip_x: false,
    }
}

/// bpm 1.875 makes the parser's time factor exactly 1, so chart seconds are
/// the JSON's time values.
fn chart_of(lines: Vec<serde_json::Value>) -> Chart {
    let source = serde_json::json!({
        "formatVersion": 3,
        "offset": 0.,
        "judgeLineList": lines,
    })
    .to_string();
    parse_phigros(&source, ChartExtra::default()).unwrap()
}

fn line_json(notes: Vec<serde_json::Value>) -> serde_json::Value {
    serde_json::json!({
        "bpm": 1.875,
        "judgeLineDisappearEvents": [{"startTime": 0., "endTime": 0., "start": 1., "end": 1.}],
        "judgeLineRotateEvents": [{"startTime": 0., "endTime": 0., "start": 0., "end": 0.}],
        "judgeLineMoveEvents": [{"startTime": 0., "endTime": 0., "start": 0.5, "end": 0.5, "start2": 0.5, "end2": 0.5}],
        "speedEvents": [{"startTime": 0., "endTime": 64., "value": 1.0}],
        "notesAbove": notes,
        "notesBelow": [],
    })
}

fn note(kind: u8, time: f64, position_x: f32, hold_time: f64) -> serde_json::Value {
    serde_json::json!({
        "type": kind,
        "time": time,
        "positionX": position_x,
        "holdTime": hold_time,
        "speed": 1.0,
        "floorPosition": 0.0,
    })
}

/// click at 1, hold 2..3, drag at 4, flick at 5, all at x = 0 on a
/// centered line.
fn basic_chart() -> Chart {
    chart_of(vec![line_json(vec![
        note(1, 1., 0., 0.),
        note(3, 2., 0., 1.),
        note(2, 4., 0., 0.),
        note(4, 5., 0., 0.),
    ])])
}

#[derive(Clone, Copy, Debug)]
enum Action {
    Down { id: u64, at: f64, pos: (f32, f32) },
    Move { id: u64, at: f64, pos: (f32, f32) },
    Up { id: u64, at: f64 },
}

impl Action {
    fn time(&self) -> f64 {
        match self {
            Action::Down { at, .. } | Action::Move { at, .. } | Action::Up { at, .. } => *at,
        }
    }
}

fn raw(id: u64, phase: TouchPhase, time: f64, (x, y): (f32, f32)) -> RawTouch {
    RawTouch { id, phase, time, x, y }
}

/// Delivers each action in the first frame whose time reaches it, with its
/// OS timestamp set to its own time (the synthetic uptime runs from zero),
/// and keeps the live set in sync — the way a well-behaved OS would.
/// Reports no dpi (0), the case on every platform that does not tell us
/// one: the swipe trigger falls back to its assumed 275, the value all
/// pre-dpi tests were tuned against.
fn drive(judge: &mut Judge, chart: &mut Chart, env: &JudgeEnv, schedule: &[f64], actions: &[Action]) {
    drive_dpi(judge, chart, env, schedule, actions, 0)
}

/// [`drive`], reporting `dpi` to the collector every frame.
fn drive_dpi(judge: &mut Judge, chart: &mut Chart, env: &JudgeEnv, schedule: &[f64], actions: &[Action], dpi: u32) {
    let mut prev = f64::NEG_INFINITY;
    let mut state: Vec<(u64, f32, f32)> = Vec::new();
    for &t in schedule {
        let mut events = Vec::new();
        for action in actions.iter().filter(|it| prev < it.time() && it.time() <= t) {
            match *action {
                Action::Down { id, at, pos } => {
                    state.retain(|it| it.0 != id);
                    state.push((id, pos.0, pos.1));
                    events.push(raw(id, TouchPhase::Started, at, pos));
                }
                Action::Move { id, at, pos } => {
                    if let Some(entry) = state.iter_mut().find(|it| it.0 == id) {
                        entry.1 = pos.0;
                        entry.2 = pos.1;
                        events.push(raw(id, TouchPhase::Moved, at, pos));
                    }
                }
                Action::Up { id, at } => {
                    state.retain(|it| it.0 != id);
                    events.push(raw(id, TouchPhase::Ended, at, (0., 0.)));
                }
            }
        }
        let mut effects = Vec::new();
        judge.update_with(
            chart,
            env,
            RawFrame {
                t,
                uptime: t,
                speed: 1.,
                aspect: ASPECT,
                dpi,
                raw: &events,
                live: &state,
            },
            0,
            &mut effects,
        );
        prev = t;
    }
}

fn fps(n: usize, step: f64) -> Vec<f64> {
    (0..n).map(|it| it as f64 * step).collect()
}

/// 60fps with one 300ms stall in the middle: the frame due at 40/64 arrives
/// 300ms late, and every later frame shifts with it.
fn stalled() -> Vec<f64> {
    let mut times = fps(400, 1. / 64.);
    for time in times.iter_mut().skip(41) {
        *time += 0.3;
    }
    times
}

fn play(actions: &[Action], schedule: &[f64]) -> (Chart, Judge) {
    let mut chart = basic_chart();
    let mut judge = Judge::new(&chart);
    drive(&mut judge, &mut chart, &env(), schedule, actions);
    (chart, judge)
}

/// [`play`], with the platform reporting `dpi` on every frame.
fn play_dpi(actions: &[Action], schedule: &[f64], dpi: u32) -> (Chart, Judge) {
    let mut chart = basic_chart();
    let mut judge = Judge::new(&chart);
    drive_dpi(&mut judge, &mut chart, &env(), schedule, actions, dpi);
    (chart, judge)
}

/// A full basic play: a perfect tap, a good hold held to its end, a drag
/// covered by a resting finger, and a flick swiped right through.
fn full_actions() -> Vec<Action> {
    vec![
        Action::Down {
            id: 1,
            at: 1.,
            pos: (0., 0.),
        },
        Action::Up { id: 1, at: 1.0625 },
        Action::Down {
            id: 2,
            at: 61. / 32.,
            pos: (0., 0.),
        },
        Action::Up { id: 2, at: 3.0625 },
        Action::Down {
            id: 3,
            at: 3.875,
            pos: (0., 0.),
        },
        Action::Up { id: 3, at: 4.125 },
        Action::Down {
            id: 4,
            at: 317. / 64.,
            pos: (0., 0.),
        },
        Action::Move {
            id: 4,
            at: 4.96875,
            pos: (0.05, 0.),
        },
        Action::Move {
            id: 4,
            at: 4.984375,
            pos: (0.1, 0.),
        },
        Action::Move {
            id: 4,
            at: 5.,
            pos: (0.15, 0.),
        },
        Action::Up { id: 4, at: 5.046875 },
    ]
}

/// Presses at `at` on (`x0`, 0) and glides right at a constant `v` stream
/// units per second until `until`, sampling the position every 1/64 s. On
/// the swipe grid this covers `v / 60` units per step — `(v / 60) * 80 / 9`
/// world units at the test aspect of 16:9.
fn glide(id: u64, at: f64, x0: f32, v: f64, until: f64) -> Vec<Action> {
    let mut actions = vec![Action::Down { id, at, pos: (x0, 0.) }];
    for k in 1..=((until - at) * 64.).round() as i64 {
        let t = at + k as f64 / 64.;
        actions.push(Action::Move {
            id,
            at: t,
            pos: ((x0 as f64 + v * (t - at)) as f32, 0.),
        });
    }
    actions.push(Action::Up { id, at: until });
    actions
}

/// The hit event of the note with the given head time. The chart reorders
/// its notes for rendering, so tests must not hardcode note ids.
fn event_at<'a>(judge: &'a Judge, chart: &Chart, time: f64) -> &'a super::HitEvent {
    let (line, note) = chart
        .lines
        .iter()
        .enumerate()
        .find_map(|(li, line)| line.notes.iter().position(|it| it.time == time).map(|ni| (li as u32, ni as u32)))
        .unwrap();
    judge.hit_events.iter().find(|it| it.line_id == line && it.note_id == note).unwrap()
}

fn events_of(judge: &Judge) -> String {
    hits_of(&judge.hit_events)
}

fn hits_of(hits: &[super::HitEvent]) -> String {
    hits.iter()
        .map(|it| format!("{:?}/{:?}/{:?}", it.line_id, it.note_id, it.judgement))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn judge_version_is_bumped() {
    assert_eq!(JUDGE_VERSION, 2);
}

#[test]
fn a_replay_reproduces_a_live_run_note_for_note() {
    let schedule = fps(400, 1. / 64.);
    let (_, judge) = play(&full_actions(), &schedule);

    let mut chart = basic_chart();
    let replayed = Judge::replay(&mut chart, &env(), &judge.inputs, 5.);
    assert_eq!(replayed.hit_events, judge.hit_events, "{}\nvs\n{}", events_of(&replayed), events_of(&judge));
    assert_eq!(replayed.result().counts, judge.result().counts);
    assert!(replayed.inputs.is_empty(), "a replay must not log a stream");
}

#[test]
fn results_do_not_depend_on_the_frame_rate() {
    let actions = full_actions();
    let (chart60, judge60) = play(&actions, &fps(400, 1. / 64.));
    let (chart144, judge144) = play(&actions, &fps(800, 1. / 128.));
    let (chart_stall, judge_stall) = play(&actions, &stalled());

    assert_eq!(judge60.inputs, judge144.inputs);
    assert_eq!(judge60.hit_events, judge144.hit_events, "{}\nvs\n{}", events_of(&judge60), events_of(&judge144));
    assert_eq!(judge60.inputs, judge_stall.inputs);
    assert_eq!(judge60.hit_events, judge_stall.hit_events);
    drop(chart60);
    drop(chart144);
    drop(chart_stall);
}

#[test]
fn candidate_line_filtering_changes_nothing() {
    let actions = full_actions();
    let mut chart = basic_chart();
    let mut filtered = Judge::new(&chart);
    drive(&mut filtered, &mut chart, &env(), &fps(400, 1. / 64.), &actions);

    let mut chart = basic_chart();
    let mut plain = Judge::new(&chart);
    plain.set_candidate_filter(false);
    drive(&mut plain, &mut chart, &env(), &fps(400, 1. / 64.), &actions);

    assert_eq!(filtered.hit_events, plain.hit_events, "{}\nvs\n{}", events_of(&filtered), events_of(&plain));
}

#[test]
fn an_on_time_tap_is_perfect_with_no_error() {
    let (_, judge) = play(
        &[
            Action::Down {
                id: 1,
                at: 1.,
                pos: (0., 0.),
            },
            Action::Up { id: 1, at: 1.0625 },
        ],
        &fps(80, 1. / 64.),
    );
    let event = &judge.hit_events[0];
    assert_eq!(event.judgement, Judgement::Perfect);
    assert!(event.diff.unwrap().abs() < 1e-9, "{:?}", event.diff);
}

#[test]
fn a_bad_carries_the_click_time_not_the_frame_time() {
    // 200ms early: past the good window, inside the click search.
    let (chart, judge) = play(
        &[
            Action::Down {
                id: 1,
                at: 0.8,
                pos: (0., 0.),
            },
            Action::Up { id: 1, at: 1. },
        ],
        &fps(80, 1. / 64.),
    );
    let event = event_at(&judge, &chart, 1.);
    assert_eq!(event.judgement, Judgement::Bad);
    let diff = event.diff.expect("a bad must be timed now");
    assert!((diff + 0.2).abs() < 1e-9, "{diff}");
}

#[test]
fn a_hold_survives_a_short_absence_and_breaks_on_a_long_one() {
    let hold = |away: f64| {
        let away_start = 2.5;
        let back = away_start + away;
        let actions = vec![
            Action::Down {
                id: 1,
                at: 61. / 32.,
                pos: (0., 0.),
            },
            Action::Move {
                id: 1,
                at: away_start,
                pos: (0.9, 0.),
            },
            Action::Move {
                id: 1,
                at: back,
                pos: (0., 0.),
            },
            Action::Up { id: 1, at: 3.0625 },
        ];
        play(&actions, &fps(512, 1. / 128.))
    };
    let (chart, short) = hold(30. / 1000.);
    let hold_event = event_at(&short, &chart, 2.);
    assert_eq!(hold_event.judgement, Judgement::Good, "{:?}", short.hit_events);
    assert!(!hold_event.hold_broken);

    let (chart, long) = hold(100. / 1000.);
    let hold_event = event_at(&long, &chart, 2.);
    assert_eq!(hold_event.judgement, Judgement::Miss);
    assert!(hold_event.hold_broken);
}

#[test]
fn a_resting_finger_judges_a_drag() {
    let (chart, judge) = play(
        &[
            Action::Down {
                id: 1,
                at: 3.875,
                pos: (0., 0.),
            },
            Action::Up { id: 1, at: 4.125 },
        ],
        &fps(320, 1. / 64.),
    );
    let event = event_at(&judge, &chart, 4.);
    assert_eq!(event.judgement, Judgement::Perfect);
    assert_eq!(event.diff, None, "a drag is judged by contact, not timing");
}

/// Sweeping a child line walks its ancestor chain by position in the chain,
/// not by line id: a drag on line 0 under parent 3 under grandparent 2 used
/// to index the two-to-three-entry chain with 3 and panic.
#[test]
fn a_drag_on_a_nested_child_line_is_swept() {
    let mut chart = chart_of(vec![
        line_json(vec![note(2, 4., 0., 0.)]),
        line_json(vec![]),
        line_json(vec![]),
        line_json(vec![]),
    ]);
    chart.lines[0].parent = Some(3);
    chart.lines[3].parent = Some(2);
    let mut judge = Judge::new(&chart);
    drive(
        &mut judge,
        &mut chart,
        &env(),
        &fps(320, 1. / 64.),
        &[
            Action::Down {
                id: 1,
                at: 3.875,
                pos: (0., 0.),
            },
            Action::Up { id: 1, at: 4.125 },
        ],
    );
    assert_eq!(event_at(&judge, &chart, 4.).judgement, Judgement::Perfect);
}

#[test]
fn a_note_past_its_window_misses_before_any_click() {
    // 281.25ms late: past bad, so the tap arriving at 1.28125 cannot hit.
    let (_, judge) = play(
        &[
            Action::Down {
                id: 1,
                at: 41. / 32.,
                pos: (0., 0.),
            },
            Action::Up { id: 1, at: 1.5 },
        ],
        &fps(120, 1. / 64.),
    );
    assert_eq!(judge.hit_events.len(), 1);
    assert_eq!(judge.hit_events[0].judgement, Judgement::Miss);
}

#[test]
fn a_child_of_a_moving_parent_line_is_judged_where_it_actually_is() {
    let mut chart = chart_of(vec![
        // a parent rotating from 0 to 90 degrees over seconds 1..2
        serde_json::json!({
            "bpm": 1.875,
            "judgeLineDisappearEvents": [{"startTime": 0., "endTime": 0., "start": 1., "end": 1.}],
            "judgeLineRotateEvents": [
                {"startTime": 0., "endTime": 1., "start": 0., "end": 0.},
                {"startTime": 1., "endTime": 2., "start": 0., "end": 90.},
                {"startTime": 2., "endTime": 4., "start": 90., "end": 90.},
            ],
            "judgeLineMoveEvents": [{"startTime": 0., "endTime": 0., "start": 0.5, "end": 0.5, "start2": 0.5, "end2": 0.5}],
            "speedEvents": [{"startTime": 0., "endTime": 64., "value": 1.0}],
            "notesAbove": [],
            "notesBelow": [],
        }),
        line_json(vec![note(1, 1.5, 32. / 9., 0.)]),
    ]);
    chart.lines[1].parent = Some(0);
    chart.lines[1].rot_with_parent = true;

    let env = env();
    // Where the note actually is at 1.5s, by the chart's own transforms.
    let tap = {
        let tau = 1.5;
        for line in &mut chart.lines {
            line.object.set_time(tau);
        }
        let tr = chart.lines[1].now_transform_with(env.aspect_ratio, &chart.lines);
        let note = &chart.lines[1].notes[0];
        let world = tr.transform_point(&Point::new(note.object.translation.0.now(), 0.));
        (world.x, -world.y * env.aspect_ratio)
    };
    let mut judge = Judge::new(&chart);
    drive(&mut judge, &mut chart, &env, &fps(128, 1. / 64.), &[Action::Down { id: 1, at: 1.5, pos: tap }, Action::Up { id: 1, at: 1.5625 }]);
    assert_eq!(judge.hit_events.len(), 1);
    assert_eq!(judge.hit_events[0].judgement, Judgement::Perfect, "{}", events_of(&judge));
}

#[test]
fn reconciliation_ends_disappeared_touches_and_appears_unseen_ones() {
    let mut chart = basic_chart();
    let mut judge = Judge::new(&chart);
    let env = env();
    let mut effects = Vec::new();

    // a touch starts, then the OS reports it gone without an End event
    judge.update_with(
        &mut chart,
        &env,
        RawFrame {
            t: 0.5,
            uptime: 0.5,
            speed: 1.,
            aspect: ASPECT,
            dpi: 0,
            raw: &[raw(7, TouchPhase::Started, 0.5, (0., 0.))],
            live: &[(7, 0., 0.)],
        },
        0,
        &mut effects,
    );
    judge.update_with(
        &mut chart,
        &env,
        RawFrame {
            t: 1.,
            uptime: 1.,
            speed: 1.,
            aspect: ASPECT,
            dpi: 0,
            raw: &[],
            live: &[],
        },
        0,
        &mut effects,
    );
    assert!(matches!(judge.inputs.last(), Some(InputEvent::End { id: 0, .. })), "{:?}", judge.inputs);

    // a touch first seen already pressed appears, and does not click: the
    // click at 1 has missed long ago, and nothing else may be judged
    judge.update_with(
        &mut chart,
        &env,
        RawFrame {
            t: 2.,
            uptime: 2.,
            speed: 1.,
            aspect: ASPECT,
            dpi: 0,
            raw: &[],
            live: &[(9, 0., 0.)],
        },
        0,
        &mut effects,
    );
    assert!(matches!(judge.inputs.last(), Some(InputEvent::Appear { .. })), "{:?}", judge.inputs);
    assert_eq!(judge.hit_events.len(), 1, "{:?}", judge.hit_events);
    assert_eq!(judge.hit_events[0].judgement, Judgement::Miss);

    // an OS id starting again while mapped closes the old slot first
    judge.update_with(
        &mut chart,
        &env,
        RawFrame {
            t: 3.,
            uptime: 3.,
            speed: 1.,
            aspect: ASPECT,
            dpi: 0,
            raw: &[raw(9, TouchPhase::Started, 3., (0.5, 0.))],
            live: &[(9, 0.5, 0.)],
        },
        0,
        &mut effects,
    );
    let tail: Vec<&InputEvent> = judge.inputs.iter().rev().take(2).collect();
    assert!(matches!(tail[0], InputEvent::Start { .. }), "{:?}", judge.inputs);
    assert!(matches!(tail[1], InputEvent::End { .. }), "{:?}", judge.inputs);
}

#[test]
fn moves_below_the_deadband_are_not_sampled() {
    let mut chart = basic_chart();
    let mut judge = Judge::new(&chart);
    let env = env();
    let mut effects = Vec::new();
    judge.update_with(
        &mut chart,
        &env,
        RawFrame {
            t: 0.,
            uptime: 0.,
            speed: 1.,
            aspect: ASPECT,
            dpi: 0,
            raw: &[raw(1, TouchPhase::Started, 0., (0., 0.))],
            live: &[(1, 0., 0.)],
        },
        0,
        &mut effects,
    );
    let mut pos = (0f32, 0f32);
    for step in 1..=90 {
        // oscillating halfway below the deadband: every sample stays within
        // it of the last emitted position, so nothing may ever be sampled
        pos.0 = if step % 2 == 0 { MOVE_DEADBAND / 2. } else { 0. };
        let t = step as f64 / 64.;
        judge.update_with(
            &mut chart,
            &env,
            RawFrame {
                t,
                uptime: t,
                speed: 1.,
                aspect: ASPECT,
                dpi: 0,
                raw: &[raw(1, TouchPhase::Moved, t, pos)],
                live: &[(1, pos.0, pos.1)],
            },
            0,
            &mut effects,
        );
    }
    let moves: Vec<_> = judge.inputs.iter().filter(|it| matches!(it, InputEvent::Move { .. })).cloned().collect();
    assert!(moves.is_empty(), "{moves:?}");
}

#[test]
fn stale_event_times_are_clamped_to_the_stream_clock() {
    let mut chart = basic_chart();
    let mut judge = Judge::new(&chart);
    let env = env();
    let mut effects = Vec::new();
    // ticks advance the clock to 1.0, then a start stamped 0.5 arrives late
    judge.update_with(
        &mut chart,
        &env,
        RawFrame {
            t: 1.,
            uptime: 1.,
            speed: 1.,
            aspect: ASPECT,
            dpi: 0,
            raw: &[],
            live: &[],
        },
        0,
        &mut effects,
    );
    judge.update_with(
        &mut chart,
        &env,
        RawFrame {
            t: 2.,
            uptime: 2.,
            speed: 1.,
            aspect: ASPECT,
            dpi: 0,
            raw: &[raw(1, TouchPhase::Started, 0.5, (0., 0.))],
            live: &[(1, 0., 0.)],
        },
        0,
        &mut effects,
    );
    assert!(matches!(judge.inputs[0], InputEvent::Start { time, .. } if time == 1.), "{:?}", judge.inputs);
}

#[test]
fn a_backward_clock_produces_a_seek() {
    let mut chart = basic_chart();
    let mut judge = Judge::new(&chart);
    let env = env();
    let mut effects = Vec::new();
    // a touch moving every frame, so its position is sampled at every tick
    judge.update_with(
        &mut chart,
        &env,
        RawFrame {
            t: 0.,
            uptime: 0.,
            speed: 1.,
            aspect: ASPECT,
            dpi: 0,
            raw: &[raw(1, TouchPhase::Started, 0., (0., 0.))],
            live: &[(1, 0., 0.)],
        },
        0,
        &mut effects,
    );
    judge.update_with(
        &mut chart,
        &env,
        RawFrame {
            t: 1.,
            uptime: 1.,
            speed: 1.,
            aspect: ASPECT,
            dpi: 0,
            raw: &[],
            live: &[(1, 0., 0.)],
        },
        0,
        &mut effects,
    );
    judge.update_with(
        &mut chart,
        &env,
        RawFrame {
            t: 0.5,
            uptime: 0.5,
            speed: 1.,
            aspect: ASPECT,
            dpi: 0,
            raw: &[],
            live: &[(1, 0., 0.)],
        },
        0,
        &mut effects,
    );
    assert!(matches!(judge.inputs.last(), Some(InputEvent::Seek { from: 1., to: 0.5 })), "{:?}", judge.inputs);
    // the grid restarts strictly after the seek target: ticks 15 (0.5) is
    // not repeated, the next move lands on tick 16
    let mut x = 0.;
    for step in 1..=8 {
        x += 0.01;
        let t = 0.5 + step as f64 / 64.;
        judge.update_with(
            &mut chart,
            &env,
            RawFrame {
                t,
                uptime: t,
                speed: 1.,
                aspect: ASPECT,
                dpi: 0,
                raw: &[raw(1, TouchPhase::Moved, t, (x, 0.))],
                live: &[(1, x, 0.)],
            },
            0,
            &mut effects,
        );
    }
    let ticks: Vec<u32> = judge
        .inputs
        .iter()
        .filter_map(|it| match it {
            InputEvent::Move { tick, .. } => Some(*tick),
            _ => None,
        })
        .collect();
    // nothing at or before tick 15 (0.5) may repeat
    assert_eq!(ticks.first(), Some(&16), "{ticks:?}");
}

#[test]
fn input_events_round_trip_through_serde() {
    let events = vec![
        InputEvent::Start {
            id: 3,
            time: 1.25,
            x: -0.5,
            y: 0.25,
        },
        InputEvent::Appear {
            id: 0,
            time: 0.,
            x: 0.,
            y: 0.,
        },
        InputEvent::End { id: 3, time: 2.5 },
        InputEvent::Move {
            tick: 900,
            points: vec![MovePoint { id: 3, x: 1., y: -1. }],
        },
        InputEvent::Swipe {
            id: 1,
            from: 3.4833,
            time: 3.5,
            new: true,
            x0: -0.2,
            y0: 0.05,
            x1: 0.2,
            y1: -0.05,
        },
        InputEvent::Seek { from: 10., to: 7.5 },
    ];
    let json = serde_json::to_string(&events).unwrap();
    assert_eq!(serde_json::from_str::<Vec<InputEvent>>(&json).unwrap(), events);
}

/// Plays `actions` on a custom chart.
fn play_on(chart: Chart, actions: &[Action], schedule: &[f64]) -> (Chart, Judge) {
    let mut chart = chart;
    let mut judge = Judge::new(&chart);
    drive(&mut judge, &mut chart, &env(), schedule, actions);
    (chart, judge)
}

#[test]
fn tap_windows_are_symmetric_with_no_late_bad() {
    let cases = [
        (1. - 0.10, Judgement::Good),
        (1. + 0.10, Judgement::Good),
        (1. + 0.17, Judgement::Good),
        (1. + 0.19, Judgement::Miss),
    ];
    for (at, expected) in cases {
        let (chart, judge) = play(&[Action::Down { id: 1, at, pos: (0., 0.) }, Action::Up { id: 1, at: at + 0.0625 }], &fps(80, 1. / 64.));
        assert_eq!(event_at(&judge, &chart, 1.).judgement, expected, "tap at {at}");
    }
}

#[test]
fn an_early_tap_at_the_lateral_edge_is_not_matched() {
    // 0.2 early and 0.19 off the note's center: inside the click width, but
    // past the bad window shrunk by that offset, so nothing is judged and
    // the note eventually misses
    let (_, judge) = play_on(
        chart_of(vec![line_json(vec![note(1, 1., 8. / 3., 0.)])]),
        &[
            Action::Down {
                id: 1,
                at: 0.8,
                pos: (0.49, 0.),
            },
            Action::Up { id: 1, at: 0.9 },
        ],
        &fps(80, 1. / 64.),
    );
    assert_eq!(judge.hit_events.len(), 1, "{}", events_of(&judge));
    assert_eq!(judge.hit_events[0].judgement, Judgement::Miss);
}

#[test]
fn a_note_left_behind_does_not_drag_a_stream_into_goods() {
    // five taps 125ms apart at one spot, the first never pressed: each press
    // lands right on its own note, which is nearer than the one left behind
    // (by then a Good away), so the stream stays Perfect and only the first
    // note misses
    let times = [1., 1.125, 1.25, 1.375, 1.5];
    let chart = chart_of(vec![line_json(times.iter().map(|&t| note(1, t, 0., 0.)).collect())]);
    let actions: Vec<Action> = times[1..]
        .iter()
        .enumerate()
        .flat_map(|(i, &at)| {
            let id = i as u64 + 1;
            [Action::Down { id, at, pos: (0., 0.) }, Action::Up { id, at: at + 1. / 32. }]
        })
        .collect();
    let (chart, judge) = play_on(chart, &actions, &fps(160, 1. / 64.));
    assert_eq!(event_at(&judge, &chart, 1.).judgement, Judgement::Miss, "{}", events_of(&judge));
    for &t in &times[1..] {
        let hit = event_at(&judge, &chart, t);
        assert_eq!(hit.judgement, Judgement::Perfect, "note at {t}: {}", events_of(&judge));
        assert!(hit.diff.unwrap().abs() < 1e-9, "note at {t} hit off its own press: {:?}", hit.diff);
    }
}

#[test]
fn a_late_press_within_perfect_keeps_the_earlier_note() {
    // two taps 62.5ms apart, both pressed 46.875ms late: the first press is
    // nearer the second note, but the first is still a Perfect away and
    // keeps its claim, so each note takes its own press
    let late = 3. / 64.;
    let chart = chart_of(vec![line_json(vec![note(1, 1., 0., 0.), note(1, 1.0625, 0., 0.)])]);
    let (chart, judge) = play_on(
        chart,
        &[
            Action::Down {
                id: 1,
                at: 1. + late,
                pos: (0., 0.),
            },
            Action::Up {
                id: 1,
                at: 1. + late + 1. / 64.,
            },
            Action::Down {
                id: 2,
                at: 1.0625 + late,
                pos: (0., 0.),
            },
            Action::Up {
                id: 2,
                at: 1.0625 + late + 1. / 64.,
            },
        ],
        &fps(160, 1. / 64.),
    );
    for t in [1., 1.0625] {
        let hit = event_at(&judge, &chart, t);
        assert_eq!(hit.judgement, Judgement::Perfect, "note at {t}: {}", events_of(&judge));
        assert!((hit.diff.unwrap() - late).abs() < 1e-9, "note at {t}: {:?}", hit.diff);
    }
}

#[test]
fn drag_protection_eats_one_press() {
    // a drag and a 50ms-later tap on one line: an early press at the tap's
    // position is eaten by the drag (which that same finger goes on to
    // touch), and only the second press reaches the tap
    let chart = chart_of(vec![line_json(vec![note(2, 2., 8. / 3., 0.), note(1, 2.05, 32. / 9., 0.)])]);
    let (chart, judge) = play_on(
        chart,
        &[
            Action::Down {
                id: 1,
                at: 1.98,
                pos: (0.4, 0.),
            },
            Action::Up { id: 1, at: 1.99 },
            Action::Down {
                id: 2,
                at: 2.05,
                pos: (0.4, 0.),
            },
            Action::Up { id: 2, at: 2.109375 },
        ],
        &fps(160, 1. / 64.),
    );
    let tap = event_at(&judge, &chart, 2.05);
    assert_eq!(tap.judgement, Judgement::Perfect, "{}", events_of(&judge));
    assert!(tap.diff.unwrap().abs() < 1e-9, "hit by the second press, not the first: {:?}", tap.diff);
    let drag = event_at(&judge, &chart, 2.);
    assert_eq!(drag.judgement, Judgement::Perfect);

    // 5ms apart instead: the tap is within the simultaneity tolerance, so
    // the very first press already goes to it
    let chart = chart_of(vec![line_json(vec![note(2, 2., 8. / 3., 0.), note(1, 2.005, 32. / 9., 0.)])]);
    let (chart, judge) = play_on(
        chart,
        &[
            Action::Down {
                id: 1,
                at: 1.98,
                pos: (0.4, 0.),
            },
            Action::Up { id: 1, at: 2.0625 },
        ],
        &fps(160, 1. / 64.),
    );
    let tap = event_at(&judge, &chart, 2.005);
    assert_eq!(tap.judgement, Judgement::Perfect, "{}", events_of(&judge));
    assert!((tap.diff.unwrap() + 0.025).abs() < 1e-9, "hit by the first press: {:?}", tap.diff);
    assert_eq!(event_at(&judge, &chart, 2.).judgement, Judgement::Perfect);
}

#[test]
fn simultaneous_taps_go_to_the_closer_line() {
    // #871: two lines, one tap each at the same time; the press sits nearer
    // line A (weighted dx + |dy|/2.2), and which line comes first in the
    // chart must not matter
    let chart_with = |second: serde_json::Value, swap: bool| {
        let mut lines = vec![line_json(vec![note(1, 2., 8. / 3., 0.)]), second];
        if swap {
            lines.swap(0, 1);
        }
        chart_of(lines)
    };
    let line_b = serde_json::json!({
        "bpm": 1.875,
        "judgeLineDisappearEvents": [{"startTime": 0., "endTime": 0., "start": 1., "end": 1.}],
        "judgeLineRotateEvents": [{"startTime": 0., "endTime": 0., "start": 0., "end": 0.}],
        "judgeLineMoveEvents": [{"startTime": 0., "endTime": 0., "start": 0.5, "end": 0.5, "start2": 0.6, "end2": 0.6}],
        "speedEvents": [{"startTime": 0., "endTime": 64., "value": 1.0}],
        "notesAbove": [note(1, 2., 8. / 3., 0.)],
        "notesBelow": [],
    });
    let actions = |id: u64| {
        vec![
            Action::Down {
                id,
                at: 2.,
                pos: (0.3, -0.01 * ASPECT),
            },
            Action::Up { id, at: 2.0625 },
        ]
    };
    for (order, chart) in [(0, chart_with(line_b.clone(), false)), (1, chart_with(line_b, true))] {
        let (chart, judge) = play_on(chart, &actions(1), &fps(192, 1. / 64.));
        // line ids: whichever way the chart lists them, the press is nearer
        // the line sitting at the screen's center line (y = 0)
        let near = chart
            .lines
            .iter()
            .position(|line| line.object.translation.1.now_opt().unwrap_or(0.).abs() < 1e-3)
            .unwrap() as u32;
        let hit = judge.hit_events.iter().find(|it| it.judgement != Judgement::Miss).unwrap();
        assert_eq!(hit.line_id, near, "order {order}: {}", events_of(&judge));
        assert_eq!(hit.judgement, Judgement::Perfect);
    }
}

#[test]
fn an_early_hold_press_enters_the_hold_as_good() {
    // 0.2 early is inside the head's early window: no bad, the hold simply
    // starts as a good one
    let (chart, judge) = play(
        &[
            Action::Down {
                id: 1,
                at: 1.8,
                pos: (0., 0.),
            },
            Action::Up { id: 1, at: 3.0625 },
        ],
        &fps(320, 1. / 64.),
    );
    let hold = event_at(&judge, &chart, 2.);
    assert_eq!(hold.judgement, Judgement::Good, "{}", events_of(&judge));
    assert!(!hold.hold_broken);
    assert!((hold.diff.unwrap() + 0.2).abs() < 1e-9);
}

#[test]
fn drag_contact_has_a_tenth_of_a_second() {
    let (chart, judge) = play(
        &[
            Action::Down {
                id: 1,
                at: 4.09,
                pos: (0., 0.),
            },
            Action::Up { id: 1, at: 4.1875 },
        ],
        &fps(320, 1. / 64.),
    );
    assert_eq!(event_at(&judge, &chart, 4.).judgement, Judgement::Perfect);

    let (chart, judge) = play(
        &[
            Action::Down {
                id: 1,
                at: 4.125,
                pos: (0., 0.),
            },
            Action::Up { id: 1, at: 4.25 },
        ],
        &fps(320, 1. / 64.),
    );
    assert_eq!(event_at(&judge, &chart, 4.).judgement, Judgement::Miss);
}

#[test]
fn pressing_on_a_flick_and_swiping_immediately_hits_it() {
    // #736: the first step of the gesture starts at the press point, so a
    // flick can be judged by a swipe that begins on it
    let (chart, judge) = play(
        &[
            Action::Down {
                id: 1,
                at: 4.98,
                pos: (0., 0.),
            },
            Action::Move {
                id: 1,
                at: 4.984375,
                pos: (0.05, 0.),
            },
            Action::Move {
                id: 1,
                at: 5.,
                pos: (0.1, 0.),
            },
            Action::Up { id: 1, at: 5.046875 },
        ],
        &fps(352, 1. / 64.),
    );
    assert_eq!(event_at(&judge, &chart, 5.).judgement, Judgement::Perfect, "{}", events_of(&judge));
}

#[test]
fn a_fast_swipe_crossing_the_whole_band_still_hits() {
    // #863: the swipe is fast enough that no grid point lands inside the
    // flick's band; the segment between two points still covers it
    let (chart, judge) = play(
        &[
            Action::Down {
                id: 1,
                at: 4.93,
                pos: (-0.3, 0.),
            },
            Action::Move {
                id: 1,
                at: 4.94,
                pos: (-0.25, 0.),
            },
            Action::Move {
                id: 1,
                at: 4.97,
                pos: (0.3, 0.),
            },
            Action::Move {
                id: 1,
                at: 4.99,
                pos: (0.5, 0.),
            },
            Action::Up { id: 1, at: 5.046875 },
        ],
        &fps(352, 1. / 64.),
    );
    assert_eq!(event_at(&judge, &chart, 5.).judgement, Judgement::Perfect, "{}", events_of(&judge));
}

#[test]
fn one_gesture_judges_one_flick_and_turning_back_judges_another() {
    let two_flicks = || chart_of(vec![line_json(vec![note(4, 4., -4. / 3., 0.), note(4, 4.05, 4. / 3., 0.)])]);
    // one long rightward swipe over both
    let actions: Vec<Action> = once(Action::Down {
        id: 1,
        at: 3.95,
        pos: (-0.35, 0.),
    })
    .chain((1..=7).map(|k| Action::Move {
        id: 1,
        at: 3.95 + 0.015625 * k as f64,
        pos: (-0.35 + 0.1 * k as f32, 0.),
    }))
    .chain(once(Action::Up { id: 1, at: 4.109375 }))
    .collect();
    let (chart, judge) = play_on(two_flicks(), &actions, &fps(352, 1. / 64.));
    assert_eq!(event_at(&judge, &chart, 4.).judgement, Judgement::Perfect, "{}", events_of(&judge));
    assert_eq!(event_at(&judge, &chart, 4.05).judgement, Judgement::Miss, "{}", events_of(&judge));

    // the same swipe, then back through the second flick: the turn starts a
    // new gesture and judges it
    let mut actions = actions.clone();
    actions.pop();
    let back: Vec<Action> = (1..=6)
        .map(|k| Action::Move {
            id: 1,
            at: 4.0625 + 0.015625 * k as f64,
            pos: (0.35 - 0.1 * k as f32, 0.),
        })
        .chain(once(Action::Up { id: 1, at: 4.203125 }))
        .collect();
    actions.extend(back);
    let (chart, judge) = play_on(two_flicks(), &actions, &fps(352, 1. / 64.));
    assert_eq!(event_at(&judge, &chart, 4.).judgement, Judgement::Perfect, "{}", events_of(&judge));
    assert_eq!(event_at(&judge, &chart, 4.05).judgement, Judgement::Perfect, "{}", events_of(&judge));
}

#[test]
fn flick_windows_close_at_140ms() {
    let swipe_at = |at: f64| {
        vec![
            Action::Down { id: 1, at, pos: (0., 0.) },
            Action::Move {
                id: 1,
                at: at + 0.015625,
                pos: (0.06, 0.),
            },
            Action::Move {
                id: 1,
                at: at + 0.03125,
                pos: (0.12, 0.),
            },
            Action::Up { id: 1, at: at + 0.078125 },
        ]
    };
    let (chart, judge) = play(&swipe_at(5.125), &fps(352, 1. / 64.));
    assert_eq!(event_at(&judge, &chart, 5.).judgement, Judgement::Perfect, "{}", events_of(&judge));

    let (chart, judge) = play(&swipe_at(5.15625), &fps(352, 1. / 64.));
    assert_eq!(event_at(&judge, &chart, 5.).judgement, Judgement::Miss, "{}", events_of(&judge));
}

/// ~500 lines with a few notes each, three moving touches, five seconds of
/// chart at 60fps. Run with `cargo test -p prpr --release -- --ignored`.
#[test]
#[ignore]
fn advance_stays_fast_on_a_large_chart() {
    let lines: Vec<_> = (0..500)
        .map(|l| {
            line_json(
                (0..4)
                    .map(|n| note(1, 0.5 + l as f64 / 100. + n as f64, (l % 17) as f32 / 8. - 1., 0.))
                    .collect(),
            )
        })
        .collect();
    let mut chart = chart_of(lines);
    let env = env();
    let mut judge = Judge::new(&chart);

    let schedule = fps(320, 1. / 64.);
    let mut state: Vec<(u64, f32, f32)> = vec![(1, 0., 0.), (2, 0.2, 0.1), (3, -0.3, -0.2)];
    for (id, x, y) in &mut state {
        let _ = (id, x, y);
    }
    let mut total = std::time::Duration::ZERO;
    let mut prev = f64::NEG_INFINITY;
    for &t in &schedule {
        let mut events: Vec<RawTouch> = Vec::new();
        if prev.is_finite() {
            for (i, entry) in state.iter_mut().enumerate() {
                let id = (i + 1) as u64;
                if prev == 0. {
                    events.push(raw(id, TouchPhase::Started, t, (entry.1, entry.2)));
                } else {
                    entry.1 += 0.001;
                    events.push(raw(id, TouchPhase::Moved, t, (entry.1, entry.2)));
                }
            }
        }
        let mut effects = Vec::new();
        let start = Instant::now();
        judge.update_with(
            &mut chart,
            &env,
            RawFrame {
                t,
                uptime: t,
                speed: 1.,
                aspect: ASPECT,
                dpi: 0,
                raw: &events,
                live: &state,
            },
            0,
            &mut effects,
        );
        total += start.elapsed();
        prev = t;
    }
    let per_frame = total.as_secs_f64() / schedule.len() as f64 * 1e6;
    println!("advance: {per_frame:.1}µs per frame over {} frames", schedule.len());
    eprintln!("advance: {per_frame:.1}µs per frame over {} frames", schedule.len());
}

// keep the windows honest: these bounds are what the tests above rely on
const _: () = {
    assert!(LIMIT_PERFECT < LIMIT_GOOD && LIMIT_GOOD < LIMIT_BAD);
};

#[test]
fn a_replay_reproduces_a_rewind() {
    // The click at 1 expires while the frames run on to 1.3; the clock then
    // rewinds to 1 (a resume) and a late tap lands at 1.05. Live, the note
    // is already a miss. The replay must have processed the same ticks
    // before applying the seek, or the tap would hit it.
    let mut chart = basic_chart();
    let mut judge = Judge::new(&chart);
    drive(&mut judge, &mut chart, &env(), &fps(84, 1. / 64.), &[]);
    let actions = [
        Action::Down {
            id: 1,
            at: 1.046875,
            pos: (0., 0.),
        },
        Action::Up { id: 1, at: 1.125 },
    ];
    let rewound: Vec<f64> = (0..80).map(|it| 1. + it as f64 / 64.).collect();
    drive(&mut judge, &mut chart, &env(), &rewound, &actions);
    assert!(judge.inputs.iter().any(|it| matches!(it, InputEvent::Seek { .. })), "{:?}", judge.inputs);
    assert_eq!(judge.hit_events[0].judgement, Judgement::Miss);

    let mut chart = basic_chart();
    let replayed = Judge::replay(&mut chart, &env(), &judge.inputs, 3.);
    assert_eq!(replayed.hit_events, judge.hit_events, "{}\nvs\n{}", events_of(&replayed), events_of(&judge));
}

#[test]
fn swiping_far_off_a_flick_right_after_pressing_on_it_hits() {
    // #736, the hard case: the first grid point after the press already
    // finds the finger well outside the band; the segment from the press
    // point still crosses the flick
    let (chart, judge) = play(
        &[
            Action::Down {
                id: 1,
                at: 4.990234375,
                pos: (0., 0.),
            },
            Action::Move {
                id: 1,
                at: 5.009765625,
                pos: (0.5, 0.),
            },
            Action::Up { id: 1, at: 5.0625 },
        ],
        &fps(352, 1. / 64.),
    );
    assert_eq!(event_at(&judge, &chart, 5.).judgement, Judgement::Perfect, "{}", events_of(&judge));
}

#[test]
fn touch_down_jitter_is_not_a_flick() {
    // a few pixels of drift a millisecond after touching down must not read
    // as a swipe just because the first step is short
    let (chart, judge) = play(
        &[
            Action::Down {
                id: 1,
                at: 4.9990234375,
                pos: (0., 0.),
            },
            Action::Move {
                id: 1,
                at: 5.,
                pos: (0.003, 0.),
            },
            Action::Up { id: 1, at: 5.0625 },
        ],
        &fps(352, 1. / 64.),
    );
    assert!(!judge.inputs.iter().any(|it| matches!(it, InputEvent::Swipe { .. })), "{:?}", judge.inputs);
    assert_eq!(event_at(&judge, &chart, 5.).judgement, Judgement::Miss, "{}", events_of(&judge));
}

/// The glide these dpi tests use: pressed at 4.921875 on x = -0.1, moving
/// right at 1.6 units/s, so it crosses the flick's x = 0 at 4.984375.
/// Per 1/60 s step that is 0.0267 stream units, 0.237 world units — 9%
/// above the 5u = 0.217 the assumed dpi of 275 demands, and well under the
/// 0.434 of 550: the same gesture is a swipe on one screen and a mere
/// drift on the other.
fn borderline_glide() -> Vec<Action> {
    glide(1, 4.921875, -0.1, 1.6, 5.0625)
}

#[test]
fn a_swipe_slightly_above_the_assumed_dpis_threshold_judges_only_there() {
    let actions = borderline_glide();
    let schedule = fps(352, 1. / 64.);

    let (chart, judge) = play_dpi(&actions, &schedule, 275);
    assert!(
        judge.inputs.iter().any(|it| matches!(it, InputEvent::Swipe { new: true, .. })),
        "the gesture must start at the assumed dpi: {:?}",
        judge.inputs
    );
    assert_eq!(event_at(&judge, &chart, 5.).judgement, Judgement::Perfect, "{}", events_of(&judge));

    let (chart, judge) = play_dpi(&actions, &schedule, 550);
    assert!(!judge.inputs.iter().any(|it| matches!(it, InputEvent::Swipe { .. })), "the doubled threshold must swallow it: {:?}", judge.inputs);
    assert_eq!(event_at(&judge, &chart, 5.).judgement, Judgement::Miss, "{}", events_of(&judge));
}

#[test]
fn an_unreported_dpi_behaves_exactly_like_the_assumed_one() {
    let actions = full_actions();
    let schedule = fps(400, 1. / 64.);
    let (_, unknown) = play(&actions, &schedule);
    let (_, assumed) = play_dpi(&actions, &schedule, 275);
    assert_eq!(unknown.inputs, assumed.inputs);
    assert_eq!(unknown.hit_events, assumed.hit_events);
}

#[test]
fn an_out_of_band_dpi_report_is_clamped() {
    assert_eq!(swipe_dpi(0), 275);
    assert_eq!(swipe_dpi(1), 100);
    assert_eq!(swipe_dpi(99), 100);
    assert_eq!(swipe_dpi(100), 100);
    assert_eq!(swipe_dpi(275), 275);
    assert_eq!(swipe_dpi(420), 420);
    assert_eq!(swipe_dpi(800), 800);
    assert_eq!(swipe_dpi(801), 800);
    assert_eq!(swipe_dpi(u32::MAX), 800);

    // and the collector runs with the clamped value, not the report
    let actions = full_actions();
    let schedule = fps(400, 1. / 64.);
    let (_, low) = play_dpi(&actions, &schedule, 1);
    let (_, bound) = play_dpi(&actions, &schedule, 100);
    assert_eq!(low.inputs, bound.inputs);
    let (_, high) = play_dpi(&actions, &schedule, u32::MAX);
    let (_, bound) = play_dpi(&actions, &schedule, 800);
    assert_eq!(high.inputs, bound.inputs);
}

#[test]
fn the_swipe_threshold_locks_at_the_first_frame() {
    // the first frame reports 275, every later one 550 — a mid-play report
    // change must not lift the threshold the run started with. Without the
    // lock the doubled threshold would swallow the glide's swipe steps.
    let actions = borderline_glide();
    let schedule = fps(352, 1. / 64.);
    let mut chart = basic_chart();
    let mut judge = Judge::new(&chart);
    let mut effects = Vec::new();
    judge.update_with(
        &mut chart,
        &env(),
        RawFrame {
            t: 0.,
            uptime: 0.,
            speed: 1.,
            aspect: ASPECT,
            dpi: 275,
            raw: &[],
            live: &[],
        },
        0,
        &mut effects,
    );
    drive_dpi(&mut judge, &mut chart, &env(), &schedule, &actions, 550);
    let (_, locked) = play_dpi(&actions, &schedule, 275);
    assert!(
        judge.inputs.iter().any(|it| matches!(it, InputEvent::Swipe { new: true, .. })),
        "the 275 threshold must still be in force: {:?}",
        judge.inputs
    );
    assert_eq!(judge.inputs, locked.inputs);
}

/// A recorded stream wrapped in a [`PlayRecord`] a [`ReplayPlayer`] can
/// drive, with a meta matching the test env: 16:9, speed 1, no mods.
fn record_of(inputs: Vec<InputEvent>) -> Arc<PlayRecord> {
    Arc::new(PlayRecord {
        version: PLAY_RECORD_VERSION,
        meta: PlayMeta {
            aspect_ratio: ASPECT,
            speed: 1.,
            ..Default::default()
        },
        notes: Vec::new(),
        inputs,
    })
}

/// A stream with a rewind in it, the shape a resume after a pause produces:
/// the clock runs on to 1.3, rewinds to 1, and a late tap lands at 1.05 —
/// long after the note it aimed at had already missed in live play.
fn rewind_stream() -> Vec<InputEvent> {
    let mut chart = basic_chart();
    let mut judge = Judge::new(&chart);
    drive(&mut judge, &mut chart, &env(), &fps(84, 1. / 64.), &[]);
    let actions = [
        Action::Down {
            id: 1,
            at: 1.046875,
            pos: (0., 0.),
        },
        Action::Up { id: 1, at: 1.125 },
    ];
    let rewound: Vec<f64> = (0..80).map(|it| 1. + it as f64 / 64.).collect();
    drive(&mut judge, &mut chart, &env(), &rewound, &actions);
    assert!(judge.inputs.iter().any(|it| matches!(it, InputEvent::Seek { .. })), "{:?}", judge.inputs);
    judge.inputs
}

/// Drives a recorded stream through the judge the way GameScene replays it:
/// one advance per frame at the given step, the clock jumping to each
/// recorded seek's target as it fires. Frame times never pass `end` and the
/// last one sits exactly on it, so precisely the points a one-shot
/// [`Judge::replay`] to `end` judges are judged, whichever the step.
fn replay_frames(judge: &mut Judge, chart: &mut Chart, player: &mut ReplayPlayer, end: f64, step: f64) {
    let mut t = 0.;
    let mut effects = Vec::new();
    loop {
        let target = (t + step).min(end);
        t = judge.replay_update_with(chart, player, target, &mut effects).unwrap_or(target);
        if t >= end {
            break;
        }
    }
    judge.replay_update_with(chart, player, end, &mut effects);
}

/// Replays `inputs` to `end` entirely through per-frame advances at `step`.
fn replayed_by_frames(inputs: &[InputEvent], end: f64, step: f64) -> Judge {
    let mut chart = basic_chart();
    let mut judge = Judge::new(&chart);
    judge.set_replaying(true);
    let mut player = ReplayPlayer::new(record_of(inputs.to_vec()));
    replay_frames(&mut judge, &mut chart, &mut player, end, step);
    judge
}

#[test]
fn an_incremental_replay_matches_a_one_shot_one() {
    // The full run covers the plain case; the rewind stream covers a seek
    // firing mid-play, where the frame it lands on must stop and hand the
    // clock jump to the driver. Steps finer and coarser than the tick grid,
    // and one off any grid at all, show the result is the stream's, not the
    // frame schedule's.
    let (_, live) = play(&full_actions(), &fps(400, 1. / 64.));
    let cases = [(live.inputs.clone(), 5.5), (rewind_stream(), 3.)];
    for (inputs, end) in cases {
        let mut chart = basic_chart();
        let one_shot = Judge::replay(&mut chart, &env(), &inputs, end);
        for step in [1. / 60., 1. / 144., 0.029, 0.1] {
            let incremental = replayed_by_frames(&inputs, end, step);
            assert_eq!(incremental.hit_events, one_shot.hit_events, "step {step}:\n{}\nvs\n{}", events_of(&incremental), events_of(&one_shot));
            assert_eq!(incremental.result().counts, one_shot.result().counts, "step {step}");
            assert!(incremental.inputs.is_empty(), "a replay must not log a stream");
        }
    }
}

#[test]
fn replay_seek_then_continue_matches_a_one_shot_replay() {
    // Scrubbing throws away the run so far and re-judges from the top to the
    // target; continuing from there must be indistinguishable from never
    // having interrupted the replay — backwards and forwards, before and
    // after a recorded seek.
    let (_, live) = play(&full_actions(), &fps(400, 1. / 64.));
    for (inputs, end, mid) in [
        (live.inputs.clone(), 5.5, 1.7),
        (live.inputs.clone(), 5.5, 4.2),
        (rewind_stream(), 3., 0.8),
        (rewind_stream(), 3., 2.2),
    ] {
        let mut chart = basic_chart();
        let one_shot = Judge::replay(&mut chart, &env(), &inputs, end);

        let mut chart = basic_chart();
        let mut judge = Judge::new(&chart);
        judge.set_replaying(true);
        let mut player = ReplayPlayer::new(record_of(inputs.clone()));
        replay_frames(&mut judge, &mut chart, &mut player, 2.4, 1. / 64.);
        chart.reset();
        judge.replay_seek(&mut chart, &mut player, mid);
        replay_frames(&mut judge, &mut chart, &mut player, end, 1. / 96.);
        assert_eq!(judge.hit_events, one_shot.hit_events, "seek to {mid}:\n{}\nvs\n{}", events_of(&judge), events_of(&one_shot));
        assert_eq!(judge.result().counts, one_shot.result().counts, "seek to {mid}");
        assert!(judge.inputs.is_empty(), "a replay must not log a stream");
    }
}

#[test]
fn replayed_fingers_slide_into_their_next_recorded_move() {
    // A finger pressed at 0.5 that stays put until it moves at tick 20 and
    // again at tick 21: drawn, it holds still until the tick interval before
    // a move, slides across that interval, and is gone once lifted.
    let tick = |it: u32| it as f64 / JUDGE_TICK_HZ;
    let inputs = vec![
        InputEvent::Start {
            id: 0,
            time: 0.5,
            x: 0.,
            y: 0.,
        },
        InputEvent::Move {
            tick: 20,
            points: vec![MovePoint { id: 0, x: 0.3, y: 0. }],
        },
        InputEvent::Move {
            tick: 21,
            points: vec![MovePoint { id: 0, x: 0.6, y: 0.3 }],
        },
        InputEvent::End { id: 0, time: 0.8 },
    ];
    let mut chart = basic_chart();
    let mut judge = Judge::new(&chart);
    judge.set_replaying(true);
    let mut player = ReplayPlayer::new(record_of(inputs));
    let mut effects = Vec::new();
    let mut at = |judge: &mut Judge, t: f64| {
        judge.replay_update_with(&mut chart, &mut player, t, &mut effects);
        judge.replay_touches_at(&player, t)
    };
    let close = |got: Vec<(u8, f32, f32)>, x: f32, y: f32| {
        assert_eq!(got.len(), 1, "{got:?}");
        assert!((got[0].1 - x).abs() < 1e-4 && (got[0].2 - y).abs() < 1e-4, "{got:?} vs ({x}, {y})");
    };
    close(at(&mut judge, 0.55), 0., 0.);
    close(at(&mut judge, (tick(19) + tick(20)) / 2.), 0.15, 0.);
    close(at(&mut judge, (tick(20) + tick(21)) / 2.), 0.45, 0.15);
    // Past the last move nothing lies ahead to slide toward.
    close(at(&mut judge, 0.75), 0.6, 0.3);
    assert!(at(&mut judge, 0.85).is_empty());
}

/// A line spinning at `rev_per_s` from t = 0, drag-free, for chart building.
fn spin_line(rev_per_s: f64, notes: Vec<serde_json::Value>) -> serde_json::Value {
    let end = rev_per_s * 360. * 10.;
    let mut line = line_json(notes);
    line["judgeLineRotateEvents"] = serde_json::json!([
        {"startTime": 0., "endTime": 10., "start": 0., "end": end},
        {"startTime": 10., "endTime": 1000., "start": end, "end": end},
    ]);
    line
}

/// A finger position `r` out at `angle` degrees, in the record space.
fn at_angle(r: f64, angle: f64) -> (f32, f32) {
    let a = angle.to_radians();
    ((r * a.cos()) as f32, (r * a.sin() * ASPECT as f64) as f32)
}

#[test]
fn spinning_drags_judge_across_the_whole_screen() {
    // full-screen drags: a drag on a fast line's centre must judge wherever
    // the finger rests, however the spin aliases with the 30 Hz grid — 960
    // rev/s turns exactly a whole number of half-turns per tick
    let run = |rev_per_s: f64, r: f64, angle: f64| {
        let chart = chart_of(vec![spin_line(rev_per_s, vec![note(2, 4., 0., 0.)])]);
        let actions = [
            Action::Down {
                id: 1,
                at: 3.5,
                pos: at_angle(r, angle),
            },
            Action::Up { id: 1, at: 5.5 },
        ];
        let (chart, judge) = play_on(chart, &actions, &fps(400, 1. / 64.));
        event_at(&judge, &chart, 4.).judgement
    };
    for rev in [15., 30., 90., 960.] {
        for r in [0.3, 0.55] {
            for a in 0..24 {
                let angle = a as f64 * 15.;
                assert_eq!(run(rev, r, angle), Judgement::Perfect, "{rev} rev/s, r={r}, {angle}°");
            }
        }
    }
}

#[test]
fn an_off_centre_spinning_drag_has_a_blind_spot() {
    // the drag sits 0.3 out on a fast line: a finger near the rotation
    // centre never comes within its band and must miss, one outside the
    // note's orbit must hit — the arc test must not judge too wide either
    let run = |r: f64| {
        let chart = chart_of(vec![spin_line(120., vec![note(2, 4., 8. / 3., 0.)])]);
        let actions = [
            Action::Down {
                id: 1,
                at: 3.5,
                pos: at_angle(r, 37.),
            },
            Action::Up { id: 1, at: 5.5 },
        ];
        let (chart, judge) = play_on(chart, &actions, &fps(400, 1. / 64.));
        event_at(&judge, &chart, 4.).judgement
    };
    assert_eq!(run(0.03), Judgement::Miss, "inside the note's orbit");
    assert_eq!(run(0.6), Judgement::Perfect, "outside the note's orbit");
}

#[test]
fn drag_windows_clip_contact_to_their_bounds() {
    // the finger sits in the band only before the window opens, leaving
    // ahead of it: the piece ending at the window's scheduled start must
    // not count — without the clip it would
    let (chart, judge) = play(
        &[
            Action::Down {
                id: 1,
                at: 3.85,
                pos: (0., 0.),
            },
            Action::Move {
                id: 1,
                at: 3.88,
                pos: (0.9, 0.),
            },
            Action::Up { id: 1, at: 4.125 },
        ],
        &fps(320, 1. / 64.),
    );
    assert_eq!(event_at(&judge, &chart, 4.).judgement, Judgement::Miss, "{}", events_of(&judge));

    // and only in the last 20 ms of the window: still a hit, the window's
    // end being a scheduled point is what keeps that last piece whole
    let (chart, judge) = play(
        &[
            Action::Down {
                id: 1,
                at: 4.085,
                pos: (0., 0.),
            },
            Action::Up { id: 1, at: 4.1875 },
        ],
        &fps(320, 1. / 64.),
    );
    assert_eq!(event_at(&judge, &chart, 4.).judgement, Judgement::Perfect, "{}", events_of(&judge));
}

#[test]
fn a_child_line_orbiting_a_spinning_parent_carries_its_drags() {
    // a drag on a child line's centre, the child orbiting a spinning
    // parent: the finger on the orbit's circle must be reached, the sweep
    // splitting the orbit into pieces short enough for the 100 Hz cap
    let mut chart = chart_of(vec![spin_line(5., vec![]), {
        let mut child = line_json(vec![note(2, 4., 0., 0.)]);
        child["judgeLineMoveEvents"] = serde_json::json!([
            {"startTime": 0., "endTime": 0., "start": 0.625, "end": 0.625, "start2": 0.5, "end2": 0.5},
        ]);
        child
    }]);
    chart.lines[1].parent = Some(0);
    chart.lines[1].rot_with_parent = true;
    // the child's origin at the drag's time, by the chart's own transforms
    let tap = {
        let tau = 4.;
        for line in &mut chart.lines {
            line.object.set_time(tau);
        }
        let tr = chart.lines[1].now_transform_with(ASPECT, &chart.lines);
        let origin = tr.transform_point(&Point::new(0., 0.));
        (origin.x, -origin.y * ASPECT)
    };
    let actions = [Action::Down { id: 1, at: 3.5, pos: tap }, Action::Up { id: 1, at: 5.5 }];
    let (chart, judge) = play_on(chart, &actions, &fps(400, 1. / 64.));
    assert_eq!(event_at(&judge, &chart, 4.).judgement, Judgement::Perfect, "{}", events_of(&judge));
}

#[test]
fn spinning_flicks_judge_across_the_screen() {
    // a flick on a fast line's centre, swiped at from wherever: the swept
    // segment-and-arc test sees it however the spin aliases
    // a sustained swipe: every step of it claims a wedge of the line's
    // rotation, and together they tile the whole circle however fast it
    // spins — a single short step at 15 rev/s covers but 90° and legitimately
    // misses when the line never points at the finger
    let run = |rev_per_s: f64, r: f64, angle: f64| {
        let chart = chart_of(vec![spin_line(rev_per_s, vec![note(4, 4., 0., 0.)])]);
        let mut actions = vec![Action::Down {
            id: 1,
            at: 3.921875,
            pos: at_angle(r, angle),
        }];
        for k in 1..=8 {
            actions.push(Action::Move {
                id: 1,
                at: 3.921875 + k as f64 / 64.,
                pos: at_angle(r + 0.12 * k as f64, angle),
            });
        }
        actions.push(Action::Up { id: 1, at: 4.078125 });
        let (chart, judge) = play_on(chart, &actions, &fps(400, 1. / 64.));
        event_at(&judge, &chart, 4.).judgement
    };
    for rev in [15., 960.] {
        for a in 0..12 {
            let angle = a as f64 * 30.;
            assert_eq!(run(rev, 0.4, angle), Judgement::Perfect, "{rev} rev/s, {angle}°");
        }
    }
}

#[test]
fn a_hold_on_a_spinning_line_survives_at_any_line_angle() {
    // the head is a click and must be pressed where the line points — that
    // has always been so and is not what sweeping fixes — but once held,
    // the mid-contact must survive the spin. At 2 rev/s the line points
    // along +x exactly at 3.5, where the head sits
    let chart = chart_of(vec![spin_line(2., vec![note(3, 3.5, 0., 1.)])]);
    let actions = [
        Action::Down {
            id: 1,
            at: 3.5,
            pos: (0.1, 0.),
        },
        Action::Up { id: 1, at: 4.5625 },
    ];
    let (chart, judge) = play_on(chart, &actions, &fps(400, 1. / 64.));
    let hold = event_at(&judge, &chart, 3.5);
    assert_eq!(hold.judgement, Judgement::Perfect, "{}", events_of(&judge));
    assert!(!hold.hold_broken);
}

/// The basic chart plus a spinning line with a full-screen drag at 2.5,
/// for the invariance tests: sweeping must be as replayable, frame-rate
/// independent and filter-independent as the rest.
fn spinning_variant() -> Chart {
    chart_of(vec![
        line_json(vec![note(1, 1., 0., 0.), note(3, 2., 0., 1.), note(2, 4., 0., 0.), note(4, 5., 0., 0.)]),
        spin_line(8., vec![note(2, 2.5, 0., 0.)]),
    ])
}

fn spinning_actions() -> Vec<Action> {
    let mut actions = full_actions();
    actions.extend([
        Action::Down {
            id: 5,
            at: 2.3,
            pos: at_angle(0.45, 53.),
        },
        Action::Up { id: 5, at: 2.7 },
    ]);
    actions
}

#[test]
fn a_replay_reproduces_a_spinning_run_note_for_note() {
    let mut chart = spinning_variant();
    let mut judge = Judge::new(&chart);
    drive(&mut judge, &mut chart, &env(), &fps(400, 1. / 64.), &spinning_actions());
    assert_eq!(event_at(&judge, &chart, 2.5).judgement, Judgement::Perfect, "{}", events_of(&judge));

    let mut chart = spinning_variant();
    let replayed = Judge::replay(&mut chart, &env(), &judge.inputs, 5.5);
    assert_eq!(replayed.hit_events, judge.hit_events, "{}\nvs\n{}", events_of(&replayed), events_of(&judge));
}

#[test]
fn spinning_results_do_not_depend_on_the_frame_rate() {
    let actions = spinning_actions();
    let run = |schedule: &[f64]| {
        let mut chart = spinning_variant();
        let mut judge = Judge::new(&chart);
        drive(&mut judge, &mut chart, &env(), schedule, &actions);
        (judge.inputs, judge.hit_events)
    };
    let (inputs64, hits64) = run(&fps(400, 1. / 64.));
    let (inputs128, hits128) = run(&fps(800, 1. / 128.));
    let (inputs_stall, hits_stall) = run(&stalled());
    assert_eq!(inputs64, inputs128);
    assert_eq!(hits64, hits128, "{}\nvs\n{}", hits_of(&hits64), hits_of(&hits128));
    assert_eq!(inputs64, inputs_stall);
    assert_eq!(hits64, hits_stall);
}
#[test]
fn spinning_candidate_filtering_changes_nothing() {
    let actions = spinning_actions();
    let run = |filter: bool| {
        let mut chart = spinning_variant();
        let mut judge = Judge::new(&chart);
        judge.set_candidate_filter(filter);
        drive(&mut judge, &mut chart, &env(), &fps(400, 1. / 64.), &actions);
        judge.hit_events
    };
    let filtered = run(true);
    let plain = run(false);
    assert_eq!(filtered, plain, "{}\nvs\n{}", hits_of(&filtered), hits_of(&plain));
}

/// The example chart — full-screen drags on lines spinning at 35.6 rev/s —
/// against every finger position, head on. Set `PHIRA_SPIN_CHART` to the
/// unpacked RPE directory to run; skipped otherwise, and the chart itself
/// never enters the repository.
#[test]
#[ignore]
fn the_example_spin_chart_judges_every_drag() {
    let Ok(dir) = std::env::var("PHIRA_SPIN_CHART") else {
        return;
    };
    use crate::{core::NoteKind, parse::parse_rpe};
    let dir = std::path::Path::new(&dir);
    let source = std::fs::read_to_string(dir.join("0362988124350467.json")).unwrap();
    // the chart's own line textures need GL; plain ones judge identically
    let mut json: serde_json::Value = serde_json::from_str(&source).unwrap();
    for line in json["judgeLineList"].as_array_mut().unwrap() {
        line["Texture"] = "line.png".into();
        line["isGif"] = false.into();
    }
    let source = json.to_string();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut fs = crate::fs::fs_from_file(dir).unwrap();
    let (mut total, mut hit) = (0, 0);
    let mut section_cost = 0.;
    for gx in -4..=4 {
        for gy in -2..=2 {
            let pos = (gx as f32 * 0.22, gy as f32 * 0.4);
            let mut chart = rt.block_on(parse_rpe(&source, fs.as_mut(), ChartExtra::default(), false)).unwrap();
            let mut judge = Judge::new(&chart);
            let actions = [Action::Down { id: 1, at: 95.0, pos }, Action::Up { id: 1, at: 103.5 }];
            let schedule: Vec<f64> = (0..).map(|it| 94.0 + it as f64 / 64.).take_while(|t| *t < 104.).collect();
            judge.advance_to(&mut chart, 94.0);
            let start = Instant::now();
            drive(&mut judge, &mut chart, &env(), &schedule, &actions);
            if gx == 4 && gy == 2 {
                section_cost = start.elapsed().as_secs_f64() * 1e6 / schedule.len() as f64;
            }
            for it in &judge.hit_events {
                if (it.line_id == 15 || it.line_id == 16)
                    && matches!(chart.lines[it.line_id as usize].notes[it.note_id as usize].kind, NoteKind::Drag)
                {
                    total += 1;
                    hit += (it.judgement == Judgement::Perfect) as u32;
                }
            }
        }
    }
    eprintln!("spin chart: {section_cost:.1}µs per frame over the spinning section, {hit}/{total} drags");
    assert_eq!(hit, total, "{}/{} drags judged", total - hit, total);
}

/// Contact over an interval counts only the fingers that were actually down
/// during it: a finger lifted at an evaluation point was down over the
/// interval before it, a finger pressed there was not. Checked against a
/// brute-force sampling of the band on a spinning line.
#[test]
fn contact_is_judged_over_the_time_a_finger_was_actually_down() {
    use crate::core::NoteKind;
    let rev = 4.;
    let spin_chart = || {
        let end = rev * 360. * 10.;
        let mut line = line_json(vec![note(2, 4., 0., 0.)]);
        line["judgeLineRotateEvents"] = serde_json::json!([{"startTime": 0., "endTime": 10., "start": 0., "end": end}, {"startTime": 10., "endTime": 1000., "start": end, "end": end}]);
        chart_of(vec![line])
    };
    // ground truth: was the finger inside the drag's band at any instant it
    // was down and the window was open? sampled every 0.1 ms
    let truth = |pos: (f32, f32), down: f64, up: f64| {
        let mut chart = spin_chart();
        let (lo, hi) = (down.max(3.9), up.min(4.1));
        let mut t = lo;
        while t <= hi {
            let line = &mut chart.lines[0];
            line.object.set_time(t);
            let inv = line.now_transform_with(ASPECT, &[]).try_inverse().unwrap();
            let p = inv.transform_point(&Point::new(pos.0, -(pos.1 / ASPECT)));
            let note = &line.notes[0];
            assert!(matches!(note.kind, NoteKind::Drag));
            if (p.x - note.object.translation.0.now()).abs() as f64 <= super::CONTACT_WIDTH {
                return true;
            }
            t += 0.0001;
        }
        false
    };
    for (label, down, up) in [("press in window", 3.915, 4.5), ("lift in window", 3.5, 4.03), ("both", 3.915, 4.03)] {
        let (mut wrong_wide, mut wrong_narrow) = (0, 0);
        for a in 0..120 {
            let a = (a as f64 * 3.).to_radians();
            let pos = ((0.9 * a.cos()) as f32, (0.9 * a.sin() * ASPECT as f64) as f32);
            let mut chart = spin_chart();
            let mut judge = Judge::new(&chart);
            drive(&mut judge, &mut chart, &env(), &fps(400, 1. / 64.), &[Action::Down { id: 1, at: down, pos }, Action::Up { id: 1, at: up }]);
            let got = judge.hit_events.first().is_some_and(|it| it.judgement == Judgement::Perfect);
            let want = truth(pos, down, up);
            if got && !want {
                wrong_wide += 1;
            }
            if !got && want {
                wrong_narrow += 1;
            }
        }
        assert_eq!((wrong_wide, wrong_narrow), (0, 0), "{label}: judged without contact / missed with contact");
    }
}
