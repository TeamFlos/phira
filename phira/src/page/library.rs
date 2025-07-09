prpr::tl_file!("library");

use super::{CollectionPage, NextPage, Page, SharedState};
use crate::{
    charts_view::{ChartDisplayItem, ChartsView, NEED_UPDATE},
    client::{Chart, Client},
    get_data,
    icons::Icons,
    popup::Popup,
    rate::RateDialog,
    scene::{check_read_tos_and_policy, ChartOrder, JUST_LOADED_TOS, ORDERS},
    tabs::{Tabs, TitleFn},
    tags::TagsDialog,
};
use anyhow::{anyhow, Result};
use macroquad::prelude::*;
use prpr::{
    ext::{poll_future, semi_black, JoinToString, LocalTask, RectExt, SafeTexture, ScaleType},
    scene::{request_file, request_input, return_input, show_error, show_message, take_input, NextScene},
    task::Task,
    ui::{button_hit, DRectButton, RectButton, Ui},
};
use std::{
    any::Any,
    borrow::Cow,
    ops::Deref,
    sync::{atomic::Ordering, Arc},
};
use tap::Tap;

const PAGE_NUM: u64 = 28;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ChartListType {
    Local,
    Ranked,
    Special,
    Unstable,
    Popular,
}

struct ChartList {
    ty: ChartListType,
    view: ChartsView,
}
impl ChartList {
    fn new(ty: ChartListType, icons: Arc<Icons>, rank_icons: [SafeTexture; 8]) -> Self {
        let mut view = ChartsView::new(icons, rank_icons);
        view.can_refresh = ty != ChartListType::Local;
        Self { ty, view }
    }
}

type OnlineTaskResult = (Vec<ChartDisplayItem>, Vec<Chart>, u64);
type OnlineTask = Task<Result<OnlineTaskResult>>;

pub struct LibraryPage {
    tabs: Tabs<ChartList>,

    current_page: u64,
    online_total_page: u64,
    prev_page_btn: DRectButton,
    next_page_btn: DRectButton,

    online_task: Option<OnlineTask>,

    icons: Arc<Icons>,

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

    next_page: Option<NextPage>,
    next_page_task: LocalTask<Result<NextPage>>,
}

impl LibraryPage {
    pub fn new(icons: Arc<Icons>, rank_icons: [SafeTexture; 8]) -> Result<Self> {
        NEED_UPDATE.store(true, Ordering::Relaxed);
        let icon_star = icons.star.clone();
        let new_list = |ty| ChartList::new(ty, Arc::clone(&icons), rank_icons.clone());
        Ok(Self {
            tabs: Tabs::new([
                (new_list(ChartListType::Local), || tl!("local")),
                (new_list(ChartListType::Ranked), || ttl!("chart-ranked")),
                (new_list(ChartListType::Special), || ttl!("chart-special")),
                (new_list(ChartListType::Unstable), || ttl!("chart-unstable")),
                (new_list(ChartListType::Popular), || tl!("popular")),
            ] as [(ChartList, TitleFn); 5]),

            current_page: 0,
            online_total_page: 0,
            prev_page_btn: DRectButton::new(),
            next_page_btn: DRectButton::new(),

            online_task: None,

            icons,

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

            next_page: None,
            next_page_task: None,
        })
    }
}

impl LibraryPage {
    fn total_page(&self, s: &SharedState) -> u64 {
        if self.tabs.selected().ty == ChartListType::Local {
            if s.charts_local.is_empty() {
                0
            } else {
                (s.charts_local.len() - 1) as u64 / PAGE_NUM + 1
            }
        } else {
            self.online_total_page
        }
    }

    pub fn load_online(&mut self) {
        if !check_read_tos_and_policy(false, false) {
            return;
        }
        if get_data().config.offline_mode {
            show_message(tl!("offline-mode")).error();
            return;
        }
        if get_data().me.is_none() {
            show_error(anyhow!(tl!("must-login")));
            return;
        }
        self.tabs.selected_mut().view.reset_scroll();
        self.tabs.selected_mut().view.clear();
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
        let chosen = self.tabs.selected().ty;
        let popular = chosen == ChartListType::Popular;
        let typ = match chosen {
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
            let charts: Vec<_> = remote_charts.iter().map(ChartDisplayItem::from_remote).collect();
            Ok((charts, remote_charts, total_page))
        }));
    }

    fn sync_local(&mut self, s: &SharedState) {
        let list = self.tabs.selected_mut();
        if list.ty == ChartListType::Local {
            let search = self.search_str.clone();
            let mut charts = Vec::new();
            charts.push(ChartDisplayItem::new(None, None));
            charts.append(&mut s.charts_local
                .iter()
                .filter(|it| it.info.name.contains(&search))
                .map(|it| ChartDisplayItem::new(Some(it.clone()), None))
                .collect::<Vec<ChartDisplayItem>>()
            );
            list.view.set(s.t, charts);
        }
    }
}

