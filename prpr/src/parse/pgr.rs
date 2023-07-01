crate::tl_file!("parser" ptl);

use super::process_lines;
use crate::{
    core::{
        Anim, AnimFloat, AnimVector, BpmList, Chart, ChartExtra, ChartSettings, JudgeLine, JudgeLineCache, JudgeLineKind, Keyframe, Note, NoteKind,
        Object, HEIGHT_RATIO,
    },
    ext::NotNanExt,
    judge::JudgeStatus,
};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::cell::RefCell;
use tracing::warn;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PgrEvent {
    pub start_time: f32,
    pub end_time: f32,
    pub start: f32,
    pub end: f32,
    #[serde(default)]
    pub start2: f32,
    #[serde(default)]
    pub end2: f32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PgrSpeedEvent {
    pub start_time: f32,
    pub end_time: f32,
    pub value: f32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PgrNote {
    #[serde(rename = "type")]
    kind: u8,
    time: f32,
    position_x: f32,
    hold_time: f32,
    speed: f32,
    floor_position: f32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PgrJudgeLine {
    bpm: f32,
    #[serde(rename = "judgeLineDisappearEvents")]
    alpha_events: Vec<PgrEvent>,
    #[serde(rename = "judgeLineRotateEvents")]
    rotate_events: Vec<PgrEvent>,
    #[serde(rename = "judgeLineMoveEvents")]
    move_events: Vec<PgrEvent>,
    speed_events: Vec<PgrSpeedEvent>,

    notes_above: Vec<PgrNote>,
    notes_below: Vec<PgrNote>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PgrChart {
    offset: f32,
    judge_line_list: Vec<PgrJudgeLine>,
}

macro_rules! validate_events {
    ($pgr:expr) => {
        $pgr.retain(|it| {
            if it.start_time > it.end_time {
                warn!("invalid time range, ignoring");
                false
            } else {
                true
            }
        });
        for i in 0..($pgr.len() - 1) {
            if $pgr[i].end_time != $pgr[i + 1].start_time {
                ptl!(bail "event-not-contiguous");
            }
        }
        // if $pgr.last().unwrap().end_time <= 900000000.0 {
        // bail!("End time is not great enough ({})", $pgr.last().unwrap().end_time);
        // }
    };
}

fn parse_speed_events(r: f32, mut pgr: Vec<PgrSpeedEvent>, max_time: f32) -> Result<(AnimFloat, AnimFloat)> {
    validate_events!(pgr);
    assert_eq!(pgr[0].start_time, 0.0);
    let mut kfs = Vec::new();
    let mut pos = 0.;
    kfs.extend(pgr[..pgr.len().saturating_sub(1)].iter().map(|it| {
        let from_pos = pos;
        pos += (it.end_time - it.start_time) * r * it.value;
        Keyframe::new(it.start_time * r, from_pos, 2)
    }));
    let last = pgr.last().unwrap();
    kfs.push(Keyframe::new(last.start_time * r, pos, 2));
    kfs.push(Keyframe::new(max_time, pos + (max_time - last.start_time * r) * last.value, 0));
    for kf in &mut kfs {
        kf.value /= HEIGHT_RATIO;
    }
    Ok((AnimFloat::new(pgr.iter().map(|it| Keyframe::new(it.start_time * r, it.value, 0)).collect()), AnimFloat::new(kfs)))
}

fn parse_float_events(r: f32, mut pgr: Vec<PgrEvent>) -> Result<AnimFloat> {
    validate_events!(pgr);
    let mut kfs = Vec::<Keyframe<f32>>::new();
    for e in pgr {
        if !kfs.last().map_or(false, |it| it.value == e.start) {
            kfs.push(Keyframe::new((e.start_time * r).max(0.), e.start, 2));
        }
        kfs.push(Keyframe::new(e.end_time * r, e.end, 2));
    }
    kfs.pop();
    Ok(AnimFloat::new(kfs))
}

fn parse_move_events(r: f32, mut pgr: Vec<PgrEvent>) -> Result<AnimVector> {
    validate_events!(pgr);
    let mut kf1 = Vec::<Keyframe<f32>>::new();
    let mut kf2 = Vec::<Keyframe<f32>>::new();
    for e in pgr {
        let st = (e.start_time * r).max(0.);
        let en = e.end_time * r;
        if !kf1.last().map_or(false, |it| it.value == e.start) {
            kf1.push(Keyframe::new(st, e.start, 2));
        }
        if !kf2.last().map_or(false, |it| it.value == e.start2) {
            kf2.push(Keyframe::new(st, e.start2, 2));
        }
        kf1.push(Keyframe::new(en, e.end, 2));
        kf2.push(Keyframe::new(en, e.end2, 2));
    }
    kf1.pop();
    kf2.pop();
    for kf in &mut kf1 {
        kf.value = -1. + kf.value * 2.;
    }
    for kf in &mut kf2 {
        kf.value = -1. + kf.value * 2.;
    }
    Ok(AnimVector(AnimFloat::new(kf1), AnimFloat::new(kf2)))
}

fn parse_notes(r: f32, mut pgr: Vec<PgrNote>, speed: &mut AnimFloat, height: &mut AnimFloat, above: bool) -> Result<Vec<Note>> {
    // is_sorted is unstable...
    if pgr.is_empty() {
        return Ok(Vec::new());
    }
    pgr.sort_by_key(|it| it.time.not_nan());
    pgr.into_iter()
        .map(|pgr| {
            let time = pgr.time * r;
            Ok(Note {
                object: Object {
                    translation: AnimVector(AnimFloat::fixed(pgr.position_x * (2. * 9. / 160.)), AnimFloat::default()),
                    ..Default::default()
                },
                kind: match pgr.kind {
                    1 => NoteKind::Click,
                    2 => NoteKind::Drag,
                    3 => {
                        let end_time = (pgr.time + pgr.hold_time) * r;
                        height.set_time(end_time);
                        let end_height = height.now();
                        NoteKind::Hold { end_time, end_height }
                    }
                    4 => NoteKind::Flick,
                    _ => ptl!(bail "unknown-note-type", "type" => pgr.kind),
                },
                time,
                speed: if pgr.kind == 3 {
                    speed.set_time(time);
                    pgr.speed / speed.now()
                } else {
                    pgr.speed
                },
                height: pgr.floor_position / HEIGHT_RATIO,

                above,
                multiple_hint: false,
                fake: false,
                judge: JudgeStatus::NotJudged,
            })
        })
        .collect()
}

fn parse_judge_line(pgr: PgrJudgeLine, max_time: f32) -> Result<JudgeLine> {
    let r = 60. / pgr.bpm / 32.;
    let (mut speed, mut height) = parse_speed_events(r, pgr.speed_events, max_time).context("Failed to parse speed events")?;
    let notes_above = parse_notes(r, pgr.notes_above, &mut speed, &mut height, true).context("Failed to parse notes above")?;
    let mut notes_below = parse_notes(r, pgr.notes_below, &mut speed, &mut height, false).context("Failed to parse notes below")?;
    let mut notes = notes_above;
    notes.append(&mut notes_below);
    let cache = JudgeLineCache::new(&mut notes);
    Ok(JudgeLine {
        object: Object {
            alpha: parse_float_events(r, pgr.alpha_events).with_context(|| ptl!("alpha-events-parse-failed"))?,
            rotation: parse_float_events(r, pgr.rotate_events).with_context(|| ptl!("rotate-events-parse-failed"))?,
            translation: parse_move_events(r, pgr.move_events).with_context(|| ptl!("move-events-parse-failed"))?,
            ..Default::default()
        },
        ctrl_obj: RefCell::default(),
        kind: JudgeLineKind::Normal,
        height,
        incline: AnimFloat::default(),
        notes,
        color: Anim::default(),
        parent: None,
        z_index: 0,
        show_below: true,
        attach_ui: None,

        cache,
    })
}

pub fn parse_phigros(source: &str, extra: ChartExtra) -> Result<Chart> {
    let pgr: PgrChart = serde_json::from_str(source).with_context(|| ptl!("json-parse-failed"))?;
    let max_time = *pgr
        .judge_line_list
        .iter()
        .map(|line| {
            line.notes_above
                .iter()
                .chain(line.notes_below.iter())
                .map(|note| note.time.not_nan())
                .max()
                .unwrap_or_default()
                * (60. / line.bpm / 32.)
        })
        .max()
        .unwrap_or_default()
        + 1.;
    let mut lines = pgr
        .judge_line_list
        .into_iter()
        .enumerate()
        .map(|(id, pgr)| parse_judge_line(pgr, max_time).with_context(|| ptl!("judge-line-location", "jlid" => id)))
        .collect::<Result<Vec<_>>>()?;
    process_lines(&mut lines);
    Ok(Chart::new(pgr.offset, lines, BpmList::default(), ChartSettings::default(), extra))
}
