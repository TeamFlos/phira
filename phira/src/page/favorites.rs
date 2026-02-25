prpr_l10n::tl_file!("library");

use super::{local_illustration, Illustration, NextPage, Page, SharedState};
use crate::{data::DEFAULT_FAVORITES_KEY, get_data, get_data_mut, save_data};
use anyhow::Result;
use image::DynamicImage;
use macroquad::prelude::*;
use prpr::{
    ext::{semi_black, semi_white, RectExt, SafeTexture, BLACK_TEXTURE},
    scene::{request_file, request_input, show_message, take_file, take_input},
    task::Task,
    ui::{button_hit, DRectButton, Dialog, RectButton, Scroll, Ui},
};
use std::{borrow::Cow, cell::RefCell, sync::Arc};
use tokio::sync::Notify;

// 收藏夹页面选择结果 || Favorites page selection result
// None = 显示全部 || show all, Some(folder_name) = 过滤指定收藏夹 || filter by folder
thread_local! {
    pub static FAV_PAGE_RESULT: RefCell<Option<Option<String>>> = RefCell::default();
}

const CARD_WIDTH: f32 = 0.41;
const CARD_HEIGHT: f32 = 0.32;
const CARD_PAD: f32 = 0.033;

// 收藏夹文件夹项 || Folder item
struct FolderItem {
    // 收藏夹名称，None 表示"显示全部谱面" || Folder name, None = "show all charts"
    folder_name: Option<String>,
    display_name: String,
    illu: Illustration,
    btn: RectButton,
    menu_btn: DRectButton,
}

pub struct FavoritesPage {
    folders: Vec<FolderItem>,
    scroll: Scroll,

    bg_tex: Option<SafeTexture>,
    bg_task: Option<Task<Result<DynamicImage>>>,

    create_btn: DRectButton,

    // 编辑状态 || Editing state
    editing_folder: Option<String>,
    edit_cover_btn: DRectButton,
    edit_rename_btn: DRectButton,
    edit_delete_btn: DRectButton,

    next_page: Option<NextPage>,
    need_rebuild: bool,
}

impl FavoritesPage {
    pub fn new() -> Self {
        let mut page = Self {
            folders: Vec::new(),
            scroll: Scroll::new(),
            bg_tex: None,
            bg_task: Some(Task::new(async move {
                let bytes = load_file("background.jpg").await.unwrap_or_default();
                if bytes.is_empty() {
                    Ok(DynamicImage::new_rgba8(1, 1))
                } else {
                    Ok(image::load_from_memory(&bytes)?)
                }
            })),
            create_btn: DRectButton::new(),
            editing_folder: None,
            edit_cover_btn: DRectButton::new(),
            edit_rename_btn: DRectButton::new(),
            edit_delete_btn: DRectButton::new(),
            next_page: None,
            need_rebuild: true,
        };
        page.rebuild_folders();
        page
    }

    // 根据当前收藏夹数据重建文件夹列表 || Rebuild folder list from current data
    fn rebuild_folders(&mut self) {
        let tex = BLACK_TEXTURE.clone();
        let data = get_data();
        let mut folders = Vec::new();

        // "显示全部谱面"卡片 || "Show all charts" card
        let all_illu = if let Some(ref bg) = self.bg_tex {
            Illustration::from_done(bg.clone())
        } else {
            Illustration::from_done(tex.clone())
        };
        folders.push(FolderItem {
            folder_name: None,
            display_name: tl!("favorites-show-all").to_string(),
            illu: all_illu,
            btn: RectButton::new(),
            menu_btn: DRectButton::new(),
        });

        // 默认收藏夹 || Default favorites folder
        if data.favorites.folders.contains_key(DEFAULT_FAVORITES_KEY) {
            let last_path = Self::last_chart_path(DEFAULT_FAVORITES_KEY);
            let cover = data.favorites.covers.get(DEFAULT_FAVORITES_KEY).cloned();
            let illu = Self::make_cover_illu(&tex, last_path.as_deref(), cover.as_deref());
            folders.push(FolderItem {
                folder_name: Some(DEFAULT_FAVORITES_KEY.to_string()),
                display_name: tl!("favorites-default").to_string(),
                illu,
                btn: RectButton::new(),
                menu_btn: DRectButton::new(),
            });
        }

        // 自定义收藏夹 || Custom folders
        for name in data.favorites.custom_folder_names() {
            let last_path = Self::last_chart_path(&name);
            let cover = data.favorites.covers.get(&name).cloned();
            let illu = Self::make_cover_illu(&tex, last_path.as_deref(), cover.as_deref());
            folders.push(FolderItem {
                folder_name: Some(name.clone()),
                display_name: name,
                illu,
                btn: RectButton::new(),
                menu_btn: DRectButton::new(),
            });
        }

        self.folders = folders;
        self.need_rebuild = false;
    }

