prpr_l10n::tl_file!("favorites");

use super::{Illustration, NextPage, Page, SharedState};
use crate::{
    client::{
        recv_raw, Chart, ChartRef, Client, Collection, CollectionContent, CollectionCover, CollectionPatch, File, LocalCollection, Ptr, UserManager,
    },
    get_data, get_data_mut,
    icons::Icons,
    page::{SFader, CHOOSE_COVER},
    popup::Popup,
    save_data,
    scene::{confirm_dialog, ProfileScene, TEX_BACKGROUND},
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use inputbox::{InputBox, InputMode};
use macroquad::prelude::*;
use prpr::{
    core::Tweenable,
    ext::{open_url, semi_black, semi_white, RectExt, SafeTexture, ScaleType},
    scene::{request_input, show_error, show_message, take_input},
    task::Task,
    ui::{button_hit, DRectButton, Dialog, RectButton, Scroll, Ui},
};
use regex::Regex;
use reqwest::Method;
use serde::Serialize;
use std::{
    borrow::Cow,
    cell::RefCell,
    collections::HashSet,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

thread_local! {
    pub static FAV_PAGE_RESULT: RefCell<Option<Option<usize>>> = const { RefCell::new(None) };
}

const CARD_WIDTH: f32 = 0.41;
const CARD_HEIGHT: f32 = 0.3;
const CARD_PAD: f32 = 0.033;

const INFO_TRANSIT: f32 = 0.32;
const INFO_WIDTH: f32 = 0.75;

#[derive(Serialize)]
struct PutCollection {
    #[serde(flatten)]
    content: CollectionContent,
    updated: Option<DateTime<Utc>>,
}

// 收藏夹文件夹项 || Folder item
struct FolderItem {
    index: Option<usize>,
    name: String,
    cover: Illustration,
    btn: RectButton,
}

pub struct FavoritesPage {
    icons: Arc<Icons>,
    rank_icons: [SafeTexture; 8],

    folders: Vec<FolderItem>,
    scroll: Scroll,

    create_btn: DRectButton,
    import_btn: DRectButton,
    all_illu: Illustration,

    active_folder: Option<usize>,

    info_btn: RectButton,
    info_scroll: Scroll,
    side_enter_time: f32,
    open_web_btn: DRectButton,
    owner_btn: RectButton,

    cloud_btn: RectButton,
    cloud_menu: Popup,
    cloud_options: Vec<&'static str>,
    need_show_cloud_menu: bool,
    cloud_delete: Arc<AtomicBool>,
    sync_from_cloud: Arc<AtomicBool>,
    force_sync_to_cloud: Arc<AtomicBool>,

    // 编辑状态 || Editing state
    edit_btn: RectButton,
    edit_menu: Popup,
    edit_options: Vec<&'static str>,
    need_show_edit_menu: bool,

    operations_menu_btn: RectButton,
    operations_menu: Popup,
    operations_options: Vec<&'static str>,
    operations_delete: Arc<AtomicBool>,
    need_show_operations_menu: bool,

    sf: SFader,
    next_page: Option<NextPage>,

    chosen_cover: Option<Result<i32, String>>,

    upload_task: Option<Task<Result<Collection>>>,
    delete_from_cloud_task: Option<Task<Result<()>>>,
    set_public_task: Option<Task<Result<Collection>>>,
    sync_task: Option<Task<Result<Option<Collection>>>>,
    import_task: Option<Task<Result<Collection>>>,
    batch_import_task: Option<Task<Result<Vec<Chart>>>>,
    set_cover_task: Option<Task<Result<Result<Collection, File>>>>,
}

impl FavoritesPage {
    pub fn new(icons: Arc<Icons>, rank_icons: [SafeTexture; 8], active_folder: Option<usize>, chosen_cover: Option<Result<i32, String>>) -> Self {
        let mut page = Self {
            icons,
            rank_icons,

            folders: Vec::new(),
            scroll: Scroll::new(),

            create_btn: DRectButton::new(),
            import_btn: DRectButton::new(),
            all_illu: Illustration::from_done(TEX_BACKGROUND.with(|it| it.borrow().clone().unwrap())),

            active_folder,

            info_btn: RectButton::new(),
            info_scroll: Scroll::new(),
            side_enter_time: f32::INFINITY,
            open_web_btn: DRectButton::new(),
            owner_btn: RectButton::new(),

            cloud_btn: RectButton::new(),
            cloud_menu: Popup::new(),
            cloud_options: Vec::new(),
            need_show_cloud_menu: false,
            cloud_delete: Arc::default(),
            sync_from_cloud: Arc::default(),
            force_sync_to_cloud: Arc::default(),

            edit_btn: RectButton::new(),
            edit_menu: Popup::new(),
            edit_options: Vec::new(),
            need_show_edit_menu: false,

            operations_menu_btn: RectButton::new(),
            operations_menu: Popup::new(),
            operations_options: Vec::new(),
            operations_delete: Arc::default(),
            need_show_operations_menu: false,

            sf: SFader::new(),
            next_page: None,

            chosen_cover,

            upload_task: None,
            delete_from_cloud_task: None,
            set_public_task: None,
            sync_task: None,
            import_task: None,
            batch_import_task: None,
            set_cover_task: None,
        };
        page.rebuild_folders();
        page
    }

    // 根据当前收藏夹数据重建文件夹列表 || Rebuild folder list from current data
    fn rebuild_folders(&mut self) {
        let data = get_data();
        let mut folders = Vec::new();

        // "显示全部谱面"卡片 || "Show all charts" card
        folders.push(FolderItem {
            index: None,
            name: tl!("show-all").to_string(),
            cover: self.all_illu.clone(),
            btn: RectButton::new(),
        });

        for (index, col) in data.collections().enumerate() {
            folders.push(FolderItem {
                index: Some(index),
                name: col.name.clone(),
                cover: col.cover(),
                btn: RectButton::new(),
            });
        }

        self.folders = folders;
    }

    fn render_info(&mut self, ui: &mut Ui, rt: f32) {
        let data = get_data();
        let col = data.collection_by_index(self.active_folder.unwrap());
        let pad = 0.03;
        ui.dx(pad);
        ui.dy(0.03);
        let width = INFO_WIDTH - pad;
        self.info_scroll.size((width - pad, ui.top * 2. - 0.06));
        self.info_scroll.render(ui, |ui| {
            let mut h = 0.;
            macro_rules! dy {
                ($e:expr) => {{
                    let dy = $e;
                    h += dy;
                    ui.dy(dy);
                }};
            }
            let mw = width - pad * 3.;
            if col.id.is_some() {
                let r = Rect::new(0.03, 0., mw, 0.12).nonuniform_feather(-0.03, -0.01);
                self.open_web_btn.render_text(ui, r, rt, ttl!("open-in-web"), 0.6, true);
                dy!(r.h + 0.04);
            }
            if let Some(uploader) = &col.owner {
                let c = 0.06;
                let s = 0.05;
                let r = ui.avatar(c, c, s, rt, UserManager::opt_avatar(uploader.id, &self.icons.user));
                self.owner_btn.set(ui, Rect::new(c - s, c - s, s * 2., s * 2.));
                if let Some((name, color)) = UserManager::name_and_color(uploader.id) {
                    ui.text(name)
                        .pos(r.right() + 0.02, r.center().y)
                        .anchor(0., 0.5)
                        .no_baseline()
                        .max_width(width - 0.15)
                        .size(0.6)
                        .color(color)
                        .draw();
                }
                dy!(0.14);
            }
            let mut item = |title: Cow<'_, str>, content: Cow<'_, str>| {
                dy!(ui.text(title).size(0.4).color(semi_white(0.7)).draw().h + 0.02);
                dy!(ui.text(content).pos(pad, 0.).size(0.6).multiline().max_width(mw).draw().h + 0.03);
            };
            item(tl!("info-name"), col.name.as_str().into());
            item(tl!("info-description"), col.description.as_str().into());
            item(tl!("info-count"), col.charts.len().to_string().into());
            if let Some(id) = col.id {
                item("ID".into(), id.to_string().into());
            }
            (width, h)
        });
    }

    fn has_task(&self) -> bool {
        self.upload_task.is_some()
            || self.delete_from_cloud_task.is_some()
            || self.set_public_task.is_some()
            || self.sync_task.is_some()
            || self.import_task.is_some()
            || self.batch_import_task.is_some()
            || self.set_cover_task.is_some()
    }

    fn collect_chart_ids(col: &LocalCollection, allow_local: bool) -> Option<Vec<i32>> {
        let data = get_data();
        let mut chart_ids = Vec::with_capacity(col.charts.len());
        let mut local_charts = Vec::new();
        for chart in &col.charts {
            match chart {
                ChartRef::Online(id, _) => {
                    chart_ids.push(*id);
                }
                ChartRef::Local(path) => {
                    if let Some(id) = data.charts.iter().find(|it| it.local_path == *path).and_then(|it| it.info.id) {
                        chart_ids.push(id);
                    } else if !allow_local {
                        local_charts.push(path);
                    }
                }
            }
        }
        if !local_charts.is_empty() {
            let mut charts = String::new();
            for path in local_charts {
                if let Some(index) = data.find_chart_by_path(path) {
                    charts.push_str(&data.charts[index].info.name);
                    charts.push_str(", ");
                }
            }
            if !charts.is_empty() {
                charts.truncate(charts.len() - 2);
            }
            Dialog::simple(ttl!("favorites-online-only", "charts" => charts)).show();
            return None;
        }
        Some(chart_ids)
    }

    fn try_import(&mut self, text: String) -> bool {
        let text = text.trim();

        let mut id = text.parse::<i32>().ok();
        if id.is_none() {
            let regex = Regex::new(r"phira\.moe/collection/(\d+)").unwrap();
            if let Some(caps) = regex.captures(text) {
                if let Some(id_str) = caps.get(1) {
                    id = id_str.as_str().parse::<i32>().ok();
                } else {
                    return false;
                }
            }
        }
        if let Some(id) = id {
            if get_data().collections().any(|col| col.id == Some(id)) {
                show_message(tl!("already-imported")).error();
            } else {
                self.import_task = Some(Task::new(async move {
                    let resp: Collection = recv_raw(Client::get(format!("/collection/{id}"))).await?.json().await?;
                    Ok(resp)
                }));
            }
            return true;
        }

        false
    }

    pub fn sync_to_cloud_task(index: usize, force: bool) -> Option<Task<Result<Option<Collection>>>> {
        let data = get_data();
        let col = data.collection_by_index(index);
        let chart_ids = Self::collect_chart_ids(&col, false)?;

        let body = PutCollection {
            content: CollectionContent {
                name: col.name.clone(),
                description: col.description.clone(),
                charts: chart_ids,
                public: col.public,
            },
            updated: if force { None } else { col.remote_updated },
        };
        let col_id = col.id.unwrap();
        Some(Task::new(async move {
            let result = recv_raw(Client::request(Method::PUT, format!("/collection/{col_id}")).json(&body)).await;
            match result {
                Ok(resp) => {
                    let resp: Collection = resp.json().await?;
                    Ok(Some(resp))
                }
                Err(err) => {
                    if err.to_string().starts_with("request failed (412)") {
                        Ok(None)
                    } else {
                        Err(err)
                    }
                }
            }
        }))
    }

    fn sync_to_cloud(&mut self, force: bool) {
        if let Some(task) = Self::sync_to_cloud_task(self.active_folder.unwrap(), force) {
            self.sync_task = Some(task);
        }
    }
}

impl Page for FavoritesPage {
    fn label(&self) -> Cow<'static, str> {
        ttl!("favorites")
    }

    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        let t = s.t;
        let rt = s.rt;

        if self.has_task() {
            return Ok(true);
        }

        if self.side_enter_time.is_finite() {
            if self.side_enter_time > 0. && rt > self.side_enter_time + INFO_TRANSIT {
                if touch.position.x < 1. - INFO_WIDTH && touch.phase == TouchPhase::Started {
                    self.side_enter_time = -rt;
                    return Ok(true);
                }
                if self.info_scroll.touch(touch, t) {
                    return Ok(true);
                }
                if self.open_web_btn.touch(touch, rt) {
                    if let Some(index) = self.active_folder {
                        let col = get_data().collection_by_index(index);
                        open_url(&format!("https://phira.moe/collection/{}", col.id.unwrap()))?;
                    }
                    return Ok(true);
                }
                if self.owner_btn.touch(touch) {
                    button_hit();
                    let col = get_data().collection_by_index(self.active_folder.unwrap());
                    self.sf
                        .goto(t, ProfileScene::new(col.owner.as_ref().unwrap().id, self.icons.user.clone(), self.rank_icons.clone()));
                    return Ok(true);
                }
            }
            return Ok(false);
        }

        if self.edit_menu.showing() {
            self.edit_menu.touch(touch, t);
            return Ok(true);
        }
        if self.operations_menu.showing() {
            self.operations_menu.touch(touch, t);
            return Ok(true);
        }
        if self.cloud_menu.showing() {
            self.cloud_menu.touch(touch, t);
            return Ok(true);
        }

        if self.create_btn.touch(touch, t) {
            request_input("fav_create", InputBox::new());
            return Ok(true);
        }
        if self.import_btn.touch(touch, t) {
            request_input("fav_import", InputBox::new());
            return Ok(true);
        }

        if self.scroll.touch(touch, rt) {
            return Ok(true);
        }

        if let Some(index) = self.active_folder {
            if self.edit_btn.touch(touch) {
                button_hit();
                let data = get_data();
                let col = data.collection_by_index(index);
                let mut options = Vec::new();
                if col.is_owned() {
                    options.push("rename");
                    options.push("set-description");
                    options.push("set-cover");
                }

                self.edit_menu.set_selected(usize::MAX);
                self.edit_menu.set_options(options.iter().map(|it| tl!(*it).into_owned()).collect());
                self.edit_options = options;
                self.need_show_edit_menu = true;
                return Ok(true);
            }
            if self.operations_menu_btn.touch(touch) {
                button_hit();
                let data = get_data();
                let col = data.collection_by_index(index);
                let is_default = col.is_default;
                let mut options = Vec::new();
                if !is_default && col.is_owned() {
                    options.push("set-as-default");
                }
                options.push("duplicate");
                if col.is_owned() {
                    options.push("batch-import");
                }
                if !is_default {
                    options.push("delete");
                }

                self.operations_menu.set_selected(usize::MAX);
                self.operations_menu.set_options(options.iter().map(|it| tl!(*it).into_owned()).collect());
                self.operations_options = options;
                self.need_show_operations_menu = true;
                return Ok(true);
            }
            if self.cloud_btn.touch(touch) {
                button_hit();
                let data = get_data();
                let col = data.collection_by_index(index);
                let mut options = Vec::new();
                if col.id.is_some() {
                    options.push("sync-to-cloud");
                    options.push("sync-from-cloud");
                    if col.public {
                        options.push("make-private");
                    } else {
                        options.push("make-public");
                    }
                    options.push("delete-from-cloud");
                } else {
                    options.push("upload-to-cloud");
                }
                self.cloud_menu.set_selected(usize::MAX);
                self.cloud_menu.set_options(options.iter().map(|it| tl!(*it).into_owned()).collect());
                self.cloud_options = options;
                self.need_show_cloud_menu = true;
                return Ok(true);
            }
            if self.info_btn.touch(touch) {
                button_hit();
                self.side_enter_time = rt;
                return Ok(true);
            }
        }

        // 编辑按钮检测 || Edit button detection
        for folder in self.folders.iter_mut() {
            if folder.btn.touch(touch) {
                button_hit();
                if self.active_folder == folder.index {
                    FAV_PAGE_RESULT.with(|it| *it.borrow_mut() = Some(folder.index));
                    self.next_page = Some(NextPage::Pop);
                } else {
                    self.active_folder = folder.index;
                }
                return Ok(true);
            }
        }

        Ok(false)
    }

    fn update(&mut self, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        self.scroll.update(s.rt);
        self.info_scroll.update(s.rt);

        self.edit_menu.update(t);
        self.operations_menu.update(t);
        self.cloud_menu.update(t);

        for folder in &mut self.folders {
            folder.cover.settle(t);
        }

        if let Some(chosen_cover) = self.chosen_cover.take() {
            let data = get_data_mut();
            let col = data.collection_by_index(self.active_folder.unwrap());
            match chosen_cover {
                Ok(chart_id) => {
                    let col_id = col.id;
                    self.set_cover_task = Some(Task::new(async move {
                        if let Some(col_id) = col_id {
                            // Collection is synced, update cloud directly
                            let resp: Collection =
                                recv_raw(Client::request(Method::PATCH, format!("/collection/{col_id}")).json(&CollectionPatch::Cover(chart_id)))
                                    .await?
                                    .json()
                                    .await?;
                            Ok(Ok(resp))
                        } else {
                            // Collection is not synced, fetch chart
                            // illustration and set as cover locally
                            let chart = Ptr::<Chart>::new(chart_id).fetch().await?;
                            Ok(Err(chart.illustration.clone()))
                        }
                    }));
                }
                Err(local_path) => {
                    if col.id.is_some() {
                        let chart = if let Some(index) = data.find_chart_by_path(&local_path) {
                            data.charts[index].info.name.clone()
                        } else {
                            String::new()
                        };
                        Dialog::simple(ttl!("favorites-online-only", "charts" => chart)).show();
                    } else {
                        let new_col = LocalCollection {
                            cover: CollectionCover::LocalChart(local_path),
                            ..col.as_ref().clone()
                        };
                        data.set_collection_info(&data.collection_uuids()[self.active_folder.unwrap()], new_col)?;
                        show_message(tl!("updated")).ok();
                        self.rebuild_folders();
                    }
                }
            }
        }

        // 处理输入事件 || Handle input events
        if let Some((id, text)) = take_input() {
            match id.as_str() {
                "fav_create" => {
                    let name = text.trim().to_string();
                    if name.is_empty() {
                        show_message(tl!("name-empty")).error();
                    } else {
                        get_data_mut().push_collection(LocalCollection::new(name))?;
                        let _ = save_data();
                        show_message(tl!("created")).ok();
                        self.rebuild_folders();
                    }
                }
                "fav_rename" => {
                    let new_name = text.trim().to_string();
                    if new_name.is_empty() {
                        show_message(tl!("name-empty")).error();
                    } else if let Some(index) = self.active_folder {
                        let data = get_data();
                        let uuid = data.collection_uuids()[index];
                        let col = data.collection_info(&uuid);
                        let new_col = LocalCollection {
                            name: new_name,
                            ..col.as_ref().clone()
                        };
                        data.set_collection_info(&uuid, new_col)?;
                        let _ = save_data();
                        show_message(tl!("updated")).ok();
                        self.rebuild_folders();
                        if col.id.is_some() && !data.config.offline_mode {
                            self.sync_to_cloud(false);
                        }
                    }
                }
                "fav_description" => {
                    let new_description = text.trim().to_string();
                    if let Some(index) = self.active_folder {
                        let data = get_data();
                        let uuid = data.collection_uuids()[index];
                        let col = data.collection_info(&uuid);
                        let new_col = LocalCollection {
                            description: new_description,
                            ..col.as_ref().clone()
                        };
                        data.set_collection_info(&uuid, new_col)?;
                        let _ = save_data();
                        show_message(tl!("updated")).ok();
                        self.rebuild_folders();
                        if col.id.is_some() && !data.config.offline_mode {
                            self.sync_to_cloud(false);
                        }
                    }
                }
                "fav_import" => {
                    if !self.try_import(text) {
                        show_message(tl!("invalid-import")).error();
                    }
                }
                "fav_batch_import" => {
                    let data = get_data();
                    let col = data.collection_by_index(self.active_folder.unwrap());
                    let local_chart_ids = Self::collect_chart_ids(&col, true).unwrap().into_iter().collect::<HashSet<_>>();
                    let Ok(mut chart_ids) = text
                        .split([',', ' '])
                        .map(|s| s.trim())
                        .filter(|s| !s.is_empty())
                        .map(|it| it.parse::<i32>())
                        .collect::<Result<Vec<_>, _>>()
                    else {
                        show_message(tl!("invalid-import")).error();
                        return Ok(());
                    };
                    chart_ids.retain(|id| !local_chart_ids.contains(id));
                    if chart_ids.is_empty() {
                        return Ok(());
                    }
                    self.batch_import_task = Some(Task::new(async move {
                        let mut ids_str = String::new();
                        for id in &chart_ids {
                            ids_str.push_str(&id.to_string());
                            ids_str.push(',');
                        }
                        ids_str.pop();

                        let resp: Vec<Chart> = recv_raw(Client::get(format!("/chart/multi-get?ids={ids_str}"))).await?.json().await?;
                        Ok(resp)
                    }));
                }
                _ => {}
            }
        }

        if let Some(index) = self.active_folder {
            let data = get_data_mut();
            let col = data.collection_by_index(index);
            if self.edit_menu.changed() {
                match self.edit_options[self.edit_menu.selected()] {
                    "rename" => {
                        request_input("fav_rename", InputBox::new().default_text(&col.name));
                    }
                    "set-description" => {
                        request_input("fav_description", InputBox::new().default_text(&col.description).mode(InputMode::Multiline));
                    }
                    "set-cover" => {
                        if col.charts.is_empty() {
                            show_message(tl!("no-charts")).error();
                        } else {
                            FAV_PAGE_RESULT.with(|it| *it.borrow_mut() = Some(Some(index)));
                            CHOOSE_COVER.store(true, Ordering::Relaxed);
                            show_message(tl!("select-cover"));
                            self.next_page = Some(NextPage::Pop);
                        }
                    }
                    _ => {}
                }
            }
            if self.operations_menu.changed() {
                match self.operations_options[self.operations_menu.selected()] {
                    "set-as-default" => {
                        let uuid = data.collection_uuids()[index];
                        let uuids = data.collection_uuids().to_vec();
                        for its_uuid in uuids {
                            let col = LocalCollection {
                                is_default: its_uuid == uuid,
                                ..data.collection_info(&its_uuid).as_ref().clone()
                            };
                            data.set_collection_info(&its_uuid, col)?;
                        }
                    }
                    "delete" => {
                        confirm_dialog(tl!("delete"), tl!("delete-confirm"), self.operations_delete.clone());
                    }
                    "duplicate" => {
                        data.push_collection(LocalCollection {
                            id: None,
                            public: false,
                            remote_updated: None,
                            is_default: false,
                            ..data.collection_by_index(index).as_ref().clone()
                        })?;
                        let _ = save_data();
                        self.rebuild_folders();
                    }
                    "batch-import" => {
                        request_input("fav_batch_import", InputBox::new());
                    }
                    _ => {}
                }
            }
            if self.operations_delete.swap(false, Ordering::SeqCst) {
                data.remove_collection(index)?;
                let _ = save_data();
                show_message(tl!("deleted")).ok();
                self.active_folder = if data.collection_uuids().is_empty() {
                    None
                } else {
                    self.active_folder.map(|it| it.min(data.collection_uuids().len() - 1))
                };
                FAV_PAGE_RESULT.with(|it| *it.borrow_mut() = Some(self.active_folder));
                self.rebuild_folders();
            }
            if self.cloud_delete.swap(false, Ordering::SeqCst) {
                let col_id = data.collection_by_index(index).id.unwrap();
                self.delete_from_cloud_task = Some(Task::new(async move {
                    recv_raw(Client::delete(format!("/collection/{col_id}"))).await?;
                    Ok(())
                }));
            }

            if self.cloud_menu.changed() {
                match self.cloud_options[self.cloud_menu.selected()] {
                    "upload-to-cloud" => {
                        let data = get_data();
                        let col = data.collection_by_index(index);
                        let Some(chart_ids) = Self::collect_chart_ids(&col, false) else {
                            return Ok(());
                        };

                        let body = CollectionContent {
                            name: col.name.clone(),
                            description: col.description.clone(),
                            charts: chart_ids,
                            public: false,
                        };
                        self.upload_task = Some(Task::new(async move {
                            let resp: Collection = recv_raw(Client::post("/collection", &body)).await?.json().await?;
                            Ok(resp)
                        }));
                    }
                    "delete-from-cloud" => {
                        confirm_dialog(tl!("delete-from-cloud"), tl!("delete-from-cloud-confirm"), self.cloud_delete.clone());
                    }
                    "make-public" | "make-private" => {
                        let data = get_data();
                        let col = data.collection_by_index(index);
                        let col_id = col.id.unwrap();
                        let new_public = !col.public;
                        self.set_public_task = Some(Task::new(async move {
                            let resp =
                                recv_raw(Client::request(Method::PATCH, format!("/collection/{col_id}")).json(&CollectionPatch::Public(new_public)))
                                    .await?
                                    .json()
                                    .await?;
                            Ok(resp)
                        }));
                    }
                    "sync-to-cloud" => {
                        self.sync_to_cloud(false);
                    }
                    "sync-from-cloud" => {
                        confirm_dialog(tl!("sync-from-cloud"), tl!("sync-confirm"), self.sync_from_cloud.clone());
                    }
                    _ => {}
                }
            }
            if self.sync_from_cloud.swap(false, Ordering::SeqCst) {
                let col_id = data.collection_by_index(index).id.unwrap();
                self.sync_task = Some(Task::new(async move {
                    let resp: Collection = recv_raw(Client::get(format!("/collection/{col_id}"))).await?.json().await?;
                    Ok(Some(resp))
                }));
            }
            if self.force_sync_to_cloud.swap(false, Ordering::SeqCst) {
                self.sync_to_cloud(true);
            }
        }

        if self.side_enter_time < 0. && -s.rt + INFO_TRANSIT < self.side_enter_time {
            self.side_enter_time = f32::INFINITY;
        }

        if let Some(task) = &mut self.upload_task {
            if let Some(result) = task.take() {
                match result {
                    Ok(col) => {
                        let data = get_data();
                        let uuid = data.collection_uuids()[self.active_folder.unwrap()];
                        let mut local = data.collection_info(&uuid).as_ref().clone();
                        local.id = Some(col.id);
                        data.set_collection_info(&uuid, local.merge(&col))?;
                        show_message(tl!("uploaded")).ok();
                        self.rebuild_folders();
                    }
                    Err(err) => {
                        show_error(err);
                    }
                }
                self.upload_task = None;
            }
        }
        if let Some(task) = &mut self.delete_from_cloud_task {
            if let Some(result) = task.take() {
                match result {
                    Ok(()) => {
                        let data = get_data();
                        let uuid = data.collection_uuids()[self.active_folder.unwrap()];
                        let mut local = data.collection_info(&uuid).as_ref().clone();
                        local.id = None;
                        data.set_collection_info(&uuid, local)?;
                        show_message(tl!("deleted")).ok();
                    }
                    Err(err) => {
                        show_error(err);
                    }
                }
                self.delete_from_cloud_task = None;
            }
        }
        if let Some(task) = &mut self.set_public_task {
            if let Some(result) = task.take() {
                match result {
                    Ok(col) => {
                        let data = get_data();
                        let uuid = data.collection_uuids()[self.active_folder.unwrap()];
                        let local = data.collection_info(&uuid);
                        data.set_collection_info(&uuid, local.merge(&col))?;
                        show_message(tl!("updated")).ok();
                        self.rebuild_folders();
                    }
                    Err(err) => {
                        show_error(err);
                    }
                }
                self.set_public_task = None;
            }
        }
        if let Some(task) = &mut self.sync_task {
            if let Some(result) = task.take() {
                match result {
                    Ok(Some(col)) => {
                        let data = get_data();
                        let uuid = data.collection_uuids()[self.active_folder.unwrap()];
                        let local = data.collection_info(&uuid);
                        data.set_collection_info(&uuid, local.merge(&col))?;
                        show_message(tl!("synced")).ok();
                        self.rebuild_folders();
                    }
                    Ok(None) => {
                        confirm_dialog(tl!("sync-to-cloud"), tl!("sync-outdated"), self.force_sync_to_cloud.clone());
                    }
                    Err(err) => {
                        show_error(err);
                    }
                }
                self.sync_task = None;
            }
        }
        if let Some(task) = &mut self.import_task {
            if let Some(result) = task.take() {
                match result {
                    Ok(col) => {
                        let data = get_data_mut();
                        let mut local = LocalCollection::new(String::new());
                        local.id = Some(col.id);
                        data.push_collection(local.merge(&col))?;
                        let _ = save_data();
                        show_message(tl!("imported")).ok();
                        self.rebuild_folders();
                    }
                    Err(err) => {
                        show_error(err);
                    }
                }
                self.import_task = None;
            }
        }
        if let Some(task) = &mut self.batch_import_task {
            if let Some(result) = task.take() {
                match result {
                    Ok(charts) => {
                        let data = get_data();
                        let uuid = data.collection_uuids()[self.active_folder.unwrap()];
                        let mut col = data.collection_info(&uuid).as_ref().clone();
                        let online = col.id.is_some();
                        col.charts.extend(charts.into_iter().map(Into::into));
                        data.set_collection_info(&uuid, col)?;
                        show_message(tl!("imported")).ok();
                        if online && !data.config.offline_mode {
                            self.sync_to_cloud(false);
                        }
                    }
                    Err(err) => {
                        show_error(err);
                    }
                }
                self.batch_import_task = None;
            }
        }
        if let Some(task) = &mut self.set_cover_task {
            if let Some(result) = task.take() {
                match result {
                    Ok(Ok(col)) => {
                        let data = get_data();
                        let uuid = data.collection_uuids()[self.active_folder.unwrap()];
                        let mut local = data.collection_info(&uuid).as_ref().clone();
                        local.id = Some(col.id);
                        data.set_collection_info(&uuid, local.merge(&col))?;
                        show_message(tl!("updated")).ok();
                        self.rebuild_folders();
                    }
                    Ok(Err(cover)) => {
                        let data = get_data();
                        let uuid = data.collection_uuids()[self.active_folder.unwrap()];
                        let mut col = data.collection_info(&uuid).as_ref().clone();
                        col.cover = CollectionCover::Online(cover);
                        data.set_collection_info(&uuid, col)?;
                        show_message(tl!("updated")).ok();
                        self.rebuild_folders();
                    }
                    Err(err) => {
                        show_error(err);
                    }
                }
                self.set_cover_task = None;
            }
        }

        Ok(())
    }

    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        for folder in &self.folders {
            folder.cover.notify();
        }

        s.render_fader(ui, |ui| {
            let top = -ui.top + 0.13;
            let bottom = ui.top;
            let content_h = bottom - top;

            if let Some(index) = self.active_folder {
                ui.scope(|ui| {
                    ui.dx(1. - 0.03);
                    ui.dy(-ui.top + 0.03);
                    let s = 0.08;
                    let r = Rect::new(-s, 0., s, s);
                    ui.fill_rect(r, (*self.icons.menu, r, ScaleType::Fit, WHITE));
                    self.operations_menu_btn.set(ui, r);
                    if self.need_show_operations_menu {
                        self.need_show_operations_menu = false;
                        self.operations_menu.set_bottom(true);
                        self.operations_menu.set_selected(usize::MAX);
                        let d = 0.28;
                        self.operations_menu.show(ui, t, Rect::new(r.x - d, r.bottom() + 0.02, r.w + d, 0.5));
                    }
                    ui.dx(-r.w - 0.03);
                    ui.fill_rect(r, (*self.icons.edit, r, ScaleType::Fit, WHITE));
                    self.edit_btn.set(ui, r);
                    if self.need_show_edit_menu {
                        self.need_show_edit_menu = false;
                        self.edit_menu.set_bottom(true);
                        self.edit_menu.set_selected(usize::MAX);
                        let d = 0.28;
                        self.edit_menu.show(ui, t, Rect::new(r.x - d, r.bottom() + 0.02, r.w + d, 0.5));
                    }
                    ui.dx(-r.w - 0.03);
                    ui.fill_rect(
                        r,
                        (
                            if get_data().collection_by_index(index).id.is_some() {
                                *self.icons.cloud_check
                            } else {
                                *self.icons.cloud_none
                            },
                            r,
                            ScaleType::Fit,
                        ),
                    );
                    self.cloud_btn.set(ui, r);
                    if self.need_show_cloud_menu {
                        self.need_show_cloud_menu = false;
                        self.cloud_menu.set_bottom(true);
                        self.cloud_menu.set_selected(usize::MAX);
                        let d = 0.28;
                        self.cloud_menu.show(ui, t, Rect::new(r.x - d, r.bottom() + 0.02, r.w + d, 0.5));
                    }
                    ui.dx(-r.w - 0.03);
                    ui.fill_rect(r, (*self.icons.info, r, ScaleType::Fit));
                    self.info_btn.set(ui, r);
                });
            }

            self.scroll.size((2., content_h));

            ui.scope(|ui| {
                ui.dx(-1.);
                ui.dy(top);
                self.scroll.render(ui, |ui| {
                    let start_x = 0.12;
                    let mut x = start_x;
                    let mut y = 0.02;
                    let max_x = 2.0 - 0.12;
                    let cols = ((max_x - start_x + CARD_PAD) / (CARD_WIDTH + CARD_PAD)).floor() as usize;
                    let cols = cols.max(1);

                    for (idx, folder) in self.folders.iter_mut().enumerate() {
                        if idx > 0 && idx % cols == 0 {
                            x = start_x;
                            y += CARD_HEIGHT + CARD_PAD;
                        }

                        let r = Rect::new(x, y, CARD_WIDTH, CARD_HEIGHT);
                        folder.btn.set(ui, r);
                        if self.active_folder == folder.index {
                            ui.fill_rect(r.feather(0.005), WHITE);
                        }

                        let illu_r = r;
                        let alpha = folder.cover.alpha(t);
                        if alpha > 0. {
                            ui.fill_rect(illu_r, folder.cover.shading(illu_r, t));
                        }
                        ui.fill_rect(illu_r, semi_black(0.3));
                        let mut name_max_w = r.w;
                        if folder.index.is_some_and(|it| get_data().collection_by_index(it).is_default) {
                            let mut text = ui.text(tl!("default")).size(0.5).color(WHITE);
                            let mut text_r = text.measure();
                            let pad_x = 0.02;
                            let pad_y = 0.01;
                            text_r.x = r.right() - text_r.w - pad_x;
                            text_r.y = r.bottom() - text_r.h - pad_y;
                            text.ui.fill_rect(text_r.nonuniform_feather(pad_x, pad_y), ORANGE);
                            text.pos(text_r.x, text_r.y).draw();
                            name_max_w -= text_r.w + pad_x * 2.;
                        }

                        ui.text(&folder.name)
                            .pos(r.x + 0.03, r.bottom() - 0.03)
                            .anchor(0., 1.)
                            .no_baseline()
                            .size(0.42)
                            .max_width(name_max_w)
                            .color(semi_white(0.9))
                            .draw();

                        // 谱面数量 || Chart count
                        if let Some(index) = folder.index {
                            let count = get_data().collection_by_index(index).charts.len();
                            ui.text(count.to_string())
                                .pos(r.right() - 0.02, r.y + 0.02)
                                .anchor(1., 0.)
                                .size(0.35)
                                .color(semi_white(0.6))
                                .draw();
                        }

                        x += CARD_WIDTH + CARD_PAD;
                    }

                    let total_h = y + CARD_HEIGHT + CARD_PAD + 0.1;
                    (2., total_h)
                });
            });

            // 新建收藏夹按钮 || Create folder button
            let btn_w = 0.28;
            let btn_h = 0.1;
            let mut btn_r = Rect::new(1.0 - btn_w - 0.04, bottom - btn_h - 0.02, btn_w, btn_h);
            let ct = btn_r.center();
            self.create_btn.render_shadow(ui, btn_r, t, |ui, path| {
                ui.fill_path(&path, semi_black(0.5));
            });
            ui.text(tl!("create"))
                .pos(ct.x, ct.y)
                .anchor(0.5, 0.5)
                .no_baseline()
                .size(0.45)
                .color(semi_white(0.9))
                .draw();

            btn_r.x -= btn_w + 0.02;
            let ct = btn_r.center();
            self.import_btn.render_shadow(ui, btn_r, t, |ui, path| {
                ui.fill_path(&path, semi_black(0.5));
            });
            ui.text(tl!("import"))
                .pos(ct.x, ct.y)
                .anchor(0.5, 0.5)
                .no_baseline()
                .size(0.45)
                .color(semi_white(0.9))
                .draw();
        });

        let rt = s.rt;
        if self.side_enter_time.is_finite() {
            let p = ((rt - self.side_enter_time.abs()) / INFO_TRANSIT).min(1.);
            let p = 1. - (1f32 - p).powi(3);
            let p = if self.side_enter_time < 0. { 1. - p } else { p };
            ui.fill_rect(ui.screen_rect(), semi_black(p * 0.6));
            let w = INFO_WIDTH;
            let lf = f32::tween(&1.04, &(1. - w), p);
            ui.scope(|ui| {
                ui.dx(lf);
                ui.dy(-ui.top);
                let r = Rect::new(-0.2, 0., 0.2 + w, ui.top * 2.);
                ui.fill_rect(r, (Color::default(), (r.x, r.y), Color::new(0., 0., 0., p * 0.7), (r.right(), r.y)));
                self.render_info(ui, rt);
            });
        }

        self.edit_menu.render(ui, t, 1.);
        self.operations_menu.render(ui, t, 1.);
        self.cloud_menu.render(ui, t, 1.);

        if self.has_task() {
            ui.full_loading("", t);
        }

        Ok(())
    }

    fn render_top(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        let rt = s.rt;
        if self.side_enter_time.is_finite() {
            let p = ((rt - self.side_enter_time.abs()) / INFO_TRANSIT).min(1.);
            let p = 1. - (1f32 - p).powi(3);
            let p = if self.side_enter_time < 0. { 1. - p } else { p };
            ui.fill_rect(ui.screen_rect(), semi_black(p * 0.6));
            let w = INFO_WIDTH;
            let lf = f32::tween(&1.04, &(1. - w), p);
            ui.scope(|ui| {
                ui.dx(lf);
                ui.dy(-ui.top);
                let r = Rect::new(-0.2, 0., 0.2 + w, ui.top * 2.);
                ui.fill_rect(r, (Color::default(), (r.x, r.y), Color::new(0., 0., 0., p * 0.7), (r.right(), r.y)));
                self.render_info(ui, rt);
            });
        }

        self.sf.render(ui, s.t);
        Ok(())
    }

    fn next_page(&mut self) -> NextPage {
        self.next_page.take().unwrap_or_default()
    }
}
