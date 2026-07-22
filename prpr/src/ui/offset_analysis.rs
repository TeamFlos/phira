use super::{Scroll, Ui};
use crate::{
    core::{Chart, NoteKind, Resource},
    ext::RectExt,
};
use lyon::math::point;
use macroquad::prelude::*;
use prpr_auto_offset::{estimate_with, AlignConfig, AlignmentResult, AutoOffsetNoteKind, NoteEvent, PreprocessedNoteGaussian, SuperFlux};
use std::{
    borrow::Cow,
    sync::{Arc, Mutex},
};

/// Ratio of graph content width to viewport width.
/// The visible viewport shows `o_range / GRAPH_CONTENT_RATIO` seconds of offset data.
const GRAPH_CONTENT_RATIO: f32 = 2.0;

#[derive(Clone)]
enum OffsetAnalysisState {
    Idle,
    Computing,
    Done(AlignmentResult),
}

pub enum OffsetPanelAction {
    Cancel,
    Reset,
    Save(f32),
}

pub struct OffsetAnalysisPanel {
    state: OffsetAnalysisState,
    requested: bool,
    handle: Option<Arc<Mutex<Option<AlignmentResult>>>>,
    scroll: Scroll,
    scroll_centered: bool,
}

impl Default for OffsetAnalysisPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl OffsetAnalysisPanel {
    pub fn new() -> Self {
        Self {
            state: OffsetAnalysisState::Idle,
            requested: false,
            handle: None,
            scroll: Scroll::new().horizontal(),
            scroll_centered: false,
        }
    }

    pub fn touch(&mut self, touch: &Touch, now: f32) -> bool {
        self.scroll.touch(touch, now)
    }