    // 获取收藏夹中最新谱面的 local_path || Get the last chart path in a folder
    fn last_chart_path(folder: &str) -> Option<String> {
        let data = get_data();
        let paths = data.favorites.get_paths(folder);
        for p in paths.iter().rev() {
            if data.charts.iter().any(|c| &c.local_path == p) {
                return Some(p.clone());
            }
        }
        None
    }

    fn make_cover_illu(tex: &SafeTexture, last_chart_path: Option<&str>, custom_cover: Option<&str>) -> Illustration {
        // 优先使用自定义封面 || Prefer custom cover
        if let Some(cover_path) = custom_cover {
            let path = cover_path.to_string();
            let notify = Arc::new(Notify::new());
            let notify_clone = Arc::clone(&notify);
            return Illustration {
                texture: (tex.clone(), tex.clone()),
                notify,
                task: Some(Task::new(async move {
                    notify_clone.notified().await;
                    let bytes = tokio::fs::read(&path).await?;
                    let img = image::load_from_memory(&bytes)?;
                    Ok((img, None))
                })),
                loaded: Arc::default(),
                load_time: f32::NAN,
            };
        }
        // 其次使用最新收藏谱面封面 || Fallback to latest chart illustration
        if let Some(path) = last_chart_path {
            local_illustration(path.to_string(), tex.clone(), false)
        } else {
            Illustration::from_done(tex.clone())
        }
    }

    fn has_menu(folder_name: &Option<String>) -> bool {
        folder_name.is_some()
    }
}

