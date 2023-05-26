prpr::tl_file!("library");

use super::{ChartItem, Fader, Page, SharedState};
use crate::{
    client::{Chart, Client},
    dir, get_data, get_data_mut,
    popup::Popup,
    rate::RateDialog,
    save_data,
    scene::{ChartOrder, SongScene, ORDERS},
    tags::TagsDialog,
};
use anyhow::{anyhow, Result};
use macroquad::prelude::*;
use prpr::{
    core::Tweenable,
    ext::{semi_black, JoinToString, RectExt, SafeTexture, ScaleType, BLACK_TEXTURE},
    scene::{request_file, request_input, return_input, show_error, show_message, take_input, NextScene},
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
use tap::Tap;
use tokio::sync::Notify;

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
    Ranked,
    Special,
    Unstable,
    Popular,
}

type OnlineTaskResult = (Vec<(ChartItem, Option<char>)>, Vec<Chart>, u64);
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
    btn_ranked: DRectButton,
    btn_special: DRectButton,
    btn_unstable: DRectButton,
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
    online_charts_symbols: Option<Vec<Option<char>>>,

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
    icon_info: SafeTexture,
    icon_filter: SafeTexture,
    icon_mod: SafeTexture,
    icon_star: SafeTexture,

    import_btn: DRectButton,

    search_btn: DRectButton,
    search_str: String,
    search_clr_btn: RectButton,

    order_btn: DRectButton,
    order_menu: Popup,
    need_show_order_menu: bool,
    current_order: usize,

    filter_btn: DRectButton,
    tags: TagsDialog,
    tags_last_show: bool,
    rating: RateDialog,
    rating_last_show: bool,
    filter_show_tag: bool,
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
        icon_info: SafeTexture,
        icon_filter: SafeTexture,
        icon_mod: SafeTexture,
        icon_star: SafeTexture,
    ) -> Result<Self> {
        NEED_UPDATE.store(true, Ordering::SeqCst);
        Ok(Self {
            btn_local: DRectButton::new(),
            btn_ranked: DRectButton::new(),
            btn_special: DRectButton::new(),
            btn_unstable: DRectButton::new(),
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
            online_charts_symbols: None,

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
            icon_info,
            icon_filter,
            icon_mod,
            icon_star: icon_star.clone(),

            import_btn: DRectButton::new(),

            search_btn: DRectButton::new(),
            search_str: String::new(),
            search_clr_btn: RectButton::new(),

            order_btn: DRectButton::new(),
            order_menu: Popup::new().with_options(ChartOrder::names()),
            need_show_order_menu: false,
            current_order: 0,

            filter_btn: DRectButton::new(),
            tags: TagsDialog::new(true).tap_mut(|it| it.perms = get_data().me.as_ref().map(|it| it.perms()).unwrap_or_default()),
            tags_last_show: false,
            rating: RateDialog::new(icon_star, true).tap_mut(|it| {
                it.rate.score = 3;
                it.rate_upper.as_mut().unwrap().score = 10;
            }),
            rating_last_show: false,
            filter_show_tag: true,
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
            _ => self.online_charts.as_ref(),
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
                if !matches!(self.chosen, ChartListType::Local) {
                    ui.text(ttl!("release-to-refresh")).pos(r.w / 2., -0.13).anchor(0.5, 0.).size(0.8).draw();
                }
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
                            chart.illu.notify();
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
                            let mut level = chart.info.level.clone();
                            if !level.contains("Lv.") {
                                use std::fmt::Write;
                                write!(&mut level, " Lv.{}", chart.info.difficulty as i32).unwrap();
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
                            ui.text(&chart.info.name)
                                .pos(r.x + 0.01, r.bottom() - 0.02)
                                .max_width(r.w)
                                .anchor(0., 1.)
                                .size(0.6 * r.w / cw)
                                .color(c)
                                .draw();
                            if !matches!(self.chosen, ChartListType::Local) {
                                if let Some(ch) = self.online_charts_symbols.as_ref().unwrap()[id as usize] {
                                    ui.text(ch.to_string()).pos(r.x + 0.01, r.y + 0.01).size(0.8 * r.w / cw).color(c).draw();
                                }
                            }
                        });
                    })
                })
            });
        });
    }

    pub fn load_online(&mut self) {
        if get_data().config.offline_mode {
            show_message(tl!("offline-mode")).error();
            return;
        }
        if get_data().me.is_none() {
            show_error(anyhow!(tl!("must-login")));
            return;
        }
        self.scroll.y_scroller.offset = 0.;
        self.online_charts = None;
        self.online_charts_symbols = None;
        let page = self.current_page;
        let search = self.search_str.clone();
        let order = {
            let (order, mut rev) = ORDERS[self.current_order];
            let order = match order {
                ChartOrder::Default => {
                    rev ^= true;
                    "updated"
                }
                ChartOrder::Name => "name",
                ChartOrder::Rating => "rating",
            };
            if rev {
                format!("-{order}")
            } else {
                order.to_owned()
            }
        };
        let tags = self
            .tags
            .tags
            .tags()
            .iter()
            .cloned()
            .chain(self.tags.unwanted.as_ref().unwrap().tags().iter().map(|it| format!("-{it}")))
            .join(",");
        let division = self.tags.division;
        let rating_range = format!("{},{}", self.rating.rate.score as f32 / 10., self.rating.rate_upper.as_ref().unwrap().score as f32 / 10.);
        let popular = matches!(self.chosen, ChartListType::Popular);
        let typ = match self.chosen {
            ChartListType::Ranked => 0,
            ChartListType::Special => 1,
            ChartListType::Unstable => 2,
            _ => -1,
        };
        let by_me = if self.tags.show_me {
            get_data().me.as_ref().map(|it| it.id)
        } else {
            None
        };
        let show_unreviewed = self.tags.show_unreviewed;
        let show_stabilize = self.tags.show_stabilize;
        self.online_task = Some(Task::new(async move {
            let mut q = Client::query::<Chart>();
            if popular {
                q = q.suffix("/popular");
            } else {
                q = q.search(search).order(order).tags(tags).query("rating", rating_range);
            }
            if let Some(me) = by_me {
                q = q.query("uploader", me.to_string());
            }
            if show_stabilize {
                q = q.query("stableRequest", "true");
            } else if show_unreviewed {
                q = q.query("reviewed", "false").query("stableRequest", "false");
            }
            let (remote_charts, count) = q
                .query("type", typ.to_string())
                .query("division", division)
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
                            illu: {
                                let notify = Arc::new(Notify::new());
                                super::Illustration {
                                    texture: (BLACK_TEXTURE.clone(), BLACK_TEXTURE.clone()),
                                    notify: Arc::clone(&notify),
                                    task: Some(Task::new({
                                        let illu = it.illustration.clone();
                                        async move {
                                            notify.notified().await;
                                            Ok((illu.load_thumbnail().await?, None))
                                        }
                                    })),
                                    loaded: Arc::default(),
                                    load_time: f32::NAN,
                                }
                            },
                            local_path: None,
                        },
                        if it.stable_request {
                            Some('+')
                        } else if !it.reviewed {
                            Some('*')
                        } else {
                            None
                        },
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
        if self.tags.touch(touch, t) {
            return Ok(true);
        }
        if self.rating.touch(touch, t) {
            return Ok(true);
        }
        if self.transit.is_some() {
            return Ok(true);
        }
        if self.btn_local.touch(touch, t) {
            self.switch_to_type(ChartListType::Local);
            return Ok(true);
        }
        let to_type = [
            (&mut self.btn_ranked, ChartListType::Ranked),
            (&mut self.btn_special, ChartListType::Special),
            (&mut self.btn_unstable, ChartListType::Unstable),
            (&mut self.btn_popular, ChartListType::Popular),
        ]
        .into_iter()
        .filter_map(|it| if it.0.touch(touch, t) { Some(it.1) } else { None })
        .next();
        if let Some(typ) = to_type {
            if self.chosen != typ {
                self.online_charts = None;
                self.online_task = None;
                self.current_page = 0;
                self.switch_to_type(typ);
                self.load_online();
            }
            return Ok(true);
        }
        if !matches!(self.chosen, ChartListType::Local) && self.online_task.is_none() {
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
                _ => self.online_charts.as_ref(),
            };
            for (id, (btn, chart)) in self.chart_btns.iter_mut().zip(charts.into_iter().flatten()).enumerate() {
                if btn.touch(touch, t) {
                    button_hit_large();
                    let download_path = chart.info.id.map(|it| format!("download/{it}"));
                    let scene = SongScene::new(
                        chart.clone(),
                        if matches!(self.chosen, ChartListType::Local) {
                            None
                        } else {
                            s.charts_local
                                .iter()
                                .find(|it| it.local_path.as_ref() == Some(download_path.as_ref().unwrap()))
                                .map(|it| it.illu.clone())
                        },
                        if matches!(self.chosen, ChartListType::Local) {
                            chart.local_path.clone()
                        } else {
                            let path = download_path.clone().unwrap();
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
                        self.icon_info.clone(),
                        s.icons.clone(),
                        self.icon_mod.clone(),
                        self.icon_star.clone(),
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
            }
        }
        match self.chosen {
            ChartListType::Local => {
                if self.import_btn.touch(touch, t) {
                    request_file("_import");
                    return Ok(true);
                }
            }
            ChartListType::Ranked | ChartListType::Special | ChartListType::Unstable => {
                if !self.search_str.is_empty() && self.search_clr_btn.touch(touch) {
                    button_hit();
                    self.search_str.clear();
                    self.current_page = 0;
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
                if self.filter_btn.touch(touch, t) {
                    if self.filter_show_tag {
                        self.tags.enter(t);
                    } else {
                        self.rating.enter(t);
                    }
                    return Ok(true);
                }
            }
            ChartListType::Popular => {}
        }
        Ok(false)
    }

    fn update(&mut self, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        self.tags.update(t);
        self.rating.update(t);
        if self.tags.show_rating {
            self.tags.show_rating = false;
            self.filter_show_tag = false;
            self.rating.enter(t);
        } else if self.tags_last_show && !self.tags.showing() {
            self.load_online();
        }
        if self.rating.show_tags {
            self.rating.show_tags = false;
            self.filter_show_tag = true;
            self.tags.enter(t);
        } else if self.rating_last_show && !self.rating.showing() {
            self.load_online();
        }
        self.tags_last_show = self.tags.showing();
        self.rating_last_show = self.rating.showing();
        if let Some(task) = &mut self.online_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => show_error(err.context(tl!("failed-to-load-online"))),
                    Ok(res) => {
                        self.online_total_page = res.2;
                        let (online_charts, online_charts_symbols) = res.0.into_iter().unzip();
                        self.online_charts = Some(online_charts);
                        self.online_charts_symbols = Some(online_charts_symbols);
                        self.charts_fader.sub(t);
                    }
                }
                self.online_task = None;
            }
        }
        if !matches!(self.chosen, ChartListType::Local) && self.scroll.y_scroller.pulled && self.online_task.is_none() {
            self.load_online();
        }
        self.scroll.update(t);
        self.order_menu.update(t);
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
                        let path = if matches!(self.chosen, ChartListType::Local) {
                            s.charts_local[transit.id as usize].local_path.clone().unwrap()
                        } else {
                            format!("download/{}", self.online_charts.as_ref().unwrap()[transit.id as usize].info.id.unwrap())
                        };
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
        if let Some((id, text)) = take_input() {
            if id == "search" {
                self.search_str = text;
                self.current_page = 0;
                self.load_online();
            } else {
                return_input(id, text);
            }
        }
        if self.order_menu.changed() {
            self.current_order = self.order_menu.selected();
            self.current_page = 0;
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
                    (&mut self.btn_ranked, ttl!("chart-ranked"), ChartListType::Ranked),
                    (&mut self.btn_special, ttl!("chart-special"), ChartListType::Special),
                    (&mut self.btn_unstable, ttl!("chart-unstable"), ChartListType::Unstable),
                    (&mut self.btn_popular, tl!("popular"), ChartListType::Popular),
                ]
                .into_iter()
                .map(|(btn, text, ty)| (btn, text, ty == self.chosen)),
            );
        });
        let mut r = ui.content_rect();
        if !matches!(self.chosen, ChartListType::Local) {
            r.h -= 0.08;
        }
        match self.chosen {
            ChartListType::Local => {
                s.render_fader(ui, |ui, c| {
                    let w = 0.24;
                    let r = Rect::new(r.right() - w, -ui.top + 0.04, w, r.y + ui.top - 0.06);
                    self.import_btn.render_text(ui, r, t, c.a, tl!("import"), 0.6, false);
                });
            }
            ChartListType::Ranked | ChartListType::Special | ChartListType::Unstable => {
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
                    let mut r = r.feather(0.01);
                    r.x = 1. - w - r.w - 0.05;
                    if empty {
                        r.x += r.w;
                    }
                    let (cr, _) = self.order_btn.render_shadow(ui, r, t, c.a, |_| semi_black(0.4 * c.a));
                    ui.fill_rect(cr, (*self.icon_order, cr, ScaleType::Fit, c));
                    if self.need_show_order_menu {
                        self.need_show_order_menu = false;
                        self.order_menu.set_bottom(true);
                        self.order_menu.set_selected(self.current_order);
                        self.order_menu.show(ui, t, Rect::new(r.x, r.bottom() + 0.02, 0.3, 0.4));
                    }
                    r.x -= r.w + 0.02;
                    let (cr, _) = self.filter_btn.render_shadow(ui, r, t, c.a, |_| semi_black(0.4 * c.a));
                    let cr = cr.feather(-0.005);
                    ui.fill_rect(cr, (*self.icon_filter, cr, ScaleType::Fit, c));
                });
            }
            ChartListType::Popular => {}
        }
        s.fader.render(ui, t, |ui, c| {
            let path = r.rounded(0.02);
            ui.fill_path(&path, semi_black(0.4 * c.a));
            self.render_charts(ui, c, s.t, &s.charts_local, r.feather(-0.01))
        });
        if !matches!(self.chosen, ChartListType::Local) {
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
        }
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
        self.order_menu.render(ui, t, 1.);
        self.tags.render(ui, t);
        self.rating.render(ui, t);
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