    pub fn update(&mut self, chart: &Chart, res: &Resource, info_offset: f32, now: f32) {
        if self.requested {
            self.requested = false;
            self.start_analysis(chart, res, info_offset);
        }

        let handle = self.handle.clone();
        if let Some(handle) = handle {
            if let Ok(mut guard) = handle.try_lock() {
                if let Some(result) = guard.take() {
                    self.state = OffsetAnalysisState::Done(result);
                    self.handle = None;
                }
            }
        }

        self.scroll.update(now);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn start_analysis(&mut self, chart: &Chart, res: &Resource, info_offset: f32) {
        use std::thread;

        let note_events = extract_note_events(chart);
        let clip = res.music.clone();
        let pcm: Vec<f32> = clip.frames().iter().map(|f| (f.0 + f.1) / 2.0).collect();
        let sample_rate = clip.sample_rate();
        let config = AlignConfig {
            search_range_sec: 0.30,
            sampling_interval_sec: 0.005,
            search_center_sec: (chart.offset + info_offset) as f64,
        };

        let result_slot: Arc<Mutex<Option<AlignmentResult>>> = Arc::new(Mutex::new(None));
        self.handle = Some(result_slot.clone());

        let _handle = thread::spawn(move || {
            let superflux = SuperFlux::new(&pcm, sample_rate, 2048, 1024);
            let note = PreprocessedNoteGaussian::new(note_events, 0.02);
            let duration = pcm.len() as f64 / sample_rate as f64;
            let result = estimate_with(&superflux, &note, duration, &config);
            if let Ok(mut guard) = result_slot.lock() {
                *guard = Some(result);
            }
        });

        self.state = OffsetAnalysisState::Computing;
        self.scroll_centered = false;
    }

    #[cfg(target_arch = "wasm32")]
    fn start_analysis(&mut self, _chart: &Chart, _res: &Resource, _info_offset: f32) {}

    pub fn render(
        &mut self,
        ui: &mut Ui,
        chart: &Chart,
        info_offset: &mut f32,
        can_adjust: bool,
        labels: &OffsetPanelLabels<'_>,
    ) -> Option<OffsetPanelAction> {
        let mut action = None;
        ui.scope(|ui| {
            let width = 0.55;
            let height = 0.4;
            ui.dx(1. - width - 0.02);
            ui.dy(ui.top - height - 0.02);
            ui.fill_rect(Rect::new(0., 0., width, height), GRAY);
            ui.dy(0.02);
            let r = ui
                .text(labels.adjust_offset.as_ref())
                .pos(width / 2. - 0.03, 0.)
                .anchor(1.0, 0.)
                .size(0.7)
                .no_baseline()
                .draw();
            if ui.button("auto-offset", Rect::new(width / 2. + 0.03, r.top(), r.w, r.h), labels.auto_offset.as_ref())
                && !matches!(self.state, OffsetAnalysisState::Computing)
            {
                self.requested = true;
            }

            ui.dy(0.04 + r.h / 2.);
            let graph_rect = Rect::new(0., 0., width, 0.17 - r.h / 2.);
            self.render_graph_area(ui, chart, *info_offset, graph_rect, &labels);

            ui.dy(0.02);
            let r = ui
                .text(format!("{}ms", (*info_offset * 1000.).round() as i32))
                .pos(width / 2., 0.)
                .anchor(0.5, 0.)
                .size(0.6)
                .no_baseline()
                .draw();
            adjust_offset_buttons(ui, info_offset, can_adjust, width, r.center().y);

            ui.dy(0.07);
            let pad = 0.02;
            let spacing = 0.01;
            let mut r = Rect::new(pad, 0., (width - pad * 2. - spacing * 2.) / 3., 0.06);
            if ui.button("cancel", r, labels.cancel.as_ref()) {
                action = Some(OffsetPanelAction::Cancel);
            }
            r.x += r.w + spacing;
            if ui.button("reset", r, labels.reset.as_ref()) {
                action = Some(OffsetPanelAction::Reset);
            }
            r.x += r.w + spacing;
            if ui.button("save", r, labels.save.as_ref()) {
                action = Some(OffsetPanelAction::Save(*info_offset));
            }
        });
        action
    }

    fn render_graph_area(&mut self, ui: &mut Ui, chart: &Chart, info_offset: f32, graph_rect: Rect, labels: &OffsetPanelLabels<'_>) {
        match self.state.clone() {
            OffsetAnalysisState::Idle => draw_centered_text(ui, graph_rect, labels.analysis_prompt.as_ref()),
            OffsetAnalysisState::Computing => draw_centered_text(ui, graph_rect, labels.analysis_computing.as_ref()),
            OffsetAnalysisState::Done(ref result) => {
                self.scroll.size((graph_rect.w, graph_rect.h));
                let chart_offset = chart.offset;
                self.scroll.render(ui, |ui| {
                    let content_width = graph_rect.w * GRAPH_CONTENT_RATIO;
                    let expanded_rect = Rect::new(0., 0., content_width, graph_rect.h);
                    draw_offset_graph(chart_offset, info_offset, ui, expanded_rect, result);
                    (content_width, graph_rect.h)
                });

                let correction_ms = ((result.offset - chart_offset as f64) * 1000.0).round() as i32;
                ui.text(format!("{correction_ms:+}ms"))
                    .pos(graph_rect.w - 0.01, 0.)
                    .anchor(1.0, 0.0)
                    .size(0.35)
                    .color(Color::new(0.0, 1.0, 0.0, 0.7))
                    .no_baseline()
                    .draw();
                draw_threshold_labels(ui, graph_rect, result);
                self.center_on_recommendation(result, graph_rect.w);
                ui.dy(graph_rect.h);
            }
        }
    }

    fn center_on_recommendation(&mut self, result: &AlignmentResult, width: f32) {
        if self.scroll_centered || result.correlation_curve.is_empty() {
            return;
        }
        let curve = &result.correlation_curve;
        let min_o = curve.first().map(|&(o, _)| o).unwrap_or(0.0);
        let max_o = curve.last().map(|&(o, _)| o).unwrap_or(0.0);
        let o_range = (max_o - min_o).max(1e-6);
        let content_width = width * GRAPH_CONTENT_RATIO;
        let green_x = ((result.offset - min_o) / o_range) as f32 * content_width;
        self.scroll.x_scroller.offset = (green_x - width / 2.0).clamp(0.0, content_width - width);
        self.scroll_centered = true;
    }
}

#[derive(Clone)]
pub struct OffsetPanelLabels<'a> {
    pub adjust_offset: Cow<'a, str>,
    pub auto_offset: Cow<'a, str>,
    pub analysis_prompt: Cow<'a, str>,
    pub analysis_computing: Cow<'a, str>,
    pub cancel: Cow<'a, str>,
    pub reset: Cow<'a, str>,
    pub save: Cow<'a, str>,
}

fn adjust_offset_buttons(ui: &mut Ui, info_offset: &mut f32, can_adjust: bool, width: f32, center_y: f32) {
    let d = 0.14;
    if ui.button("lg_sub", Rect::new(d, center_y, 0., 0.).feather(0.026), "-") && can_adjust {
        *info_offset -= 0.05;
    }
    if ui.button("lg_add", Rect::new(width - d, center_y, 0., 0.).feather(0.026), "+") && can_adjust {
        *info_offset += 0.05;
    }
    let d = 0.08;
    if ui.button("sm_sub", Rect::new(d, center_y, 0., 0.).feather(0.022), "-") && can_adjust {
        *info_offset -= 0.005;
    }
    if ui.button("sm_add", Rect::new(width - d, center_y, 0., 0.).feather(0.022), "+") && can_adjust {
        *info_offset += 0.005;
    }
    let d = 0.03;
    if ui.button("ti_sub", Rect::new(d, center_y, 0., 0.).feather(0.017), "-") && can_adjust {
        *info_offset -= 0.001;
    }
    if ui.button("ti_add", Rect::new(width - d, center_y, 0., 0.).feather(0.017), "+") && can_adjust {
        *info_offset += 0.001;
    }
}

fn draw_centered_text(ui: &mut Ui, rect: Rect, text: &str) {
    ui.dy(rect.h / 2. - 0.03);
    ui.text(text).pos(rect.w / 2., 0.).anchor(0.5, 0.5).size(0.5).no_baseline().draw();
    ui.dy(rect.h / 2. + 0.03);
}