impl Page for LibraryPage {
    fn label(&self) -> Cow<'static, str> {
        tl!("label")
    }

    fn on_result(&mut self, res: Box<dyn Any>, s: &mut SharedState) -> Result<()> {
        let _res = match res.downcast::<bool>() {
            Err(res) => res,
            Ok(delete) => {
                self.tabs.selected_mut().view.on_result(s.t, *delete);
                return Ok(());
            }
        };
        Ok(())
    }

    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        let t = s.t;
        if self.order_menu.showing() {
            self.order_menu.touch(touch, t);
            return Ok(true);
        }
        if self.tabs.touch(touch, s.rt) {
            return Ok(true);
        }
        if self.tags.touch(touch, t) {
            return Ok(true);
        }
        if self.rating.touch(touch, t) {
            return Ok(true);
        }
        let charts_view = &mut self.tabs.selected_mut().view;
        if charts_view.transiting() {
            return Ok(true);
        }
        if charts_view.touch(touch, t, s.rt)? {
            return Ok(true);
        }
        if !matches!(self.tabs.selected().ty, ChartListType::Local) {
            if self.prev_page_btn.touch(touch, t) {
                if self.current_page != 0 {
                    self.current_page -= 1;
                    self.load_online();
                }
                return Ok(true);
            }
            if self.next_page_btn.touch(touch, t) {
                if self.current_page + 1 < self.total_page(s) {
                    self.current_page += 1;
                    self.load_online();
                }
                return Ok(true);
            }
        }

        match self.tabs.selected().ty {
            ChartListType::Local => {
                if self.import_btn.touch(touch, t) {
                    request_file("_import");
                    return Ok(true);
                }
                if !self.search_str.is_empty() && self.search_clr_btn.touch(touch) {
                    button_hit();
                    self.search_str.clear();
                    self.sync_local(s);
                    return Ok(true);
                }
                if !self.search_clr_btn.contains(touch.position) && self.search_btn.touch(touch, t) {
                    request_input("search", &self.search_str);
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
                if !self.search_clr_btn.contains(touch.position) && self.search_btn.touch(touch, t) {
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

        let is_local = self.tabs.selected().ty == ChartListType::Local;
        if self.tabs.changed() {
            self.tabs.selected_mut().view.reset_scroll();
            self.online_task = None;
            if is_local {
                self.sync_local(s);
            } else {
                self.current_page = 0;
                self.load_online();
            }
        }
        if self.tabs.selected_mut().view.clicked_special {
            let icons = Arc::clone(&self.icons);
            self.next_page_task = Some(Box::pin(async move { Ok(NextPage::Overlay(Box::new(CollectionPage::new(icons).await?))) }));
            self.tabs.selected_mut().view.clicked_special = false;
        }
        if let Some(task) = &mut self.next_page_task {
            if let Some(res) = poll_future(task.as_mut()) {
                self.next_page = Some(res?);
                self.next_page_task = None;
            }
        }

        if self.tags.show_rating {
            self.tags.show_rating = false;
            self.filter_show_tag = false;
            self.rating.enter(t);
        } else if self.tags_last_show && !self.tags.showing() {
            self.current_page = 0;
            self.load_online();
        }
        if self.rating.show_tags {
            self.rating.show_tags = false;
            self.filter_show_tag = true;
            self.tags.enter(t);
        } else if self.rating_last_show && !self.rating.showing() {
            self.current_page = 0;
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
                        self.tabs.selected_mut().view.set(t, res.0);
                    }
                }
                self.online_task = None;
            }
        }
        self.order_menu.update(t);
        for chart in &mut s.charts_local {
            chart.illu.settle(t);
        }
        if self.tabs.selected_mut().view.update(t)? {
            self.load_online();
        }
        if self.tabs.selected_mut().view.need_update() {
            s.reload_local_charts();
            self.sync_local(s);
        }
        if let Some((id, text)) = take_input() {
            if id == "search" {
                self.search_str = text;
                if is_local {
                    self.sync_local(s);
                } else {
                    self.current_page = 0;
                    self.load_online();
                }
            } else {
                return_input(id, text);
            }
        }
        if self.order_menu.changed() {
            self.current_order = self.order_menu.selected();
            self.current_page = 0;
            self.load_online();
        }
        if JUST_LOADED_TOS.fetch_and(false, Ordering::Relaxed) {
            check_read_tos_and_policy(false, false);
        }
        Ok(())
    }

    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        let rt = s.rt;
        let mut r = ui.content_rect();
        let chosen = self.tabs.selected().ty;
        if chosen != ChartListType::Local {
            r.h -= 0.08;
        }
        s.render_fader(ui, |ui| {
            self.tabs.render(ui, rt, r, |ui, list| {
                list.view.render(ui, r.feather(-0.01), t);
                Ok(())
            })
        })?;
        if chosen != ChartListType::Popular {
            s.render_fader(ui, |ui| {
                let empty = self.search_str.is_empty();
                let w = 0.53;
                let mut r = Rect::new(r.right() - w, -ui.top + 0.04, w, r.y + ui.top - 0.06);
                if empty {
                    r.x += r.h;
                    r.w -= r.h;
                }
                let rt = r.right();
                self.search_btn.render_shadow(ui, r, t, |ui, path| {
                    ui.fill_path(&path, semi_black(0.4));
                });
                let mut r = r.feather(-0.01);
                r.w = r.h;
                if !empty {
                    ui.fill_rect(r, (*self.icons.close, r, ScaleType::Fit));
                    self.search_clr_btn.set(ui, r);
                    r.x += r.w;
                }
                ui.fill_rect(r, (*self.icons.search, r, ScaleType::Fit));
                ui.text(&self.search_str)
                    .pos(r.right() + 0.01, r.center().y)
                    .anchor(0., 0.5)
                    .no_baseline()
                    .size(0.6)
                    .max_width(rt - r.right() - 0.02)
                    .draw();
                let mut r = r.feather(0.01);
                // TODO: better shifting
                r.x = 1. - w - r.w - 0.05;
                if empty {
                    r.x += r.w;
                }
                if chosen == ChartListType::Local {
                    let w = 0.24;
                    r.x = r.x + r.w - w;
                    r.w = w;
                    let ct = r.center();
                    self.import_btn.render_shadow(ui, r, t, |ui, path| {
                        ui.fill_path(&path, semi_black(0.4));
                    });
                    ui.text(tl!("import"))
                        .pos(ct.x, ct.y)
                        .anchor(0.5, 0.5)
                        .no_baseline()
                        .size(0.6)
                        .draw();
                } else {
                    self.order_btn.render_shadow(ui, r, t, |ui, path| {
                        ui.fill_path(&path, semi_black(0.4));
                        ui.fill_rect(r, (*self.icons.order, r, ScaleType::Fit));
                    });
                    if self.need_show_order_menu {
                        self.need_show_order_menu = false;
                        self.order_menu.set_bottom(true);
                        self.order_menu.set_selected(self.current_order);
                        self.order_menu.show(ui, t, Rect::new(r.x, r.bottom() + 0.02, 0.3, 0.4));
                    }
                    r.x -= r.w + 0.02;
                    self.filter_btn.render_shadow(ui, r, t, |ui, path| {
                        ui.fill_path(&path, semi_black(0.4));
                        let cr = r.feather(-0.005);
                        ui.fill_rect(cr, (*self.icons.filter, cr, ScaleType::Fit));
                    });
                }
            });
        }
        if chosen != ChartListType::Local {
            let total_page = self.total_page(s);
            s.render_fader(ui, |ui| {
                let cx = r.center().x;
                let r = ui
                    .text(tl!("page", "current" => self.current_page + 1, "total" => total_page))
                    .pos(cx, r.bottom() + 0.034)
                    .anchor(0.5, 0.)
                    .no_baseline()
                    .size(0.5)
                    .draw();
                let dist = 0.3;
                let ft = 0.024;
                let prev_page = tl!("prev-page");
                let r = ui.text(prev_page.deref()).pos(cx - dist, r.y).anchor(0.5, 0.).size(0.5).measure();
                self.prev_page_btn.render_text(ui, r.feather(ft), t, prev_page, 0.5, false);
                let next_page = tl!("next-page");
                let r = ui.text(next_page.deref()).pos(cx + dist, r.y).anchor(0.5, 0.).size(0.5).measure();
                self.next_page_btn.render_text(ui, r.feather(ft), t, next_page, 0.5, false);
            });
        }
        self.order_menu.render(ui, t, 1.);
        self.tags.render(ui, t);
        self.rating.render(ui, t);
        Ok(())
    }

    fn render_top(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        self.tabs.selected_mut().view.render_top(ui, s.t);
        Ok(())
    }

    fn next_page(&mut self) -> NextPage {
        self.next_page.take().unwrap_or_default()
    }

    fn next_scene(&mut self, _s: &mut SharedState) -> NextScene {
        self.tabs.selected_mut().view.next_scene().unwrap_or_default()
    }
}
