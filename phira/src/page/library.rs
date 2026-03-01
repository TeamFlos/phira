prpr_l10n::tl_file!("library");

use super::{BriefChartInfo, ChartItem, ChartType, CollectionPage, Illustration, NextPage, Page, SharedState, BLACK_TEXTURE};
use crate::{
    charts_view::{ChartDisplayItem, ChartsView, NEED_UPDATE},
    client::{Chart, Client},
    dir, get_data, get_data_mut,
    icons::Icons,
    popup::Popup,
    rate::RateDialog,
    save_data,
    scene::{check_read_tos_and_policy, confirm_delete, ChartOrder, JUST_LOADED_TOS, ORDERS},
    tabs::{Tabs, TitleFn},
    tags::TagsDialog,
};
use anyhow::{anyhow, Result};
use macroquad::prelude::*;
use prpr::{
    ext::{poll_future, semi_black, JoinToString, LocalTask, RectExt, SafeTexture, ScaleType},
    scene::{request_file, request_input, return_file, return_input, show_error, show_message, take_file, take_input, NextScene},
    task::Task,
    ui::{button_hit, DRectButton, RectButton, Ui},
};
use std::{
    any::Any,
    borrow::Cow,
    ops::Deref,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tap::Tap;
use tracing::warn;

const PAGE_NUM: u64 = 28;

// 加载文件夹图标（普通图片文件）
fn load_folder_icon(path: String, def: SafeTexture) -> Illustration {
    let notify = Arc::new(tokio::sync::Notify::new());
    Illustration {
        texture: (def.clone(), def),
        notify: Arc::clone(&notify),
        task: Some(Task::new(async move {
            notify.notified().await;
            // 直接加载图片文件
            let img = image::load_from_memory(&std::fs::read(&path)?)?;
            let thumbnail = crate::images::Images::thumbnail(&img);
            Ok((thumbnail, Some(img)))
        })),
        loaded: Arc::default(),
        load_time: f32::NAN,
    }
}

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
    create_folder_btn: DRectButton,
    back_btn: DRectButton,
    rename_folder_btn: DRectButton,
    change_icon_btn: DRectButton,
    delete_folder_btn: DRectButton,

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

    // 文件夹导航相关
    current_folder: Vec<String>, // 当前文件夹路径，如 ["folder1", "subfolder2"]
    folder_menu: Popup,
    need_show_folder_menu: bool,
    selected_chart_path: Option<String>,   // 被选中铺面的 local_path
    should_delete_folder: Arc<AtomicBool>, // 是否确认删除文件夹
    charts_count_before_import: usize,     // 导入前的铺面数量
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
            create_folder_btn: DRectButton::new(),
            back_btn: DRectButton::new(),
            rename_folder_btn: DRectButton::new(),
            change_icon_btn: DRectButton::new(),
            delete_folder_btn: DRectButton::new(),

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

            current_folder: Vec::new(),
            folder_menu: Popup::new(),
            need_show_folder_menu: false,
            selected_chart_path: None,
            should_delete_folder: Arc::new(AtomicBool::new(false)),
            charts_count_before_import: 0,
        })
    }
}

impl LibraryPage {
    // 收集所有文件夹路径（避免代码重复）
    fn collect_all_folders(&self, s: &SharedState) -> Vec<String> {
        let mut all_folders: std::collections::HashSet<String> = std::collections::HashSet::new();

        // 从铺面的 folder 字段中收集
        for chart in &s.charts_local {
            if let Some(folder) = &chart.folder {
                let parts: Vec<&str> = folder.split('/').collect();
                for i in 1..=parts.len() {
                    all_folders.insert(parts[..i].join("/"));
                }
            }
        }

        // 从 s.folders 中收集已创建的空文件夹
        for folder_path in s.folders.keys() {
            if !folder_path.is_empty() {
                all_folders.insert(folder_path.clone());
                let parts: Vec<&str> = folder_path.split('/').collect();
                for i in 1..parts.len() {
                    all_folders.insert(parts[..i].join("/"));
                }
            }
        }

        let mut sorted_folders: Vec<_> = all_folders.into_iter().collect();
        sorted_folders.sort();
        sorted_folders
    }

    // 验证文件夹名称
    fn validate_folder_name(name: &str) -> Result<String, &'static str> {
        let trimmed = name.trim();

