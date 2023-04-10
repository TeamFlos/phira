prpr::tl_file!("library");

use super::{ChartItem, Fader, Page, SharedState};
use crate::{
    client::{Chart, Client, File},
    data::LocalChart,
    dir, get_data_mut,
    popup::Popup,
    save_data,
    scene::{import_chart, ChartOrder, SongScene, ORDERS},
};
use anyhow::Result;
use macroquad::prelude::*;
use prpr::{
    core::Tweenable,
    ext::{semi_black, RectExt, SafeTexture, ScaleType, BLACK_TEXTURE},
    scene::{request_file, request_input, return_file, return_input, show_error, show_message, take_file, take_input, NextScene},
    task::Task,
    ui::{button_hit, button_hit_large, DRectButton, RectButton, Scroll, Ui},
};
use std::{
    any::Any,
    borrow::Cow,
    ops::{Deref, Range},
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

const CHART_HEIGHT: f32 = 0.3;
const CHART_PADDING: f32 = 0.013;
const ROW_NUM: u32 = 4;
const PAGE_NUM: u64 = 28;
const TRANSIT_TIME: f32 = 0.4;
const BACK_FADE_IN_TIME: f32 = 0.2;

pub static NEED_UPDATE: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, PartialEq, Eq)]
enum ChartListType {
    Local,
    Online,
    Popular,
}

type OnlineTaskResult = (Vec<(ChartItem, File)>, Vec<Chart>, u64);
type OnlineTask = Task<Result<OnlineTaskResult>>;

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

pub struct LibraryPage {
    btn_local: DRectButton,
    btn_online: DRectButton,
    btn_popular: DRectButton,
    chosen: ChartListType,

    transit: Option<TransitState>,
    back_fade_in: Option<(u32, f32)>,

    scroll: Scroll,
    chart_btns: Vec<DRectButton>,
    charts_fader: Fader,
    current_page: u64,
    online_total_page: u64,
    prev_page_btn: DRectButton,
    next_page_btn: DRectButton,

    online_task: Option<OnlineTask>,
    online_charts: Option<Vec<ChartItem>>,

    icon_back: SafeTexture,
    icon_play: SafeTexture,
    icon_download: SafeTexture,
    icon_menu: SafeTexture,
    icon_edit: SafeTexture,
    icon_ldb: SafeTexture,
    icon_user: SafeTexture,
    icon_close: SafeTexture,
    icon_search: SafeTexture,
    icon_order: SafeTexture,

    import_btn: DRectButton,
    import_task: Option<Task<Result<LocalChart>>>,

    search_btn: DRectButton,
    search_str: String,
    search_clr_btn: RectButton,

    order_btn: DRectButton,
    order_menu: Popup,
    need_show_order_menu: bool,
    current_order: usize,
}

impl LibraryPage {
    pub fn new(
        icon_back: SafeTexture,
        icon_play: SafeTexture,
        icon_download: SafeTexture,
        icon_menu: SafeTexture,
        icon_edit: SafeTexture,
        icon_ldb: SafeTexture,
        icon_user: SafeTexture,
        icon_close: SafeTexture,
        icon_search: SafeTexture,
        icon_order: SafeTexture,
    ) -> Result<Self> {
        NEED_UPDATE.store(true, Ordering::SeqCst);
        Ok(Self {
            btn_local: DRectButton::new(),
            btn_online: DRectButton::new(),
            btn_popular: DRectButton::new(),
            chosen: ChartListType::Local,

            transit: None,
            back_fade_in: None,

            scroll: Scroll::new(),
            chart_btns: Vec::new(),
            charts_fader: Fader::new().with_distance(0.12),
            current_page: 0,
            online_total_page: 0,
            prev_page_btn: DRectButton::new(),
            next_page_btn: DRectButton::new(),

            online_task: None,
            online_charts: None,

            icon_back,
            icon_play,
            icon_download,
            icon_menu,
            icon_edit,
            icon_ldb,
            icon_user,
            icon_close,
            icon_search,
            icon_order,

            import_btn: DRectButton::new(),
            import_task: None,

            search_btn: DRectButton::new(),
            search_str: String::new(),
            search_clr_btn: RectButton::new(),

            order_btn: DRectButton::new(),
            order_menu: Popup::new().with_options(ChartOrder::names()),
            need_show_order_menu: false,
            current_order: 0,
        })
    }
}