impl Page for FavoritesPage {
    fn label(&self) -> Cow<'static, str> {
        tl!("favorites")
    }

    fn on_back_pressed(&mut self, _s: &mut SharedState) -> bool {
        // 编辑模式下先退出编辑 || Exit editing mode first
        if self.editing_folder.is_some() {
            self.editing_folder = None;
            return true;
        }
        false
    }

    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        let t = s.t;
        let rt = s.rt;

        // 编辑栏按钮处理 || Edit bar button handling
        if let Some(ref editing) = self.editing_folder.clone() {
            let is_default = editing == DEFAULT_FAVORITES_KEY;
            if self.edit_cover_btn.touch(touch, t) {
                request_file("fav_cover");
                return Ok(true);
            }
            if !is_default && self.edit_rename_btn.touch(touch, t) {
                request_input("fav_rename", editing);
                return Ok(true);
            }
            if !is_default && self.edit_delete_btn.touch(touch, t) {
                let folder = editing.clone();
                Dialog::plain(tl!("favorites-delete"), tl!("favorites-delete-confirm"))
                    .buttons(vec![tl!("favorites-all").to_string(), tl!("favorites-delete").to_string()])
                    .listener(move |_dialog, pos| {
                        if pos == 1 {
                            get_data_mut().favorites.delete_folder(&folder);
                            get_data_mut().favorites.covers.remove(&folder);
                            let _ = save_data();
                            show_message(tl!("favorites-deleted")).ok();
                        }
                        false
                    })
                    .show();
                self.editing_folder = None;
                self.need_rebuild = true;
                return Ok(true);
            }
        }

        if self.create_btn.touch(touch, t) {
            request_input("fav_create", "");
            return Ok(true);
        }

        // 编辑按钮检测 || Edit button detection
        for folder in self.folders.iter_mut() {
            if Self::has_menu(&folder.folder_name) && folder.menu_btn.touch(touch, t) {
                if let Some(name) = &folder.folder_name {
                    button_hit();
                    self.editing_folder = Some(name.clone());
                    return Ok(true);
                }
            }
        }

        // 卡片点击检测 || Card click detection
        if touch.phase == TouchPhase::Ended {
            for folder in self.folders.iter() {
                if folder.btn.contains(touch.position) {
                    let folder_name = folder.folder_name.clone();
                    button_hit();
                    FAV_PAGE_RESULT.with(|it| *it.borrow_mut() = Some(folder_name));
                    self.next_page = Some(NextPage::Pop);
                    return Ok(true);
                }
            }
        }

        if self.scroll.touch(touch, rt) {
            return Ok(true);
        }

        Ok(false)
    }

    fn update(&mut self, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        self.scroll.update(s.rt);

        // 加载背景图 || Load background image
        if let Some(task) = &mut self.bg_task {
            if let Some(res) = task.take() {
                if let Ok(image) = res {
                    let tex: SafeTexture = image.into();
                    self.bg_tex = Some(tex);
                    self.need_rebuild = true;
                }
                self.bg_task = None;
            }
        }

        for folder in &mut self.folders {
            folder.illu.settle(t);
        }

        if self.need_rebuild {
            self.rebuild_folders();
        }

        // 处理输入事件 || Handle input events
        if let Some((id, text)) = take_input() {
            if id == "fav_create" {
                let name = text.trim().to_string();
                if name.is_empty() {
                    show_message(tl!("favorites-name-empty")).error();
                } else if get_data().favorites.folders.contains_key(&name) {
                    show_message(tl!("favorites-name-exists")).error();
                } else {
                    get_data_mut().favorites.create_folder(&name);
                    let _ = save_data();
                    show_message(tl!("favorites-created")).ok();
                    self.need_rebuild = true;
                }
            } else if id == "fav_rename" {
                let new_name = text.trim().to_string();
                if new_name.is_empty() {
                    show_message(tl!("favorites-name-empty")).error();
                } else if let Some(old_name) = &self.editing_folder {
                    let old = old_name.clone();
                    if get_data_mut().favorites.rename_folder(&old, &new_name) {
                        if let Some(cover) = get_data_mut().favorites.covers.remove(&old) {
                            get_data_mut().favorites.covers.insert(new_name.clone(), cover);
                        }
                        let _ = save_data();
                        show_message(tl!("favorites-renamed")).ok();
                        self.editing_folder = Some(new_name);
                        self.need_rebuild = true;
                    } else {
                        show_message(tl!("favorites-name-exists")).error();
                    }
                }
            }
        }

        // 处理文件选择（自定义封面） || Handle file selection (custom cover)
        if let Some((id, path)) = take_file() {
            if id == "fav_cover" {
                if let Some(ref folder) = self.editing_folder {
                    get_data_mut().favorites.covers.insert(folder.clone(), path);
                    let _ = save_data();
                    self.need_rebuild = true;
                }
            }
        }

        Ok(())
    }

    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        for folder in &self.folders {
            folder.illu.notify();
        }

        let edit_bar_height = if self.editing_folder.is_some() { 0.09 } else { 0.0 };

        s.render_fader(ui, |ui| {
            let top = -ui.top + 0.13;
            let bottom = ui.top - edit_bar_height;
            let content_h = bottom - top;

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
                        ui.fill_rect(r, semi_black(0.5));
                        let illu_r = r;
                        let alpha = folder.illu.alpha(t);
                        if alpha > 0. {
                            ui.fill_rect(illu_r, folder.illu.shading(illu_r, t));
                        }
                        ui.fill_rect(illu_r, semi_black(0.15));
                        ui.text(&folder.display_name)
                            .pos(r.x + 0.03, r.bottom() - 0.03)
                            .anchor(0., 1.)
                            .no_baseline()
                            .size(0.42)
                            .max_width(r.w - 0.06)
                            .color(semi_white(0.9))
                            .draw();

                        // 编辑按钮（左上角） || Edit button (top-left)
                        if Self::has_menu(&folder.folder_name) {
                            let menu_size = 0.05;
                            let menu_r = Rect::new(r.left() + menu_size - 0.03, r.y + 0.01, menu_size, menu_size);
                            folder.menu_btn.render_shadow(ui, menu_r, t, |ui, path| {
                                ui.fill_path(&path, semi_black(0.4));
                            });
                            ui.text("...")
                                .pos(menu_r.center().x, menu_r.center().y - 0.01)
                                .anchor(0.5, 0.5)
                                .no_baseline()
                                .size(0.5)
                                .color(semi_white(0.9))
                                .draw();
                        }

                        // 编辑中高亮边框 || Highlight border when editing
                        if self.editing_folder.as_ref() == folder.folder_name.as_ref() {
                            let border = r.feather(0.003);
                            ui.fill_rect(Rect::new(border.x, border.y, border.w, 0.003), semi_white(0.8));
                            ui.fill_rect(Rect::new(border.x, border.bottom() - 0.003, border.w, 0.003), semi_white(0.8));
                            ui.fill_rect(Rect::new(border.x, border.y, 0.003, border.h), semi_white(0.8));
                            ui.fill_rect(Rect::new(border.right() - 0.003, border.y, 0.003, border.h), semi_white(0.8));
                        }

                        // 谱面数量 || Chart count
                        if let Some(ref name) = folder.folder_name {
                            let count = get_data().favorites.get_paths(name).len();
                            ui.text(format!("{}", count))
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
            let btn_h = 0.06;
            let btn_r = Rect::new(1.0 - btn_w - 0.04, bottom - btn_h - 0.02, btn_w, btn_h);
            let ct = btn_r.center();
            self.create_btn.render_shadow(ui, btn_r, t, |ui, path| {
                ui.fill_path(&path, semi_black(0.5));
            });
            ui.text(tl!("favorites-create"))
                .pos(ct.x, ct.y)
                .anchor(0.5, 0.5)
                .no_baseline()
                .size(0.45)
                .color(semi_white(0.9))
                .draw();
        });

        // 编辑栏 || Edit bar
        if let Some(ref editing) = self.editing_folder {
            let is_default = editing == DEFAULT_FAVORITES_KEY;
            let bar_y = ui.top - edit_bar_height;
            let bar_r = Rect::new(-1., bar_y, 2., edit_bar_height);
            ui.fill_rect(bar_r, semi_black(0.7));
            ui.fill_rect(Rect::new(-1., bar_y, 2., 0.002), semi_white(0.3));

            let btn_w = 0.3;
            let btn_h = 0.055;
            let btn_y = bar_y + (edit_bar_height - btn_h) / 2.;
            let gap = 0.08;

            if is_default {
                // 默认收藏夹仅可修改封面 || Default folder: cover only
                let r = Rect::new(-btn_w / 2., btn_y, btn_w, btn_h);
                self.edit_cover_btn.render_shadow(ui, r, s.t, |ui, path| {
                    ui.fill_path(&path, semi_black(0.4));
                });
                ui.text(tl!("favorites-custom-cover"))
                    .pos(r.center().x, r.center().y)
                    .anchor(0.5, 0.5)
                    .no_baseline()
                    .size(0.4)
                    .color(semi_white(0.9))
                    .draw();
            } else {
                // 自定义收藏夹：封面、重命名、删除 || Custom folder: cover, rename, delete
                let total_w = btn_w * 3. + gap * 2.;
                let start_x = -total_w / 2.;

                let r = Rect::new(start_x, btn_y, btn_w, btn_h);
                self.edit_cover_btn.render_shadow(ui, r, s.t, |ui, path| {
                    ui.fill_path(&path, semi_black(0.4));
                });
                ui.text(tl!("favorites-custom-cover"))
                    .pos(r.center().x, r.center().y)
                    .anchor(0.5, 0.5)
                    .no_baseline()
                    .size(0.4)
                    .color(semi_white(0.9))
                    .draw();

                let r = Rect::new(start_x + btn_w + gap, btn_y, btn_w, btn_h);
                self.edit_rename_btn.render_shadow(ui, r, s.t, |ui, path| {
                    ui.fill_path(&path, semi_black(0.4));
                });
                ui.text(tl!("favorites-rename"))
                    .pos(r.center().x, r.center().y)
                    .anchor(0.5, 0.5)
                    .no_baseline()
                    .size(0.4)
                    .color(semi_white(0.9))
                    .draw();

                let r = Rect::new(start_x + (btn_w + gap) * 2., btn_y, btn_w, btn_h);
                self.edit_delete_btn.render_shadow(ui, r, s.t, |ui, path| {
                    ui.fill_path(&path, semi_black(0.4));
                });
                ui.text(tl!("favorites-delete"))
                    .pos(r.center().x, r.center().y)
                    .anchor(0.5, 0.5)
                    .no_baseline()
                    .size(0.4)
                    .color(Color::new(1., 0.3, 0.3, 0.9))
                    .draw();
            }
        }

        Ok(())
    }

    fn next_page(&mut self) -> NextPage {
        self.next_page.take().unwrap_or_default()
    }
}