        if trimmed.is_empty() {
            return Err("folder-name-empty");
        }

        // 检查非法字符
        if trimmed.contains('/') || trimmed.contains('\\') {
            return Err("folder-name-invalid-slash");
        }

        // 检查其他可能有问题的字符
        if trimmed.contains(':')
            || trimmed.contains('*')
            || trimmed.contains('?')
            || trimmed.contains('"')
            || trimmed.contains('<')
            || trimmed.contains('>')
            || trimmed.contains('|')
        {
            return Err("folder-name-invalid-char");
        }

        Ok(trimmed.to_string())
    }

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

            // 构建当前文件夹路径
            let current_path = if self.current_folder.is_empty() {
                None
            } else {
                Some(self.current_folder.join("/"))
            };

            // 收集当前文件夹下的子文件夹（包括空文件夹）
            let mut subfolders: std::collections::HashSet<String> = std::collections::HashSet::new();

            // 从铺面的 folder 字段中提取子文件夹
            for chart in &s.charts_local {
                if let Some(folder) = &chart.folder {
                    if folder.is_empty() {
                        continue;
                    }

                    if let Some(ref cur_path) = current_path {
                        // 在子文件夹中，找直接子文件夹
                        if let Some(remaining) = folder.strip_prefix(cur_path) {
                            if let Some(remaining) = remaining.strip_prefix('/') {
                                if let Some(next_folder) = remaining.split('/').next() {
                                    if !next_folder.is_empty() {
                                        subfolders.insert(next_folder.to_string());
                                    }
                                }
                            }
                        }
                    } else {
                        // 在根目录，找第一级文件夹
                        if let Some(first_folder) = folder.split('/').next() {
                            if !first_folder.is_empty() {
                                subfolders.insert(first_folder.to_string());
                            }
                        }
                    }
                }
            }

            // 从 SharedState.folders 中添加已创建的空文件夹
            for folder_path in s.folders.keys() {
                // 检查这个文件夹是否应该显示在当前目录下
                if let Some(ref cur_path) = current_path {
                    // 在子文件夹中
                    if let Some(remaining) = folder_path.strip_prefix(cur_path) {
                        if let Some(remaining) = remaining.strip_prefix('/') {
                            // 这是直接子文件夹（不包含更多的 '/'）
                            if !remaining.contains('/') && !remaining.is_empty() {
                                subfolders.insert(remaining.to_string());
                            }
                        }
                    }
                } else {
                    // 在根目录，只显示不包含 '/' 的文件夹
                    if !folder_path.contains('/') && !folder_path.is_empty() {
                        subfolders.insert(folder_path.clone());
                    }
                }
            }

            // 添加文件夹卡片（按字母顺序）
            let mut sorted_folders: Vec<_> = subfolders.into_iter().collect();
            sorted_folders.sort();
            for folder_name in sorted_folders {
                // 构建完整的文件夹路径
                let full_folder_path = if let Some(ref cur_path) = current_path {
                    format!("{}/{}", cur_path, folder_name)
                } else {
                    folder_name.clone()
                };

                // 加载自定义图标或使用默认黑色背景
                let illu = if let Some(icon_path) = s.folder_icons.get(&full_folder_path) {
                    // 使用自定义图标
                    load_folder_icon(icon_path.to_string(), BLACK_TEXTURE.clone())
                } else {
                    // 使用默认黑色背景
                    Illustration::from_done(BLACK_TEXTURE.clone())
                };

                let folder_chart = ChartItem {
                    info: BriefChartInfo {
                        id: None,
                        uploader: None,
                        name: folder_name.clone(),
                        level: "Folder".to_string(),
                        difficulty: 0.0,
                        charter: String::new(),
                        composer: String::new(),
                        illustrator: String::new(),
                        created: None,
                        updated: None,
                        chart_updated: None,
                        intro: String::new(),
                        has_unlock: false,
                    },
                    local_path: None,
                    illu,
                    chart_type: ChartType::Imported,
                    folder: Some(folder_name),
                };
                charts.push(ChartDisplayItem::new(Some(folder_chart), None));
            }

            // 添加当前文件夹下的铺面
            for chart in &s.charts_local {
                // 搜索过滤
                if !search.is_empty() && !chart.info.name.contains(&search) {
                    continue;
                }

                // 检查是否在当前文件夹
                let in_current_folder = if let Some(ref cur_path) = current_path {
                    chart.folder.as_ref().map(|f| f == cur_path).unwrap_or(false)
                } else {
                    chart.folder.is_none()
                };

                if in_current_folder {
                    charts.push(ChartDisplayItem::new(Some(chart.clone()), None));
                }
            }

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
        if self.folder_menu.showing() {
            self.folder_menu.touch(touch, t);
            return Ok(true);
        }
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
                // 返回上一级按钮
                if !self.current_folder.is_empty() && self.back_btn.touch(touch, t) {
                    button_hit();
                    self.current_folder.pop();
                    self.sync_local(s);
                    return Ok(true);
                }
                // 重命名文件夹按钮
                if !self.current_folder.is_empty() && self.rename_folder_btn.touch(touch, t) {
                    button_hit();
                    let current_name = self.current_folder.last().unwrap().clone();
                    request_input("rename_folder", &current_name);
                    return Ok(true);
                }
                // 更改图标按钮
                if !self.current_folder.is_empty() && self.change_icon_btn.touch(touch, t) {
                    button_hit();
                    request_file("change_folder_icon");
                    return Ok(true);
                }
                // 删除文件夹按钮
                if !self.current_folder.is_empty() && self.delete_folder_btn.touch(touch, t) {
                    button_hit();
                    confirm_delete(Arc::clone(&self.should_delete_folder));
                    return Ok(true);
                }
                // 新建文件夹按钮
                if self.create_folder_btn.touch(touch, t) {
                    button_hit();
                    request_input("create_folder", "");
                    return Ok(true);
                }
                // 导入按钮
                if self.import_btn.touch(touch, t) {
                    // 只在文件夹内导入时记录铺面数量
                    if !self.current_folder.is_empty() {
                        self.charts_count_before_import = get_data().charts.len();
                    }
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

        // 检查是否确认删除文件夹
        if self.should_delete_folder.swap(false, Ordering::SeqCst) {
            let folder_path = self.current_folder.join("/");
            let data = get_data_mut();

            // 删除文件夹本身
            data.folders.remove(&folder_path);

            // 删除所有子文件夹
            let prefix = format!("{}/", folder_path);
            data.folders.retain(|k, _| !k.starts_with(&prefix));

            // 删除文件夹图标
            data.folder_icons.remove(&folder_path);
            data.folder_icons.retain(|k, _| !k.starts_with(&prefix));

            // 收集要删除的铺面路径
            let charts_dir = dir::charts()?;
            let mut charts_to_remove = Vec::new();

            // 找到文件夹内的所有铺面
            for (idx, chart) in data.charts.iter().enumerate() {
                if let Some(ref f) = chart.folder {
                    if f == &folder_path || f.starts_with(&prefix) {
                        charts_to_remove.push((idx, chart.local_path.clone()));
                    }
                }
            }

            // 从后往前删除（避免索引变化）
            charts_to_remove.reverse();
            for (idx, local_path) in charts_to_remove {
                // 删除文件系统中的铺面目录
                let path = format!("{}/{}", charts_dir, local_path);
                if let Err(e) = std::fs::remove_dir_all(&path) {
                    warn!("Failed to delete chart directory {}: {:?}", path, e);
                }
                // 从数据中删除
                data.charts.remove(idx);
            }

            save_data()?;
            show_message(tl!("folder-deleted")).ok();

            // 返回上一级
            self.current_folder.pop();
            s.reload_local_charts();
            self.sync_local(s);
        }

        // 处理文件选择器返回的文件（更改文件夹图标）
        if let Some((id, path)) = take_file() {
            if id == "change_folder_icon" && !self.current_folder.is_empty() {
                let folder_path = self.current_folder.join("/");
                let data = get_data_mut();

                // 保存图标文件路径
                data.folder_icons.insert(folder_path.clone(), path);
                save_data().ok();
                show_message(tl!("icon-changed")).ok();

                // 重新加载以更新图标
                s.reload_local_charts();
                self.sync_local(s);
            } else {
                // 不是我们的文件，返回给其他处理器
                return_file(id, path);
            }
        }

        // 处理文件夹点击
        if let Some(folder_name) = self.tabs.selected_mut().view.clicked_folder.take() {
            self.current_folder.push(folder_name);
            self.sync_local(s);
        }

        // 处理菜单点击
        if let Some(_chart_index) = self.tabs.selected_mut().view.clicked_menu.take() {
            self.selected_chart_path = self.tabs.selected_mut().view.clicked_chart_path.take();

            // 构建文件夹选项列表
            let mut folder_options = vec![tl!("root-folder").to_string()];
            let sorted_folders = self.collect_all_folders(s);
            folder_options.extend(sorted_folders);

            self.folder_menu = Popup::new().with_options(folder_options);
            self.need_show_folder_menu = true;
        }

        // 处理文件夹菜单选择
        if self.folder_menu.changed() {
            if let Some(chart_path) = self.selected_chart_path.take() {
                let selected = self.folder_menu.selected();
                let folder = if selected == 0 {
                    None // 根目录
                } else {
                    let sorted_folders = self.collect_all_folders(s);
                    sorted_folders.get(selected - 1).cloned()
                };

                // 更新铺面的文件夹
                let data = get_data_mut();
                if let Some(local_chart) = data.charts.iter_mut().find(|c| c.local_path == chart_path) {
                    local_chart.folder = folder.clone();
                    save_data().ok();
                    show_message(if let Some(f) = &folder {
                        tl!("moved-to-folder", "folder" => f.clone())
                    } else {
                        tl!("moved-to-root").to_string()
                    })
                    .ok();
                    // 重新加载本地铺面数据
                    s.reload_local_charts();
                    // 重新同步显示
                    self.sync_local(s);
                }
            }
        }

        // 处理输入
        if let Some((id, text)) = take_input() {
            if id == "search" {
                self.search_str = text;
                if is_local {
                    self.sync_local(s);
                } else {
                    self.current_page = 0;
                    self.load_online();
                }
            } else if id == "create_folder" {
                match Self::validate_folder_name(&text) {
                    Ok(folder_name) => {
                        // 构建新文件夹的完整路径
                        let new_folder_path = if self.current_folder.is_empty() {
                            folder_name.clone()
                        } else {
                            format!("{}/{}", self.current_folder.join("/"), folder_name)
                        };

                        // 检查是否已存在
                        let data = get_data_mut();
                        if data.folders.contains_key(&new_folder_path) || s.charts_local.iter().any(|c| c.folder.as_ref() == Some(&new_folder_path)) {
                            show_error(anyhow::anyhow!("Folder already exists"));
                        } else {
                            data.folders.insert(new_folder_path.clone(), false);
                            save_data().ok();
                            show_message(tl!("folder-created", "name" => folder_name)).ok();
                            s.reload_local_charts();
                            self.sync_local(s);
                        }
                    }
                    Err(err_msg) => {
                        show_error(anyhow::anyhow!(err_msg));
                    }
                }
            } else if id == "rename_folder" {
                if !self.current_folder.is_empty() {
                    match Self::validate_folder_name(&text) {
                        Ok(folder_name) => {
                            let old_folder_path = self.current_folder.join("/");
                            let mut new_folder_parts = self.current_folder.clone();
                            new_folder_parts.pop();
                            new_folder_parts.push(folder_name.clone());
                            let new_folder_path = new_folder_parts.join("/");

                            let data = get_data_mut();

                            // 检查新名称是否与现有文件夹冲突（除了自己）
                            if new_folder_path != old_folder_path
                                && (data.folders.contains_key(&new_folder_path)
                                    || s.charts_local.iter().any(|c| c.folder.as_ref() == Some(&new_folder_path)))
                            {
                                show_error(anyhow::anyhow!("Folder name already exists"));
                            } else {
                                // 重命名文件夹本身
                                if data.folders.remove(&old_folder_path).is_some() {
                                    data.folders.insert(new_folder_path.clone(), false);
                                }

                                // 重命名所有子文件夹
                                let old_prefix = format!("{}/", old_folder_path);
                                let new_prefix = format!("{}/", new_folder_path);
                                let mut folders_to_rename = Vec::new();
                                for (k, v) in data.folders.iter() {
                                    if k.starts_with(&old_prefix) {
                                        folders_to_rename.push((k.clone(), *v));
                                    }
                                }
                                for (old_key, value) in folders_to_rename {
                                    data.folders.remove(&old_key);
                                    let new_key = old_key.replace(&old_prefix, &new_prefix);
                                    data.folders.insert(new_key, value);
                                }

                                // 重命名文件夹图标
                                if let Some(icon) = data.folder_icons.remove(&old_folder_path) {
                                    data.folder_icons.insert(new_folder_path.clone(), icon);
                                }
                                let mut icons_to_rename = Vec::new();
                                for (k, v) in data.folder_icons.iter() {
                                    if k.starts_with(&old_prefix) {
                                        icons_to_rename.push((k.clone(), v.clone()));
                                    }
                                }
                                for (old_key, value) in icons_to_rename {
                                    data.folder_icons.remove(&old_key);
                                    let new_key = old_key.replace(&old_prefix, &new_prefix);
                                    data.folder_icons.insert(new_key, value);
                                }

                                // 更新所有铺面的文件夹路径
                                for chart in data.charts.iter_mut() {
                                    if let Some(ref folder) = chart.folder {
                                        if folder == &old_folder_path {
                                            chart.folder = Some(new_folder_path.clone());
                                        } else if folder.starts_with(&old_prefix) {
                                            chart.folder = Some(folder.replace(&old_prefix, &new_prefix));
                                        }
                                    }
                                }

                                save_data().ok();
                                show_message(tl!("folder-renamed", "name" => folder_name)).ok();

                                // 更新当前文件夹路径
                                self.current_folder = new_folder_parts;

                                // 重新加载本地铺面数据
                                s.reload_local_charts();
                                // 刷新显示
                                self.sync_local(s);
                            }
                        }
                        Err(err_msg) => {
                            show_error(anyhow::anyhow!(err_msg));
                        }
                    }
                }
            } else {
                return_input(id, text);
            }
        }

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
        self.folder_menu.update(t);
        for chart in &mut s.charts_local {
            chart.illu.settle(t);
        }
        if self.tabs.selected_mut().view.update(t)? {
            self.load_online();
        }
        if self.tabs.selected_mut().view.need_update() {
            // 导入到当前文件夹
            if !self.current_folder.is_empty() && self.charts_count_before_import > 0 {
                let target_folder = self.current_folder.join("/");
                let data = get_data_mut();

                // 只移动新导入的铺面（索引大于等于导入前的数量）
                let current_count = data.charts.len();
                if current_count > self.charts_count_before_import {
                    for idx in self.charts_count_before_import..current_count {
                        if data.charts[idx].folder.is_none() {
                            data.charts[idx].folder = Some(target_folder.clone());
                        }
                    }
                    save_data().ok();
                }
            }

            // 重置计数器
            self.charts_count_before_import = 0;

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
                    if !self.current_folder.is_empty() {
                        // 在子文件夹中，显示所有按钮在一行（6个按钮）
                        let btn_h = r.h; // 使用搜索框的高度
                        let btn_w = btn_h; // 按钮宽度等于高度，保持正方形
                        let spacing = 0.012; // 增加按钮间距
                        let start_x = r.right() - btn_w * 6.0 - spacing * 5.0;

                        let mut btn_r = Rect::new(start_x, r.y, btn_w, btn_h);

                        // 返回按钮
                        let ct = btn_r.center();
                        self.back_btn.render_shadow(ui, btn_r, t, |ui, path| {
                            ui.fill_path(&path, semi_black(0.4));
                        });
                        // 使用返回图标
                        let icon_size = btn_h * 0.6;
                        let icon_r = Rect::new(ct.x - icon_size / 2.0, ct.y - icon_size / 2.0, icon_size, icon_size);
                        ui.fill_rect(icon_r, (*self.icons.back, icon_r, ScaleType::Fit));

                        // 重命名按钮
                        btn_r.x += btn_w + spacing;
                        let ct = btn_r.center();
                        self.rename_folder_btn.render_shadow(ui, btn_r, t, |ui, path| {
                            ui.fill_path(&path, semi_black(0.4));
                        });
                        // 使用编辑图标
                        let icon_size = btn_h * 0.6;
                        let icon_r = Rect::new(ct.x - icon_size / 2.0, ct.y - icon_size / 2.0, icon_size, icon_size);
                        ui.fill_rect(icon_r, (*self.icons.edit, icon_r, ScaleType::Fit));

                        // 更改图标按钮
                        btn_r.x += btn_w + spacing;
                        let ct = btn_r.center();
                        self.change_icon_btn.render_shadow(ui, btn_r, t, |ui, path| {
                            ui.fill_path(&path, semi_black(0.4));
                        });
                        // 绘制图片图标（四个角的框 + 山和太阳）
                        let icon_size = btn_h * 0.5;
                        let corner_len = icon_size * 0.25; // 角的长度
                        let line_width = icon_size * 0.08; // 线条宽度

                        // 绘制四个角
                        // 左上角
                        ui.fill_rect(
                            Rect::new(ct.x - icon_size / 2.0, ct.y - icon_size / 2.0, corner_len, line_width),
                            Color::new(1.0, 1.0, 1.0, 1.0),
                        );
                        ui.fill_rect(
                            Rect::new(ct.x - icon_size / 2.0, ct.y - icon_size / 2.0, line_width, corner_len),
                            Color::new(1.0, 1.0, 1.0, 1.0),
                        );

                        // 右上角
                        ui.fill_rect(
                            Rect::new(ct.x + icon_size / 2.0 - corner_len, ct.y - icon_size / 2.0, corner_len, line_width),
                            Color::new(1.0, 1.0, 1.0, 1.0),
                        );
                        ui.fill_rect(
                            Rect::new(ct.x + icon_size / 2.0 - line_width, ct.y - icon_size / 2.0, line_width, corner_len),
                            Color::new(1.0, 1.0, 1.0, 1.0),
                        );

                        // 左下角
                        ui.fill_rect(
                            Rect::new(ct.x - icon_size / 2.0, ct.y + icon_size / 2.0 - line_width, corner_len, line_width),
                            Color::new(1.0, 1.0, 1.0, 1.0),
                        );
                        ui.fill_rect(
                            Rect::new(ct.x - icon_size / 2.0, ct.y + icon_size / 2.0 - corner_len, line_width, corner_len),
                            Color::new(1.0, 1.0, 1.0, 1.0),
                        );

                        // 右下角
                        ui.fill_rect(
                            Rect::new(ct.x + icon_size / 2.0 - corner_len, ct.y + icon_size / 2.0 - line_width, corner_len, line_width),
                            Color::new(1.0, 1.0, 1.0, 1.0),
                        );
                        ui.fill_rect(
                            Rect::new(ct.x + icon_size / 2.0 - line_width, ct.y + icon_size / 2.0 - corner_len, line_width, corner_len),
                            Color::new(1.0, 1.0, 1.0, 1.0),
                        );

                        // 绘制太阳（右上角的小圆）
                        let sun_size = icon_size * 0.12;
                        ui.fill_circle(ct.x + icon_size * 0.15, ct.y - icon_size * 0.15, sun_size, Color::new(1.0, 1.0, 1.0, 1.0));

                        // 绘制山（两个三角形）
                        let mountain_height = icon_size * 0.2;
                        let mountain_base = icon_size * 0.25;

                        // 左边的山（较高）
                        let m1_x = ct.x - icon_size * 0.15;
                        let m1_y = ct.y + icon_size * 0.15;
                        ui.fill_rect(
                            Rect::new(m1_x - mountain_base / 2.0, m1_y - mountain_height, mountain_base, mountain_height),
                            Color::new(1.0, 1.0, 1.0, 0.8),
                        );

                        // 右边的山（较矮）
                        let m2_x = ct.x + icon_size * 0.1;
                        let m2_y = ct.y + icon_size * 0.15;
                        let m2_height = mountain_height * 0.7;
                        ui.fill_rect(
                            Rect::new(m2_x - mountain_base / 2.0, m2_y - m2_height, mountain_base * 0.8, m2_height),
                            Color::new(1.0, 1.0, 1.0, 0.8),
                        );

                        // 删除按钮
                        btn_r.x += btn_w + spacing;
                        let ct = btn_r.center();
                        self.delete_folder_btn.render_shadow(ui, btn_r, t, |ui, path| {
                            ui.fill_path(&path, semi_black(0.4));
                        });
                        // 使用删除图标
                        let icon_size = btn_h * 0.6;
                        let icon_r = Rect::new(ct.x - icon_size / 2.0, ct.y - icon_size / 2.0, icon_size, icon_size);
                        ui.fill_rect(icon_r, (*self.icons.delete, icon_r, ScaleType::Fit));

                        // 新建文件夹按钮
                        btn_r.x += btn_w + spacing;
                        let ct = btn_r.center();
                        self.create_folder_btn.render_shadow(ui, btn_r, t, |ui, path| {
                            ui.fill_path(&path, semi_black(0.4));
                        });
                        // 绘制加号图标
                        let icon_size = btn_h * 0.5;
                        let line_width = icon_size * 0.15;
                        // 竖线
                        ui.fill_rect(
                            Rect::new(ct.x - line_width / 2.0, ct.y - icon_size / 2.0, line_width, icon_size),
                            Color::new(1.0, 1.0, 1.0, 1.0),
                        );
                        // 横线
                        ui.fill_rect(
                            Rect::new(ct.x - icon_size / 2.0, ct.y - line_width / 2.0, icon_size, line_width),
                            Color::new(1.0, 1.0, 1.0, 1.0),
                        );

                        // 导入按钮
                        btn_r.x += btn_w + spacing;
                        let ct = btn_r.center();
                        self.import_btn.render_shadow(ui, btn_r, t, |ui, path| {
                            ui.fill_path(&path, semi_black(0.4));
                        });
                        // 使用下载图标
                        let icon_size = btn_h * 0.6;
                        let icon_r = Rect::new(ct.x - icon_size / 2.0, ct.y - icon_size / 2.0, icon_size, icon_size);
                        ui.fill_rect(icon_r, (*self.icons.download, icon_r, ScaleType::Fit));
                    } else {
                        // 在根目录，只显示导入和新建文件夹按钮
                        let btn_h = r.h; // 使用搜索框的高度
                        let btn_w = btn_h; // 按钮宽度等于高度，保持正方形
                        let spacing = 0.015; // 增加按钮间距

                        // 新建文件夹按钮
                        let btn_r = Rect::new(r.right() - btn_w * 2.0 - spacing, r.y, btn_w, btn_h);
                        let ct = btn_r.center();
                        self.create_folder_btn.render_shadow(ui, btn_r, t, |ui, path| {
                            ui.fill_path(&path, semi_black(0.4));
                        });
                        // 绘制加号图标
                        let icon_size = btn_h * 0.5;
                        let line_width = icon_size * 0.15;
                        // 竖线
                        ui.fill_rect(
                            Rect::new(ct.x - line_width / 2.0, ct.y - icon_size / 2.0, line_width, icon_size),
                            Color::new(1.0, 1.0, 1.0, 1.0),
                        );
                        // 横线
                        ui.fill_rect(
                            Rect::new(ct.x - icon_size / 2.0, ct.y - line_width / 2.0, icon_size, line_width),
                            Color::new(1.0, 1.0, 1.0, 1.0),
                        );

                        // 导入按钮
                        let btn_r = Rect::new(r.right() - btn_w, r.y, btn_w, btn_h);
                        let ct = btn_r.center();
                        self.import_btn.render_shadow(ui, btn_r, t, |ui, path| {
                            ui.fill_path(&path, semi_black(0.4));
                        });
                        // 使用下载图标
                        let icon_size = btn_h * 0.6;
                        let icon_r = Rect::new(ct.x - icon_size / 2.0, ct.y - icon_size / 2.0, icon_size, icon_size);
                        ui.fill_rect(icon_r, (*self.icons.download, icon_r, ScaleType::Fit));
                    }
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
                // 显示文件夹选择菜单
                if self.need_show_folder_menu {
                    self.need_show_folder_menu = false;
                    self.folder_menu.set_bottom(false);
                    let menu_w = 0.35;
                    let menu_h = 0.5;
                    let menu_x = 0.0_f32.max(-0.9).min(0.9 - menu_w);
                    let menu_y = -0.3;
                    let menu_r = Rect::new(menu_x, menu_y, menu_w, menu_h);
                    self.folder_menu.show(ui, t, menu_r);
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
        self.folder_menu.render(ui, t, 1.);
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