impl LibraryPage {
    fn total_page(&self, s: &SharedState) -> u64 {
        match self.chosen {
            ChartListType::Local => {
                if s.charts_local.is_empty() {
                    0
                } else {
                    (s.charts_local.len() - 1) as u64 / PAGE_NUM + 1
                }
            }
            _ => self.online_total_page,
        }
    }

    fn charts_display_range(&mut self, content_size: (f32, f32)) -> Range<u32> {
        let sy = self.scroll.y_scroller.offset;
        let start_line = (sy / CHART_HEIGHT) as u32;
        let end_line = ((sy + content_size.1) / CHART_HEIGHT).ceil() as u32;
        let res = (start_line * ROW_NUM)..((end_line + 1) * ROW_NUM);
        if let Some(need) = (res.end as usize).checked_sub(self.chart_btns.len()) {
            self.chart_btns
                .extend(std::iter::repeat_with(|| DRectButton::new().no_sound()).take(need));
        }
        res
    }

    pub fn render_charts(&mut self, ui: &mut Ui, c: Color, t: f32, local: &Vec<ChartItem>, r: Rect) {
        let content_size = (r.w, r.h);
        let range = self.charts_display_range(content_size);
        self.scroll.size(content_size);
        let charts = match self.chosen {
            ChartListType::Local => Some(local),
            ChartListType::Online => self.online_charts.as_ref(),
            _ => unreachable!(),
        };
        let Some(charts) = charts else {
            let ct = r.center();
            ui.loading(ct.x, ct.y, t, c, ());
            return;
        };
        if charts.is_empty() {
            let ct = r.center();
            ui.text(tl!("list-empty")).pos(ct.x, ct.y).anchor(0.5, 0.5).no_baseline().color(c).draw();
            return;
        }
        ui.scope(|ui| {
            ui.dx(r.x);
            ui.dy(r.y);
            self.scroll.render(ui, |ui| {
                let cw = r.w / ROW_NUM as f32;
                let ch = CHART_HEIGHT;
                let p = CHART_PADDING;
                let r = Rect::new(p, p, cw - p * 2., ch - p * 2.);
                self.charts_fader.reset();
                self.charts_fader.for_sub(|f| {
                    ui.hgrids(content_size.0, ch, ROW_NUM, charts.len() as u32, |ui, id| {
                        if let Some(transit) = &mut self.transit {
                            if transit.id == id {
                                transit.rect = Some(ui.rect_to_global(r));
                            }
                        }
                        if !range.contains(&id) {
                            if let Some(btn) = self.chart_btns.get_mut(id as usize) {
                                btn.invalidate();
                            }
                            return;
                        }
                        f.render(ui, t, |ui, nc| {
                            let mut c = Color { a: nc.a * c.a, ..nc };
                            let chart = &charts[id as usize];
                            let (r, path) = self.chart_btns[id as usize].render_shadow(ui, r, t, c.a, |_| semi_black(c.a));
                            ui.fill_path(
                                &path,
                                (
                                    *chart.illu.texture.0,
                                    r.feather(0.01),
                                    ScaleType::CropCenter,
                                    Color {
                                        a: c.a * chart.illu.alpha(t),
                                        ..c
                                    },
                                ),
                            );
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
                            ui.text(&chart.info.name)
                                .pos(r.x + 0.01, r.bottom() - 0.02)
                                .max_width(r.w)
                                .anchor(0., 1.)
                                .size(0.6 * r.w / cw)
                                .color(c)
                                .draw();
                        });
                    })
                })
            });
        });
    }

