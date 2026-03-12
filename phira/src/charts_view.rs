prpr_l10n::tl_file!("charts_view");

use crate::{
    client::{Chart, ChartRef},
    dir, get_data, get_data_mut,
    icons::Icons,
    page::{ChartItem, ChartType, Fader, Illustration, CHOOSE_COVER, CHOSEN_COVER},
    popup::Popup,
    save_data,
    scene::{render_release_to_refresh, SongScene, MP_PANEL},
};
use anyhow::Result;
use core::f32;
use macroquad::prelude::*;
use prpr::{
    core::{Tweenable, BOLD_FONT},
    ext::{semi_black, RectExt, SafeTexture},
    scene::{show_message, NextScene},
    ui::{button_hit, button_hit_large, DRectButton, LongTouchState, Scroll, Ui},
};
use std::{
    ops::Range,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

pub static NEED_UPDATE: AtomicBool = AtomicBool::new(false);

const CHART_PADDING: f32 = 0.013;
const TRANSIT_TIME: f32 = 0.4;
const BACK_FADE_IN_TIME: f32 = 0.2;

pub struct ChartDisplayItem {
    pub chart: Option<ChartItem>,
    symbol: Option<char>,
    btn: DRectButton,
    long_touch: LongTouchState,
}

impl ChartDisplayItem {
    pub fn new(chart: Option<ChartItem>, symbol: Option<char>) -> Self {
        Self {
            chart,
            symbol,
            btn: DRectButton::new(),
            long_touch: LongTouchState::default(),
        }
    }

    pub fn from_remote(chart: &Chart) -> Self {
        Self::new(
            Some(ChartItem {
                info: chart.to_info(),
                illu: Illustration::from_file_thumbnail(chart.illustration.clone()),
                local_path: None,
                chart_type: ChartType::Downloaded,
            }),
            if chart.stable_request {
                Some('+')
            } else if !chart.reviewed {
                Some('*')
            } else {
                None
            },
        )
    }
}

struct TransitState {
    id: u32,
    rect: Option<Rect>,
    chart: ChartItem,
    start_time: f32,
    next_scene: Option<NextScene>,
    back: bool,
    done: bool,
    delete: bool,
}

pub struct ChartsView {
    scroll: Scroll,
    fader: Fader,

    icons: Arc<Icons>,
    rank_icons: [SafeTexture; 8],

    back_fade_in: Option<(u32, f32)>,

    transit: Option<TransitState>,
    pub charts: Option<Vec<ChartDisplayItem>>,

    pub row_num: u32,
    pub row_height: f32,

    pub can_refresh: bool,

    pub clicked_special: bool,

    pub allow_edit: bool,
    editing_chart: Option<usize>,
    chart_menu: Popup,
    need_show_chart_menu: bool,
    edit_move_state: Option<bool>,
    movement: Option<(usize, usize)>,

    pub multi_select: Option<Vec<ChartRef>>,
}

impl ChartsView {
    pub fn new(icons: Arc<Icons>, rank_icons: [SafeTexture; 8]) -> Self {
        Self {
            scroll: Scroll::new(),
            fader: Fader::new().with_distance(0.06),

            icons,
            rank_icons,

            back_fade_in: None,

            transit: None,
            charts: None,

            row_num: 4,
            row_height: 0.3,

            can_refresh: true,

            clicked_special: false,

            allow_edit: false,
            editing_chart: None,
            chart_menu: Popup::new(),
            need_show_chart_menu: false,
            edit_move_state: None,
            movement: None,

            multi_select: None,
        }
    }

    pub fn allow_edit(&mut self, allow: bool) {
        self.allow_edit = allow;
        if !allow {
            self.edit_move_state = None;
            self.movement = None;
        }
    }

    pub fn take_movement(&mut self) -> Option<(usize, usize)> {
        self.movement.take()
    }

    fn charts_display_range(&self, content_size: (f32, f32)) -> Range<u32> {
        let sy = self.scroll.y_scroller.offset;
        let start_line = (sy / self.row_height) as u32;
        let end_line = ((sy + content_size.1) / self.row_height).ceil() as u32;
        (start_line * self.row_num)..((end_line + 1) * self.row_num)
    }

    pub fn clear(&mut self) {
        self.charts = None;
    }

    pub fn set(&mut self, t: f32, charts: Vec<ChartDisplayItem>) {
        self.charts = Some(charts);
        self.fader.sub(t);
    }

    pub fn reset_scroll(&mut self) {
        self.scroll.y_scroller.reset();
    }

    pub fn transiting(&self) -> bool {
        self.transit.is_some()
    }

    pub fn on_result(&mut self, t: f32, delete: bool) {
        if let Some(transit) = &mut self.transit {
            transit.start_time = t;
            transit.back = true;
            transit.done = false;
            transit.delete = delete;
        }
    }

    pub fn need_update(&self) -> bool {
        NEED_UPDATE.fetch_and(false, Ordering::Relaxed)
    }

    pub fn touch(&mut self, touch: &Touch, t: f32, rt: f32) -> Result<bool> {
        if self.chart_menu.showing() {
            self.chart_menu.touch(touch, t);
            return Ok(true);
        }
        if self.scroll.touch(touch, t) {
            return Ok(true);
        }
        if !self.scroll.contains(touch) {
            return Ok(false);
        }
        let mut movement = None;
        if let Some(charts) = &mut self.charts {
            for (id, item) in charts.iter_mut().enumerate() {
                if let Some(chart) = &item.chart {
                    if item.btn.touch(touch, t) {
                        item.long_touch.reset();
                        let handled_by_mp = MP_PANEL.with(|it| {
                            if let Some(panel) = it.borrow_mut().as_mut() {
                                if panel.in_room() {
                                    if let Some(id) = chart.info.id {
                                        panel.select_chart(id);
                                        panel.show(rt);
                                    } else {
                                        use crate::mp::{mtl, L10N_LOCAL};
                                        show_message(mtl!("select-chart-local")).error();
                                    }
                                    return true;
                                }
                            }
                            false
                        });
                        if handled_by_mp {
                            button_hit_large();
                            continue;
                        }
                        if let Some(after) = self.edit_move_state.take() {
                            button_hit();
                            movement = Some((id, after));
                            continue;
                        }
                        if let Some(sel) = &mut self.multi_select {
                            button_hit();
                            let r = chart.to_ref();
                            let mut removed = false;
                            sel.retain(|it| {
                                if it == &r {
                                    removed = true;
                                    false
                                } else {
                                    true
                                }
                            });
                            if !removed {
                                sel.push(r);
                            }
                            continue;
                        }
                        if CHOOSE_COVER.load(Ordering::Relaxed) {
                            button_hit();
                            CHOSEN_COVER.with(|it| {
                                *it.borrow_mut() = Some(if let Some(id) = chart.info.id {
                                    Ok(id)
                                } else {
                                    Err(chart.local_path.clone().unwrap())
                                });
                            });
                            continue;
                        }

                        button_hit_large();
                        let download_path = chart.info.id.map(|it| format!("download/{it}"));
                        let scene = SongScene::new(
                            chart.clone(),
                            if let Some(path) = &chart.local_path {
                                Some(path.clone())
                            } else {
                                let path = download_path.clone().unwrap();
                                if Path::new(&format!("{}/{path}", dir::charts()?)).exists() {
                                    Some(path)
                                } else {
                                    None
                                }
                            },
                            Arc::clone(&self.icons),
                            self.rank_icons.clone(),
                            get_data()
                                .charts
                                .iter()
                                .find(|it| Some(&it.local_path) == download_path.as_ref())
                                .map(|it| it.mods)
                                .unwrap_or_default(),
                        );
                        self.transit = Some(TransitState {
                            id: id as _,
                            rect: None,
                            chart: chart.clone(),
                            start_time: t,
                            next_scene: Some(NextScene::Overlay(Box::new(scene))),
                            back: false,
                            done: false,
                            delete: false,
                        });
                        return Ok(true);
                    }
                    if self.multi_select.is_none() && item.btn.long_touch(touch, t, &mut item.long_touch) {
                        self.scroll.y_scroller.halt();
                        self.editing_chart = Some(id);
                        let mut options = vec![tl!("select").into_owned()];
                        if self.allow_edit {
                            options.extend([
                                tl!("move-to-first").into_owned(),
                                tl!("move-to-last").into_owned(),
                                tl!("move-before").into_owned(),
                                tl!("move-after").into_owned(),
                            ]);
                        }
                        self.chart_menu.set_options(options);
                        self.chart_menu.set_selected(usize::MAX);
                        self.need_show_chart_menu = true;
                        return Ok(true);
                    }
                } else if item.btn.touch(touch, t) {
                    self.editing_chart = None;
                    self.edit_move_state = None;
                    button_hit_large();
                    self.clicked_special = true;
                }
            }
        }
        if let Some((id, after)) = movement {
            let has_header = self.has_header();
            let editing = self.editing_chart.unwrap();
            let to = if after {
                id + (id < editing) as usize
            } else {
                id - (id > editing) as usize
            };
            if let Some(charts) = &mut self.charts {
                let chart = charts.remove(editing);
                charts.insert(to, chart);
                assert!(to >= has_header as usize);
                self.movement = Some((editing - has_header as usize, to - has_header as usize));
            }
        }
        Ok(false)
    }

    fn has_header(&self) -> bool {
        self.charts.as_ref().is_some_and(|it| it.first().is_some_and(|item| item.chart.is_none()))
    }

    pub fn update(&mut self, t: f32) -> Result<bool> {
        let refreshed = self.can_refresh && self.scroll.y_scroller.pulled;
        self.scroll.update(t);
        self.chart_menu.update(t);
        if self.chart_menu.changed() {
            let has_header = self.has_header();
            let editing = self.editing_chart.unwrap();
            match self.chart_menu.selected() {
                0 => {
                    let chart = self.charts.as_ref().unwrap()[editing].chart.as_ref().unwrap();
                    self.multi_select = Some([chart.to_ref()].into());
                }
                1 => {
                    self.movement = Some((editing - has_header as usize, 0));
                    if let Some(charts) = &mut self.charts {
                        let chart = charts.remove(editing);
                        charts.insert(has_header as usize, chart);
                    }
                }
                2 => {
                    self.movement = Some((editing - has_header as usize, self.charts.as_ref().unwrap().len() - 1 - has_header as usize));
                    if let Some(charts) = &mut self.charts {
                        let chart = charts.remove(editing);
                        charts.push(chart);
                    }
                }
                3 | 4 => {
                    self.edit_move_state = Some(self.chart_menu.selected() == 4);
                    show_message(tl!("choose-target"));
                }
                _ => {}
            }
        }
        if let Some(transit) = &mut self.transit {
            transit.chart.illu.settle(t);
            if t > transit.start_time + TRANSIT_TIME {
                if transit.back {
                    if transit.delete {
                        let data = get_data_mut();
                        let item = &self.charts.as_ref().unwrap()[transit.id as usize];
                        let path = if let Some(path) = &item.chart.as_ref().unwrap().local_path {
                            path.clone()
                        } else {
                            format!("download/{}", item.chart.as_ref().unwrap().info.id.unwrap())
                        };
                        std::fs::remove_dir_all(format!("{}/{path}", dir::charts()?))?;

                        if let Some(chart) = data.find_chart_by_path(path.as_str()) {
                            data.charts.remove(chart);
                        }

                        save_data()?;
                        NEED_UPDATE.store(true, Ordering::SeqCst);
                    } else {
                        self.back_fade_in = Some((transit.id, t));
                    }
                    self.transit = None;
                } else {
                    transit.done = true;
                }
            }
        }

        if let Some(charts) = &mut self.charts {
            for chart in charts {
                if let Some(chart) = &mut chart.chart {
                    chart.illu.settle(t);
                }
            }
        }

        Ok(refreshed)
    }

    pub fn render(&mut self, ui: &mut Ui, r: Rect, t: f32) {
        let content_size = (r.w, r.h);
        let range = self.charts_display_range(content_size);
        let Some(charts) = &mut self.charts else {
            let ct = r.center();
            ui.loading(ct.x, ct.y, t, WHITE, ());
            return;
        };
        if charts.is_empty() {
            let ct = r.center();
            ui.text(ttl!("list-empty")).pos(ct.x, ct.y).anchor(0.5, 0.5).no_baseline().draw();
            return;
        }
        ui.scope(|ui| {
            ui.dx(r.x);
            ui.dy(r.y);
            let off = self.scroll.y_scroller.offset;
            self.scroll.size(content_size);
            self.scroll.render(ui, |ui| {
                if self.can_refresh {
                    render_release_to_refresh(ui, r.w / 2., off);
                }
                let cw = r.w / self.row_num as f32;
                let ch = self.row_height;
                let p = CHART_PADDING;
                let r = Rect::new(p, p, cw - p * 2., ch - p * 2.);
                self.fader.reset();
                self.fader.for_sub(|f| {
                    ui.hgrids(content_size.0, ch, self.row_num, charts.len() as u32, |ui, id| {
                        if let Some(transit) = &mut self.transit {
                            if transit.id == id {
                                transit.rect = Some(ui.rect_to_global(r));
                            }
                        }
                        if self.editing_chart == Some(id as usize) && self.need_show_chart_menu {
                            self.need_show_chart_menu = false;
                            self.chart_menu.set_auto_adjust(Some(ui.screen_rect().nonuniform_feather(-0.03, -0.05)));
                            self.chart_menu.show(ui, t, Rect::new(cw * 2. / 3., ch * 2. / 3., 0.35, 0.4));
                        }
                        if !range.contains(&id) {
                            if let Some(item) = charts.get_mut(id as usize) {
                                item.btn.invalidate();
                            }
                            return;
                        }
                        f.render(ui, t, |ui| {
                            let mut c = WHITE;

                            let item = &mut charts[id as usize];

                            item.btn.render_shadow(ui, r, t, |ui, path| {
                                let selected_color = Color::from_rgba(30, 136, 229, 255);

                                if let Some(chart) = &mut item.chart {
                                    let selected = self.multi_select.as_ref().and_then(|set| {
                                        let chart_ref = chart.to_ref();
                                        set.iter().position(|it| it == &chart_ref)
                                    });
                                    if selected.is_some() {
                                        ui.fill_path(&r.feather(0.008).rounded(0.003), selected_color);
                                    }

                                    chart.illu.notify();
                                    ui.fill_path(&path, semi_black(c.a));
                                    ui.fill_path(&path, chart.illu.shading(r.feather(0.01), t));
                                    if let Some((that_id, start_time)) = &self.back_fade_in {
                                        if id == *that_id {
                                            let p = ((t - start_time) / BACK_FADE_IN_TIME).max(0.);
                                            if p > 1. {
                                                self.back_fade_in = None;
                                            } else {
                                                ui.fill_path(&path, semi_black(0.55 * (1. - p)));
                                                c.a *= p;
                                            }
                                        }
                                    }

                                    ui.fill_path(&path, (semi_black(0.4 * c.a), (0., 0.), semi_black(0.8 * c.a), (0., ch)));

                                    let info = &chart.info;
                                    let mut level = info.level.clone();
                                    if !level.contains("Lv.") {
                                        use std::fmt::Write;
                                        write!(&mut level, " Lv.{}", info.difficulty as i32).unwrap();
                                    }
                                    let mut t = ui
                                        .text(level)
                                        .pos(r.right() - 0.016, r.y + 0.016)
                                        .max_width(r.w * 2. / 3.)
                                        .anchor(1., 0.)
                                        .size(0.52 * r.w / cw)
                                        .color(c);
                                    let ms = t.measure();
                                    t.ui.fill_path(
                                        &ms.feather(0.008).rounded(0.01),
                                        Color {
                                            a: c.a * 0.7,
                                            ..t.ui.background()
                                        },
                                    );
                                    t.draw();
                                    ui.text(&info.name)
                                        .pos(r.x + 0.01, r.bottom() - 0.02)
                                        .max_width(r.w)
                                        .anchor(0., 1.)
                                        .size(0.6 * r.w / cw)
                                        .color(c)
                                        .draw();
                                    if let Some(symbol) = item.symbol {
                                        ui.text(symbol.to_string())
                                            .pos(r.x + 0.01, r.y + 0.01)
                                            .size(0.8 * r.w / cw)
                                            .color(c)
                                            .draw();
                                    }

                                    if let Some(pos) = selected {
                                        ui.fill_path(&path, Color { a: 0.4, ..selected_color });
                                        let ct = r.center();
                                        ui.text((pos + 1).to_string())
                                            .pos(ct.x, ct.y)
                                            .anchor(0.5, 0.5)
                                            .no_baseline()
                                            .size(1.2 * r.w / cw)
                                            .color(WHITE)
                                            .draw_using(&BOLD_FONT);
                                    }
                                } else {
                                    ui.fill_path(&path, (*self.icons.r#abstract, r));
                                    ui.fill_path(&path, semi_black(0.2));
                                    let ct = r.center();
                                    use crate::page::coll::{tl as page_tl, L10N_LOCAL};
                                    ui.text(page_tl!("label"))
                                        .pos(ct.x, ct.y)
                                        .anchor(0.5, 0.5)
                                        .no_baseline()
                                        .size(0.7)
                                        .draw_using(&BOLD_FONT);
                                }
                            });
                        });
                    })
                })
            });
        });
        self.chart_menu.render(ui, t, 1.);
    }

    pub fn render_top(&mut self, ui: &mut Ui, t: f32) {
        if let Some(transit) = &self.transit {
            if let Some(fr) = transit.rect {
                let p = ((t - transit.start_time) / TRANSIT_TIME).clamp(0., 1.);
                let p = (1. - p).powi(4);
                let p = if transit.back { p } else { 1. - p };
                let r = Rect::tween(&fr, &ui.screen_rect(), p);
                let path = r.rounded(0.02 * (1. - p));
                ui.fill_path(&path, (*transit.chart.illu.texture.1, r.feather(0.01 * (1. - p))));
                ui.fill_path(&path, semi_black(0.55));
            }
        }
    }

    pub fn next_scene(&mut self) -> Option<NextScene> {
        if let Some(transit) = &mut self.transit {
            if transit.done {
                return transit.next_scene.take();
            }
        }
        None
    }
}