fn draw_threshold_labels(ui: &mut Ui, graph_rect: Rect, result: &AlignmentResult) {
    let s_top = offset_graph_score_top(result);
    let v_pad = 0.08;
    let inner_y = graph_rect.h * v_pad;
    let inner_h = graph_rect.h * (1.0 - 2.0 * v_pad);
    for (value, label) in [(0.2_f32, "0.2"), (0.6_f32, "0.6")] {
        if value > s_top {
            continue;
        }
        let y = inner_y + (1.0 - value / s_top) * inner_h;
        ui.text(label)
            .pos(0.01, y + inner_h * 0.015)
            .anchor(0.0, 0.0)
            .size(0.22)
            .color(Color::new(1.0, 0.82, 0.32, 0.62))
            .no_baseline()
            .draw();
    }
}

fn offset_graph_score_top(result: &AlignmentResult) -> f32 {
    result.correlation_curve.iter().map(|&(_, s)| s).fold(0.0, f32::max).max(0.25)
}

fn draw_offset_graph(chart_offset: f32, info_offset: f32, ui: &mut Ui, rect: Rect, result: &AlignmentResult) {
    let curve = &result.correlation_curve;
    if curve.is_empty() {
        return;
    }

    let min_o = curve.first().map(|&(o, _)| o).unwrap_or(0.0);
    let max_o = curve.last().map(|&(o, _)| o).unwrap_or(0.0);
    let o_range = (max_o - min_o).max(1e-6);
    let s_top = offset_graph_score_top(result);

    ui.fill_rect(rect, Color::new(0.0, 0.0, 0.0, 0.3));

    let v_pad = 0.08;
    let inner = Rect::new(rect.x, rect.y + rect.h * v_pad, rect.w, rect.h * (1.0 - 2.0 * v_pad));
    let line_w = rect.w * 0.003;

    for value in [0.2_f32, 0.6_f32] {
        if value > s_top {
            continue;
        }
        let y = inner.y + (1.0 - value / s_top) * inner.h;
        let mut mb = lyon::path::Path::builder();
        mb.begin(point(inner.x, y));
        mb.line_to(point(inner.x + inner.w, y));
        mb.end(false);
        ui.stroke_path(&mb.build(), line_w, Color::new(1.0, 0.82, 0.32, 0.26));
    }

    let max_pts = 70usize;
    let step = ((curve.len() as f64) / (max_pts as f64)).ceil() as usize;
    let mut path_builder = lyon::path::Path::builder();
    let mut first = true;
    for i in (0..curve.len()).step_by(step) {
        let (o, s) = curve[i];
        let x = inner.x + ((o - min_o) / o_range) as f32 * inner.w;
        let y = inner.y + (1.0 - (s / s_top).clamp(0.0, 1.0)) * inner.h;
        if first {
            path_builder.begin(point(x, y));
            first = false;
        } else {
            path_builder.line_to(point(x, y));
        }
    }
    path_builder.end(false);
    ui.stroke_path(&path_builder.build(), line_w, Color::new(0.6, 0.6, 0.6, 0.6));

    let marker_line_w = line_w * 1.5;
    draw_offset_marker(ui, inner, min_o, o_range, chart_offset as f64, marker_line_w, Color::new(1.0, 0.5, 0.0, 0.5));
    draw_offset_marker(ui, inner, min_o, o_range, result.offset, marker_line_w, Color::new(0.0, 1.0, 0.0, 0.5));
    draw_offset_marker(ui, inner, min_o, o_range, (chart_offset + info_offset) as f64, marker_line_w, Color::new(0.0, 0.5, 1.0, 0.5));
}

fn draw_offset_marker(ui: &mut Ui, inner: Rect, min_o: f64, o_range: f64, offset: f64, width: f32, color: Color) {
    let max_o = min_o + o_range;
    if offset < min_o || offset > max_o {
        return;
    }
    let x = inner.x + ((offset - min_o) / o_range) as f32 * inner.w;
    let mut mb = lyon::path::Path::builder();
    mb.begin(point(x, inner.y));
    mb.line_to(point(x, inner.y + inner.h));
    mb.end(false);
    ui.stroke_path(&mb.build(), width, color);
}

fn extract_note_events(chart: &Chart) -> Vec<NoteEvent> {
    let mut notes: Vec<NoteEvent> = chart
        .lines
        .iter()
        .flat_map(|line| line.notes.iter())
        .filter(|note| !note.fake && note.time >= 0.0)
        .map(|note| NoteEvent::new(note.time, auto_offset_note_kind(&note.kind)))
        .collect();
    notes.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());
    notes
}

fn auto_offset_note_kind(kind: &NoteKind) -> AutoOffsetNoteKind {
    match kind {
        NoteKind::Click => AutoOffsetNoteKind::Tap,
        NoteKind::Hold { .. } => AutoOffsetNoteKind::Hold,
        NoteKind::Flick => AutoOffsetNoteKind::Flick,
        NoteKind::Drag => AutoOffsetNoteKind::Drag,
    }
}