    pub fn load_online(&mut self) {
        self.scroll.y_scroller.offset = 0.;
        self.online_charts = None;
        let page = self.current_page;
        let search = self.search_str.clone();
        let order = {
            let (order, mut rev) = ORDERS[self.current_order];
            let order = match order {
                ChartOrder::Default => {
                    rev ^= true;
                    "updated"
                }
                ChartOrder::Name => {
                    rev ^= true;
                    "name"
                }
            };
            if rev {
                format!("-{order}")
            } else {
                order.to_owned()
            }
        };
        self.online_task = Some(Task::new(async move {
            let (remote_charts, count) = Client::query::<Chart>()
                .search(search)
                .order(order)
                .page(page)
                .page_num(PAGE_NUM)
                .send()
                .await?;
            let total_page = if count == 0 { 0 } else { (count - 1) / PAGE_NUM + 1 };
            let charts: Vec<_> = remote_charts
                .iter()
                .map(|it| {
                    (
                        ChartItem {
                            info: it.to_info(),
                            illu: super::Illustration {
                                texture: (BLACK_TEXTURE.clone(), BLACK_TEXTURE.clone()),
                                task: Some(Task::new({
                                    let illu = it.illustration.clone();
                                    async move { Ok((illu.load_thumbnail().await?, None)) }
                                })),
                                loaded: Arc::default(),
                                load_time: f32::NAN,
                            },
                            local_path: None,
                        },
                        it.illustration.clone(),
                    )
                })
                .collect();
            Ok((charts, remote_charts, total_page))
        }));
    }

    #[inline]
    fn switch_to_type(&mut self, ty: ChartListType) {
        if self.chosen != ty {
            self.chosen = ty;
            self.chart_btns.clear();
            self.current_page = 0;
            self.scroll.y_scroller.offset = 0.;
        }
    }
}

