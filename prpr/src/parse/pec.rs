crate::tl_file!("parser" ptl);

use super::{process_lines, RPE_TWEEN_MAP};
use crate::{
    core::{
        Anim, AnimFloat, AnimVector, BpmList, Chart, ChartExtra, ChartSettings, JudgeLine, JudgeLineCache, JudgeLineKind, Keyframe, Note, NoteKind,
        Object, TweenId, EPS,
    },
    ext::NotNanExt,
    judge::{HitSound, JudgeStatus},
};
use anyhow::{bail, Context, Result};
use std::{cell::RefCell, collections::HashMap};
use tracing::warn;

trait Take {
    fn take_f32(&mut self) -> Result<f32>;
    fn take_usize(&mut self) -> Result<usize>;
    fn take_tween(&mut self) -> Result<TweenId>;
    fn take_time(&mut self, r: &mut BpmList) -> Result<f32>;
}

impl<'a, T: Iterator<Item = &'a str>> Take for T {
    fn take_f32(&mut self) -> Result<f32> {
        self.next()
            .ok_or_else(|| ptl!(err "unexpected-eol"))
            .and_then(|it| -> Result<f32> { Ok(it.parse()?) })
            .with_context(|| ptl!("expected-f32"))
    }

    fn take_usize(&mut self) -> Result<usize> {
        self.next()
            .ok_or_else(|| ptl!(err "unexpected-eol"))
            .and_then(|it| -> Result<usize> { Ok(it.parse()?) })
            .with_context(|| ptl!("expected-usize"))
    }

    fn take_tween(&mut self) -> Result<TweenId> {
        self.next()
            .ok_or_else(|| ptl!(err "unexpected-eol"))
            .and_then(|it| -> Result<u8> {
                let t = it.parse::<u8>()?;
                Ok(RPE_TWEEN_MAP.get(t as usize).copied().unwrap_or(RPE_TWEEN_MAP[0]))
            })
            .with_context(|| ptl!("expected-tween"))
    }

    fn take_time(&mut self, r: &mut BpmList) -> Result<f32> {
        self.take_f32().map(|it| r.time_beats(it))
    }
}

struct PECEvent {
    start_time: f32,
    end_time: f32,
    end: f32,
    easing: TweenId,
}

impl PECEvent {
    pub fn new(start_time: f32, end_time: f32, end: f32, tween: TweenId) -> Self {
        Self {
            start_time,
            end_time,
            end,
            easing: tween,
        }
    }

    pub fn single(time: f32, value: f32) -> Self {
        Self::new(time, time, value, 0)
    }
}

#[derive(Default)]
struct PECJudgeLine {
    speed_events: Vec<(f32, f32)>,
    alpha_events: Vec<PECEvent>,
    move_events: (Vec<PECEvent>, Vec<PECEvent>),
    rotate_events: Vec<PECEvent>,
    notes: Vec<Note>,
}

fn sanitize_events(events: &mut [PECEvent], id: usize, desc: &str) {
    events.sort_by_key(|e| (e.end_time.not_nan(), e.start_time.not_nan()));
    let mut last_start = 0.0;
    let mut last_end = f32::NEG_INFINITY;
    for e in events.iter_mut() {
        if e.start_time < last_end {
            warn!(
                judge_line = id,
                "Overlap detected in {desc} events: [{last_start}, {last_end}) and [{}, {}). Clipping the last one to [{last_end}, {})",
                e.start_time,
                e.end_time,
                e.end_time
            );
            e.start_time = last_end;
        }
        last_start = e.start_time;
        last_end = e.end_time;
    }
}

fn parse_events(mut events: Vec<PECEvent>, id: usize, desc: &str) -> Result<AnimFloat> {
    sanitize_events(&mut events, id, desc);
    let mut kfs = Vec::new();
    for e in events {
        if e.start_time == e.end_time {
            kfs.push(Keyframe::new(e.start_time, e.end, 0));
        } else {
            if kfs.is_empty() {
                bail!("failed to parse {desc} events: interpolating event found before a concrete value appears");
            }
            assert!(!kfs.is_empty());
            kfs.push(Keyframe::new(e.start_time, kfs.last().unwrap().value, e.easing));
            kfs.push(Keyframe::new(e.end_time, e.end, 0));
        }
    }
    Ok(AnimFloat::new(kfs))
}