impl Page for LibraryPage {
    fn label(&self) -> Cow<'static, str> {
        "LIBRARY".into()
    }

    fn on_result(&mut self, res: Box<dyn Any>, s: &mut SharedState) -> Result<()> {
        let _res = match res.downcast::<bool>() {
            Err(res) => res,
            Ok(delete) => {
                let transit = self.transit.as_mut().unwrap();
                transit.start_time = s.t;
                transit.back = true;
                transit.done = false;
                transit.delete = *delete;
                return Ok(());
            }
        };
        Ok(())
    }

    fn enter(&mut self, s: &mut SharedState) -> Result<()> {
        if self.transit.is_none() && NEED_UPDATE.fetch_and(false, Ordering::SeqCst) {
            s.reload_local_charts();
        }
        Ok(())
    }

    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        let t = s.t;
        if self.order_menu.showing() {
            self.order_menu.touch(touch, t);
            return Ok(true);
        }
        if self.transit.is_some() || self.import_task.is_some() {
            return Ok(true);
        }
        if self.btn_local.touch(touch, t) {
            self.switch_to_type(ChartListType::Local);
            return Ok(true);
        }
        if self.btn_online.touch(touch, t) {
            self.switch_to_type(ChartListType::Online);
            if self.online_charts.is_none() {
                self.load_online();
            }
            return Ok(true);
        }
        if self.btn_popular.touch(touch, t) {
            // self.chosen = ChartListType::Popular;
            show_message(tl!("not-opened")).warn();
            return Ok(true);
        }
        if self.online_task.is_none() {
            if self.prev_page_btn.touch(touch, t) {
                if self.current_page != 0 {
                    self.current_page -= 1;
                    self.chart_btns.clear();
                    self.load_online();
                }
                return Ok(true);
            }
            if self.next_page_btn.touch(touch, t) {
                if self.current_page + 1 < self.total_page(s) {
                    self.current_page += 1;
                    self.chart_btns.clear();
                    self.load_online();
                }
                return Ok(true);
            }
        }
        if self.scroll.touch(touch, t) {
            return Ok(true);
        }
        if self.scroll.contains(touch) {
            let charts = match self.chosen {
                ChartListType::Local => Some(&s.charts_local),
                ChartListType::Online => self.online_charts.as_ref(),
                _ => unreachable!(),
            };
            for (id, (btn, chart)) in self.chart_btns.iter_mut().zip(charts.into_iter().flatten()).enumerate() {
                if btn.touch(touch, t) {
                    button_hit_large();
                    let scene = SongScene::new(
                        chart.clone(),
                        if matches!(self.chosen, ChartListType::Local) {
                            None
                        } else {
                            let path = format!("download/{}", chart.info.id.unwrap());
                            s.charts_local
                                .iter()
                                .find(|it| it.local_path.as_ref() == Some(&path))
                                .map(|it| it.illu.clone())
                        },
                        if matches!(self.chosen, ChartListType::Local) {
                            chart.local_path.clone()
                        } else {
                            let path = format!("download/{}", chart.info.id.unwrap());
                            if Path::new(&format!("{}/{path}", dir::charts()?)).exists() {
                                Some(path)
                            } else {
                                None
                            }
                        },
                        self.icon_back.clone(),
                        self.icon_play.clone(),
                        self.icon_download.clone(),
                        self.icon_menu.clone(),
                        self.icon_edit.clone(),
                        self.icon_ldb.clone(),
                        self.icon_user.clone(),
                        s.icons.clone(),
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
            }
        }
        match self.chosen {
            ChartListType::Local => {
                if self.import_btn.touch(touch, t) {
                    request_file("import");
                    return Ok(true);
                }
            }
            ChartListType::Online => {
                if !self.search_str.is_empty() && self.search_clr_btn.touch(touch) {
                    button_hit();
                    self.search_str.clear();
                    self.load_online();
                    return Ok(true);
                }
                if !self.search_clr_btn.rect.contains(touch.position) && self.search_btn.touch(touch, t) {
                    request_input("search", &self.search_str);
                    return Ok(true);
                }
                if self.order_btn.touch(touch, t) {
                    self.need_show_order_menu = true;
                    return Ok(true);
                }
            }
            ChartListType::Popular => {}
        }
        Ok(false)
    }

    fn update(&mut self, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        if let Some(task) = &mut self.online_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => show_error(err.context(tl!("failed-to-load-online"))),
                    Ok(res) => {
                        self.online_total_page = res.2;
                        self.online_charts = Some(res.0.into_iter().map(|it| it.0).collect());
                        self.charts_fader.sub(t);
                    }
                }
                self.online_task = None;
            }
        }
        self.scroll.update(t);
        for chart in &mut s.charts_local {
            chart.illu.settle(t);
        }
        if let Some(charts) = &mut self.online_charts {
            for chart in charts {
                chart.illu.settle(t);
            }
        }
        if let Some(transit) = &mut self.transit {
            transit.chart.illu.settle(t);
            if t > transit.start_time + TRANSIT_TIME {
                if transit.back {
                    if transit.delete {
                        let data = get_data_mut();
                        let path = s.charts_local[transit.id as usize].local_path.clone().unwrap();
                        std::fs::remove_dir_all(format!("{}/{path}", dir::charts()?))?;
                        data.charts.remove(data.find_chart_by_path(path.as_str()).unwrap());
                        save_data()?;
                        NEED_UPDATE.store(true, Ordering::SeqCst);
                    } else {
                        self.back_fade_in = Some((transit.id, t));
                    }
                    if NEED_UPDATE.fetch_and(false, Ordering::SeqCst) {
                        s.reload_local_charts();
                    }
                    self.transit = None;
                } else {
                    transit.done = true;
                }
            }
        }
        if let Some((id, file)) = take_file() {
            if id == "import" {
                self.import_task = Some(Task::new(import_chart(file)));
            } else {
                return_file(id, file);
            }
        }
        if let Some((id, text)) = take_input() {
            if id == "search" {
                self.search_str = text;
                self.load_online();
            } else {
                return_input(id, text);
            }
        }
        if let Some(task) = &mut self.import_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        show_error(err.context(tl!("import-failed")));
                    }
                    Ok(chart) => {
                        show_message(tl!("import-success")).ok();
                        get_data_mut().charts.push(chart);
                        save_data()?;
                        s.reload_local_charts();
                    }
                }
                self.import_task = None;
            }
        }
        if self.order_menu.changed() {
            self.current_order = self.order_menu.selected();
            self.load_online();
        }
        Ok(())
    }

    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        s.render_fader(ui, |ui, c| {
            ui.tab_rects(
                c,
                t,
                [
                    (&mut self.btn_local, tl!("local"), ChartListType::Local),
                    (&mut self.btn_online, tl!("online"), ChartListType::Online),
                    (&mut self.btn_popular, tl!("popular"), ChartListType::Popular),
                ]
                .into_iter()
                .map(|(btn, text, ty)| (btn, text, ty == self.chosen)),
            );
        });
        let mut r = ui.content_rect();
        r.h -= 0.08;
        match self.chosen {
            ChartListType::Local => {
                s.render_fader(ui, |ui, c| {
                    let w = 0.24;
                    let r = Rect::new(r.right() - w, -ui.top + 0.04, w, r.y + ui.top - 0.06);
                    self.import_btn.render_text(ui, r, t, c.a, tl!("import"), 0.6, false);
                });
            }
            ChartListType::Online => {
                s.render_fader(ui, |ui, c| {
                    let empty = self.search_str.is_empty();
                    let w = 0.53;
                    let mut r = Rect::new(r.right() - w, -ui.top + 0.04, w, r.y + ui.top - 0.06);
                    if empty {
                        r.x += r.h;
                        r.w -= r.h;
                    }
                    let rt = r.right();
                    self.search_btn.render_shadow(ui, r, t, c.a, |_| semi_black(0.4 * c.a));
                    let mut r = r.feather(-0.01);
                    r.w = r.h;
                    if !empty {
                        ui.fill_rect(r, (*self.icon_close, r, ScaleType::Fit, c));
                        self.search_clr_btn.set(ui, r);
                        r.x += r.w;
                    }
                    ui.fill_rect(r, (*self.icon_search, r, ScaleType::Fit, c));
                    ui.text(&self.search_str)
                        .pos(r.right() + 0.01, r.center().y)
                        .anchor(0., 0.5)
                        .no_baseline()
                        .size(0.6)
                        .max_width(rt - r.right() - 0.02)
                        .color(c)
                        .draw();
                    r.x = w - r.w - 0.03;
                    let r = r.feather(0.01);
                    let (cr, _) = self.order_btn.render_shadow(ui, r, t, c.a, |_| semi_black(0.4 * c.a));
                    ui.fill_rect(cr, (*self.icon_order, cr, ScaleType::Fit, c));
                    if self.need_show_order_menu {
                        self.need_show_order_menu = false;
                        self.order_menu.set_bottom(true);
                        self.order_menu.set_selected(self.current_order);
                        self.order_menu.show(ui, t, Rect::new(r.x, r.bottom() + 0.02, 0.3, 0.4));
                    }
                });
            }
            ChartListType::Popular => {}
        }
        s.fader.render(ui, t, |ui, c| {
            let path = r.rounded(0.02);
            ui.fill_path(&path, semi_black(0.4 * c.a));
            self.render_charts(ui, c, s.t, &s.charts_local, r.feather(-0.01))
        });
        let total_page = self.total_page(s);
        s.render_fader(ui, |ui, c| {
            let cx = r.center().x;
            let r = ui
                .text(tl!("page", "current" => self.current_page + 1, "total" => total_page))
                .pos(cx, r.bottom() + 0.034)
                .anchor(0.5, 0.)
                .no_baseline()
                .size(0.5)
                .color(c)
                .draw();
            let dist = 0.3;
            let ft = 0.024;
            let prev_page = tl!("prev-page");
            let r = ui.text(prev_page.deref()).pos(cx - dist, r.y).anchor(0.5, 0.).size(0.5).measure();
            self.prev_page_btn.render_text(ui, r.feather(ft), t, c.a, prev_page, 0.5, false);
            let next_page = tl!("next-page");
            let r = ui.text(next_page.deref()).pos(cx + dist, r.y).anchor(0.5, 0.).size(0.5).measure();
            self.next_page_btn.render_text(ui, r.feather(ft), t, c.a, next_page, 0.5, false);
        });
        if let Some(transit) = &self.transit {
            if let Some(fr) = transit.rect {
                let p = ((t - transit.start_time) / TRANSIT_TIME).clamp(0., 1.);
                let p = (1. - p).powi(4);
                let p = if transit.back { p } else { 1. - p };
                let r = Rect::new(
                    f32::tween(&fr.x, &-1., p),
                    f32::tween(&fr.y, &-ui.top, p),
                    f32::tween(&fr.w, &2., p),
                    f32::tween(&fr.h, &(ui.top * 2.), p),
                );
                let path = r.rounded(0.02 * (1. - p));
                ui.fill_path(&path, (*transit.chart.illu.texture.1, r.feather(0.01 * (1. - p))));
                ui.fill_path(&path, semi_black(0.55));
            }
        }
        if self.import_task.is_some() {
            ui.full_loading(tl!("importing"), t);
        }
        self.order_menu.render(ui, t, 1.);
        Ok(())
    }

    fn next_scene(&mut self, _s: &mut SharedState) -> NextScene {
        if let Some(transit) = &mut self.transit {
            if transit.done {
                return transit.next_scene.take().unwrap_or_default();
            }
        }
        NextScene::None
    }
}