fn parse_speed_events(mut pec: Vec<(f32, f32)>, max_time: f32) -> AnimFloat {
    if pec[0].0 >= EPS {
        pec.insert(0, (0., 0.));
    }
    let mut kfs = Vec::new();
    let mut height = 0.0;
    let mut last_time = 0.0;
    let mut last_speed = 0.0;
    for (time, speed) in pec {
        height += (time - last_time) * last_speed;
        kfs.push(Keyframe::new(time, height, 2));
        last_time = time;
        last_speed = speed;
    }
    kfs.push(Keyframe::new(max_time, height + (max_time - last_time) * last_speed, 0));
    AnimFloat::new(kfs)
}

fn parse_judge_line(mut pec: PECJudgeLine, id: usize, max_time: f32) -> Result<JudgeLine> {
    let mut height = parse_speed_events(pec.speed_events, max_time);
    let mut process_notes = |notes: &mut Vec<Note>| {
        for note in notes {
            height.set_time(note.time);
            note.height = height.now();
            if let NoteKind::Hold { end_time, end_height } = &mut note.kind {
                height.set_time(*end_time);
                *end_height = height.now();
            }
        }
    };
    pec.move_events.0.iter_mut().for_each(|it| it.end = it.end / 2048. * 2. - 1.);
    pec.move_events.1.iter_mut().for_each(|it| it.end = it.end / 1400. * 2. - 1.);
    pec.alpha_events.iter_mut().for_each(|it| {
        if it.end >= 0.0 {
            it.end /= 255.;
        }
    });
    process_notes(&mut pec.notes);
    let cache = JudgeLineCache::new(&mut pec.notes);
    Ok(JudgeLine {
        object: Object {
            alpha: parse_events(pec.alpha_events, id, "alpha")?,
            translation: AnimVector(parse_events(pec.move_events.0, id, "move X")?, parse_events(pec.move_events.1, id, "move Y")?),
            rotation: parse_events(pec.rotate_events, id, "rotate")?,
            scale: AnimVector(AnimFloat::fixed(3.91 / 6.), AnimFloat::default()),
        },
        ctrl_obj: RefCell::default(),
        kind: JudgeLineKind::Normal,
        height,
        incline: AnimFloat::default(),
        notes: pec.notes,
        color: Anim::default(),
        parent: None,
        z_index: 0,
        show_below: false,
        attach_ui: None,

        cache,
    })
}

pub fn parse_pec(source: &str, extra: ChartExtra) -> Result<Chart> {
    let mut offset = None;
    let mut r = None;
    let mut lines = Vec::new();
    let mut bpm_list = Vec::new();
    let mut last_line = None;
    fn get_line(lines: &mut Vec<PECJudgeLine>, id: usize) -> &mut PECJudgeLine {
        if lines.len() <= id {
            lines.reserve(id - lines.len() + 1);
            for _ in 0..=(id - lines.len()) {
                lines.push(PECJudgeLine::default());
            }
        }
        &mut lines[id]
    }
    fn ensure_bpm<'a>(r: &'a mut Option<BpmList>, bpm_list: &mut Vec<(f32, f32)>) -> &'a mut BpmList {
        if r.is_none() {
            *r = Some(BpmList::new(std::mem::take(bpm_list)));
        }
        r.as_mut().unwrap()
    }
    macro_rules! bpm {
        () => {
            ensure_bpm(&mut r, &mut bpm_list)
        };
    }
    macro_rules! last_note {
        () => {{
            let Some(last_line) = last_line else {
                                                        ptl!(bail "no-notes-inserted");
                                                    };
            lines[last_line].notes.last_mut().unwrap()
        }};
    }
    let mut inner = |line: &str| -> Result<()> {
        let mut it = line.split_whitespace();
        if offset.is_none() {
            offset = Some(it.take_f32()? / 1000. - 0.15);
        } else {
            let Some(cmd) = it.next() else {
                return Ok(());
            };
            let cs: Vec<_> = cmd.chars().collect();
            if cs.len() > 2 {
                ptl!(bail "unknown-command", "cmd" => cmd);
            }
            match cs[0] {
                'b' if cmd == "bp" => {
                    if r.is_some() {
                        ptl!(bail "bp-error");
                    }
                    bpm_list.push((it.take_f32()?, it.take_f32()?));
                }
                'n' if cs.len() == 2 && ('1'..='4').contains(&cs[1]) => {
                    let r = bpm!();
                    let line = it.take_usize()?;
                    last_line = Some(line);
                    let line = get_line(&mut lines, line);
                    let time = it.take_time(r)?;
                    let kind = match cs[1] {
                        '1' => NoteKind::Click,
                        '2' => NoteKind::Hold {
                            end_time: it.take_time(r)?,
                            end_height: 0.0,
                        },
                        '3' => NoteKind::Flick,
                        '4' => NoteKind::Drag,
                        _ => unreachable!(),
                    };
                    let position_x = it.take_f32()? / 1024.;
                    // TODO we don't understand..
                    let above = it.take_usize()? == 1;
                    let fake = match it.take_usize()? {
                        0 => false,
                        1 => true,
                        _ => ptl!(bail "expected-01"),
                    };
                    let hitsound = HitSound::default_from_kind(&kind);
                    line.notes.push(Note {
                        object: Object {
                            translation: AnimVector(AnimFloat::fixed(position_x), AnimFloat::default()),
                            ..Default::default()
                        },
                        kind,
                        hitsound,
                        time,
                        height: 0.0,
                        speed: 1.0,

                        above,
                        multiple_hint: false,
                        fake,
                        judge: JudgeStatus::NotJudged,
                    });
                    if it.next() == Some("#") {
                        last_note!().speed = it.take_f32()?;
                    }
                    if it.next() == Some("&") {
                        let note = last_note!();
                        let size = it.take_f32()?;
                        if (size - 1.0).abs() >= EPS {
                            note.object.scale.0 = AnimFloat::fixed(size);
                        }
                    }
                }
                '#' if cs.len() == 1 => {
                    last_note!().speed = it.take_f32()?;
                }
                '&' if cs.len() == 1 => {
                    let note = last_note!();
                    let size = it.take_f32()?;
                    if (size - 1.0).abs() >= EPS {
                        note.object.scale.0 = AnimFloat::fixed(size);
                    }
                }
                'c' if cs.len() == 2 => {
                    let r = bpm!();
                    let line = get_line(&mut lines, it.take_usize()?);
                    let time = it.take_time(r)?;
                    match cs[1] {
                        'v' => {
                            line.speed_events.push((time, it.take_f32()? / 5.85));
                        }
                        'p' => {
                            let x = it.take_f32()?;
                            let y = it.take_f32()?;
                            line.move_events.0.push(PECEvent::single(time, x));
                            line.move_events.1.push(PECEvent::single(time, y));
                        }
                        'd' => {
                            line.rotate_events.push(PECEvent::single(time, -it.take_f32()?));
                        }
                        'a' => {
                            line.alpha_events.push(PECEvent::single(time, it.take_f32()?));
                        }
                        'm' => {
                            let end_time = it.take_time(r)?;
                            let x = it.take_f32()?;
                            let y = it.take_f32()?;
                            let t = it.take_tween()?;
                            line.move_events.0.push(PECEvent::new(time, end_time, x, t));
                            line.move_events.1.push(PECEvent::new(time, end_time, y, t));
                        }
                        'r' => {
                            line.rotate_events
                                .push(PECEvent::new(time, it.take_time(r)?, -it.take_f32()?, it.take_tween()?));
                        }
                        'f' => {
                            line.alpha_events.push(PECEvent::new(time, it.take_time(r)?, it.take_f32()?, 2));
                        }
                        _ => ptl!(bail "unknown-command", "cmd" => cmd),
                    }
                }
                _ => ptl!(bail "unknown-command", "cmd" => cmd),
            }
        }
        if let Some(next) = it.next() {
            ptl!(bail "unexpected-extra", "next" => next);
        }
        Ok(())
    };
    for (id, line) in source.lines().enumerate() {
        inner(line).with_context(|| ptl!("line-location", "lid" => id + 1))?;
    }
    let max_time = *lines
        .iter()
        .map(|it| {
            it.alpha_events
                .iter()
                .chain(it.rotate_events.iter())
                .chain(it.move_events.0.iter())
                .chain(it.move_events.1.iter())
                .map(|it| it.end_time.not_nan())
                .chain(it.speed_events.iter().map(|it| it.0.not_nan()))
                .chain(it.notes.iter().map(|it| it.time.not_nan()))
                .max()
                .unwrap_or_default()
        })
        .max()
        .unwrap_or_default()
        + 1.;
    let mut lines = lines
        .into_iter()
        .enumerate()
        .map(|(id, line)| parse_judge_line(line, id, max_time).with_context(|| ptl!("judge-line-location", "jlid" => id)))
        .collect::<Result<Vec<_>>>()?;
    process_lines(&mut lines);
    ensure_bpm(&mut r, &mut bpm_list);
    Ok(Chart::new(
        offset.unwrap(),
        lines,
        r.unwrap(),
        ChartSettings {
            pe_alpha_extension: true,
            ..Default::default()
        },
        extra,
        HashMap::new(),
    ))
}
