//! Replay browser. Lists all `data/replays/*.json` files grouped by chart
//! and lets the user replay them.

use super::{NextPage, Page, SharedState};
use crate::{dir, get_data, icons::Icons, scene::fs_from_path};
use anyhow::Result;
use chrono::{Local, TimeZone};
use inputbox::InputBox;
use macroquad::prelude::*;
use prpr::{
    ext::{poll_future, semi_black, semi_white, LocalTask, SafeTexture, ScaleType},
    fs,
    judge::icon_index,
    replay::ReplayData,
    scene::{request_input, return_input, show_error, take_input, BasicPlayer, GameMode, LoadingScene, NextScene},
    ui::{button_hit, DRectButton, Scroll, Ui},
};
use std::{borrow::Cow, collections::HashMap, sync::Arc};

const ITEM_HEIGHT: f32 = 0.18;

pub struct ReplayListPage {
    icons: Arc<Icons>,
    rank_icons: [SafeTexture; 8],

    /// Replays grouped by chart name (root view) or list of single-replay
    /// entries (folder view).
    folders: Vec<FolderEntry>,
    entries: Vec<ReplayEntry>,
    /// Folder we're currently inside (None = root). Stores `(group_key,
    /// display_name)` — the key is used to match replay files, the display
    /// name is what the page label / title bar shows.
    current_folder: Option<(String, String)>,

    folder_btns: Vec<DRectButton>,
    play_btns: Vec<DRectButton>,
    favorite_btns: Vec<DRectButton>,
    rename_btns: Vec<DRectButton>,
    delete_btns: Vec<DRectButton>,
    favorite_filter_btn: DRectButton,
    favorites_only: bool,
    renaming_file: Option<String>,

    scroll: Scroll,

    /// Async task building a `LoadingScene` for a selected replay.
    load_task: LocalTask<Result<NextScene>>,
    /// Scene to push to the main scene loop.
    pending_scene: Option<NextScene>,
}

struct FolderEntry {
    /// Group key: phira chart `local_path` when known, else `name:<display>`.
    /// This is what's persisted as `current_folder` while we're inside a
    /// chart's replay list.
    key: String,
    chart_name: String,
    chart_id: Option<i32>,
    count: usize,
}

struct ReplayEntry {
    file_name: String,
    replay_name: String,
    timestamp: i64,
    score: i32,
    accuracy: f32,
    full_combo: bool,
    speed: f32,
    favorite: bool,
}

impl ReplayListPage {
    pub fn new(icons: Arc<Icons>, rank_icons: [SafeTexture; 8]) -> Result<Self> {
        let mut this = Self {
            icons,
            rank_icons,
            folders: Vec::new(),
            entries: Vec::new(),
            current_folder: None,
            folder_btns: Vec::new(),
            play_btns: Vec::new(),
            favorite_btns: Vec::new(),
            rename_btns: Vec::new(),
            delete_btns: Vec::new(),
            favorite_filter_btn: DRectButton::new(),
            favorites_only: false,
            renaming_file: None,
            scroll: Scroll::new(),
            load_task: None,
            pending_scene: None,
        };
        this.reload();
        Ok(this)
    }

    fn reload(&mut self) {
        if let Some((key, _)) = self.current_folder.clone() {
            self.entries = read_chart_replays(&key, self.favorites_only);
            self.entries.sort_by_key(|b| std::cmp::Reverse(b.timestamp));
            self.play_btns = (0..self.entries.len()).map(|_| DRectButton::new()).collect();
            self.favorite_btns = (0..self.entries.len()).map(|_| DRectButton::new()).collect();
            self.rename_btns = (0..self.entries.len()).map(|_| DRectButton::new()).collect();
            self.delete_btns = (0..self.entries.len()).map(|_| DRectButton::new()).collect();
        } else {
            self.folders = read_all_folders(self.favorites_only);
            self.folders.sort_by(|a, b| a.chart_name.cmp(&b.chart_name));
            self.folder_btns = (0..self.folders.len()).map(|_| DRectButton::new()).collect();
        }
    }

    fn replay_path(file_name: &str) -> Result<std::path::PathBuf> {
        Ok(std::path::PathBuf::from(dir::replays()?).join(file_name))
    }

    fn update_replay(file_name: &str, update: impl FnOnce(&mut ReplayData)) -> Result<()> {
        let path = Self::replay_path(file_name)?;
        let content = std::fs::read_to_string(&path)?;
        let mut replay: ReplayData = serde_json::from_str(&content)?;
        update(&mut replay);
        std::fs::write(path, serde_json::to_string_pretty(&replay)?)?;
        Ok(())
    }

    fn toggle_favorite(&mut self, file_name: &str) {
        if let Err(e) = Self::update_replay(file_name, |replay| replay.favorite = !replay.favorite) {
            show_error(e);
        } else {
            self.reload();
        }
    }

    fn rename_replay(&mut self, file_name: &str, name: String) {
        if let Err(e) = Self::update_replay(file_name, |replay| replay.replay_name = name.trim().to_string()) {
            show_error(e);
        } else {
            self.reload();
        }
    }

    fn request_rename(&mut self, index: usize) {
        let entry = &self.entries[index];
        let text = if entry.replay_name.is_empty() {
            fmt_timestamp(entry.timestamp)
        } else {
            entry.replay_name.clone()
        };
        self.renaming_file = Some(entry.file_name.clone());
        request_input("replay_rename", InputBox::new().default_text(&text));
    }

    fn launch_replay_async(&mut self, file_name: String) -> Result<()> {
        let path = Self::replay_path(&file_name)?;
        let content = std::fs::read_to_string(&path)?;
        let replay: ReplayData = serde_json::from_str(&content)?;

        // Match the recorded chart back to a local entry, preferring the
        // strongest signal we have (online id > host's local_path > display
        // name). Two locally imported charts with the same name no longer
        // collide because `local_path` is unique per chart directory.
        let local_path = get_data()
            .charts
            .iter()
            .find(|c| {
                if let Some(id) = replay.chart_id {
                    if c.info.id == Some(id) {
                        return true;
                    }
                }
                if !replay.chart_local_path.is_empty() && c.local_path == replay.chart_local_path {
                    return true;
                }
                false
            })
            .or_else(|| get_data().charts.iter().find(|c| c.info.name == replay.chart_name))
            .map(|c| c.local_path.clone())
            .ok_or_else(|| anyhow::anyhow!("找不到对应的铺面: {}", replay.chart_name))?;

        prpr::replay::set_pending_playback(replay.clone());
        let replay_clone = replay;

        self.load_task = Some(Box::pin(async move {
            let mut fs_obj = fs_from_path(&local_path)?;
            let mut info = fs::load_info(fs_obj.as_mut()).await?;
            if info.id.is_none() {
                info.id = replay_clone.chart_id;
            }

            let mut config = get_data().config.clone();
            config.player_name = get_data().me.as_ref().map(|it| it.name.clone()).unwrap_or_else(|| "Guest".to_string());
            config.res_pack_path = {
                let id = get_data().respack_id;
                if id == 0 {
                    None
                } else {
                    Some(format!("{}/{}", dir::respacks()?, get_data().respacks[id - 1]))
                }
            };
            config.offline_mode = true;
            config.speed = replay_clone.speed.max(0.5);
            // Replay disables auto_record so we don't record-of-replay.
            config.auto_record = false;

            let preload = LoadingScene::load(fs_obj.as_mut(), &info.illustration).await?;
            let player = get_data().me.as_ref().map(|it| BasicPlayer {
                avatar: crate::client::UserManager::get_avatar(it.id).flatten(),
                id: it.id,
                rks: it.rks,
                historic_best: 0,
            });

            let scene = LoadingScene::new(GameMode::Normal, info, config, fs_obj, player, None, None, None, None, Some(preload)).await?;
            Ok(NextScene::Overlay(Box::new(scene)))
        }));
        Ok(())
    }
}

/// Build a stable group key for a `ReplayData`. We prefer the host's
/// `local_path` (unique per chart directory) so two locally imported
/// charts with the same display name don't end up in the same folder.
/// Old replays without `chart_local_path` fall back to a name-based key.
fn replay_group_key(r: &ReplayData) -> String {
    if !r.chart_local_path.is_empty() {
        format!("path:{}", r.chart_local_path)
    } else if let Some(id) = r.chart_id {
        format!("id:{id}")
    } else {
        format!("name:{}", r.chart_name)
    }
}

fn read_all_folders(favorites_only: bool) -> Vec<FolderEntry> {
    let dir = match dir::replays() {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    let path = std::path::Path::new(&dir);
    if !path.exists() {
        return Vec::new();
    }
    let mut groups: HashMap<String, FolderEntry> = HashMap::new();
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                if let Ok(replay) = serde_json::from_str::<ReplayData>(&content) {
                    if favorites_only && !replay.favorite {
                        continue;
                    }
                    let key = replay_group_key(&replay);
                    let g = groups.entry(key.clone()).or_insert_with(|| FolderEntry {
                        key,
                        chart_name: replay.chart_name.clone(),
                        chart_id: replay.chart_id,
                        count: 0,
                    });
                    g.count += 1;
                    if g.chart_id.is_none() {
                        g.chart_id = replay.chart_id;
                    }
                }
            }
        }
    }
    groups.into_values().collect()
}

fn read_chart_replays(folder_key: &str, favorites_only: bool) -> Vec<ReplayEntry> {
    let dir = match dir::replays() {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    let path = std::path::Path::new(&dir);
    if !path.exists() {
        return Vec::new();
    }
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                if let Ok(replay) = serde_json::from_str::<ReplayData>(&content) {
                    if replay_group_key(&replay) == folder_key {
                        if favorites_only && !replay.favorite {
                            continue;
                        }
                        out.push(ReplayEntry {
                            file_name: entry.file_name().to_string_lossy().to_string(),
                            replay_name: replay.replay_name,
                            timestamp: replay.timestamp,
                            score: replay.score,
                            accuracy: replay.accuracy,
                            full_combo: replay.full_combo,
                            speed: replay.speed,
                            favorite: replay.favorite,
                        });
                    }
                }
            }
        }
    }
    out
}

fn fmt_timestamp(ts: i64) -> String {
    if let Some(dt) = Local.timestamp_opt(ts, 0).single() {
        dt.format("%Y-%m-%d %H:%M:%S").to_string()
    } else {
        format!("ts={}", ts)
    }
}

impl Page for ReplayListPage {
    fn label(&self) -> Cow<'static, str> {
        if let Some((_, name)) = &self.current_folder {
            name.clone().into()
        } else {
            "回放列表".into()
        }
    }

    fn enter(&mut self, _s: &mut SharedState) -> Result<()> {
        self.reload();
        Ok(())
    }

    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        let t = s.t;

        if self.favorite_filter_btn.touch(touch, t) {
            button_hit();
            self.favorites_only = !self.favorites_only;
            self.reload();
            return Ok(true);
        }
        if self.scroll.touch(touch, t) {
            return Ok(true);
        }

        if self.current_folder.is_some() {
            let entries_len = self.entries.len();
            for i in 0..entries_len {
                if self.favorite_btns[i].touch(touch, t) {
                    button_hit();
                    let file_name = self.entries[i].file_name.clone();
                    self.toggle_favorite(&file_name);
                    return Ok(true);
                }
                if self.rename_btns[i].touch(touch, t) {
                    button_hit();
                    self.request_rename(i);
                    return Ok(true);
                }
                if self.delete_btns[i].touch(touch, t) {
                    button_hit();
                    let file_name = self.entries[i].file_name.clone();
                    if let Ok(path) = ReplayListPage::replay_path(&file_name) {
                        let _ = std::fs::remove_file(path);
                    }
                    self.reload();
                    return Ok(true);
                }
                if self.play_btns[i].touch(touch, t) {
                    button_hit();
                    let file_name = self.entries[i].file_name.clone();
                    if let Err(e) = self.launch_replay_async(file_name) {
                        show_error(e);
                    }
                    return Ok(true);
                }
            }
        } else {
            for i in 0..self.folders.len() {
                if self.folder_btns[i].touch(touch, t) {
                    button_hit();
                    let folder = &self.folders[i];
                    self.current_folder = Some((folder.key.clone(), folder.chart_name.clone()));
                    self.reload();
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }

    fn update(&mut self, s: &mut SharedState) -> Result<()> {
        if let Some((id, text)) = take_input() {
            if id == "replay_rename" {
                if let Some(file_name) = self.renaming_file.take() {
                    self.rename_replay(&file_name, text);
                } else {
                    return_input(id, text);
                }
            } else {
                return_input(id, text);
            }
        }

        self.scroll.update(s.t);

        if let Some(task) = &mut self.load_task {
            if let Some(res) = poll_future(task.as_mut()) {
                match res {
                    Ok(scene) => {
                        self.pending_scene = Some(scene);
                    }
                    Err(e) => {
                        show_error(e.context("加载回放失败"));
                    }
                }
                self.load_task = None;
            }
        }
        Ok(())
    }

    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        let t = s.t;

        s.render_fader(ui, |ui| {
            let top = ui.top;
            let chosen = self.favorites_only;
            let rr = Rect::new(0.76, -top + 0.04, 0.20, 0.07);
            self.favorite_filter_btn.render_shadow(ui, rr, t, |ui, path| {
                ui.fill_path(&path, if chosen { WHITE } else { semi_black(0.5) });
                ui.text("仅显示收藏")
                    .pos(rr.center().x, rr.center().y)
                    .anchor(0.5, 0.5)
                    .no_baseline()
                    .size(0.35)
                    .max_width(rr.w - 0.02)
                    .color(if chosen { Color::new(0.3, 0.3, 0.3, 1.) } else { WHITE })
                    .draw();
            });
        });

        let r = ui.content_rect();
        s.render_fader(ui, |ui| {
            ui.scope(|ui| {
                ui.dx(r.x);
                ui.dy(r.y);
                self.scroll.size((r.w, r.h));
                self.scroll.render(ui, |ui| {
                    let pad = 0.02;
                    // 2-column grid layout.
                    let cols = 2usize;
                    let cell_w = (r.w - pad * (cols as f32 + 1.)) / cols as f32;

                    if self.current_folder.is_some() {
                        let n = self.entries.len();
                        let rows = n.div_ceil(cols);
                        let total_h = pad + rows as f32 * (ITEM_HEIGHT + pad);
                        if n == 0 {
                            ui.text("此谱面暂无回放")
                                .pos(r.w / 2., 0.2)
                                .anchor(0.5, 0.)
                                .size(0.5)
                                .color(semi_white(0.7))
                                .draw();
                        }
                        for i in 0..n {
                            let col = i % cols;
                            let row = i / cols;
                            let x = pad + col as f32 * (cell_w + pad);
                            let y = pad + row as f32 * (ITEM_HEIGHT + pad);
                            let item_r = Rect::new(x, y, cell_w, ITEM_HEIGHT);
                            let entry = &self.entries[i];
                            self.play_btns[i].render_shadow(ui, item_r, t, |ui, path| {
                                ui.fill_path(&path, semi_black(0.55));
                            });

                            let pad_in = 0.018;
                            let mut tx = item_r.x + pad_in;
                            let icon = self.rank_icons[icon_index(entry.score as u32, entry.full_combo)].clone();
                            let icon_size = ITEM_HEIGHT - 0.04;
                            let ic_r = Rect::new(tx, item_r.y + 0.02, icon_size, icon_size);
                            ui.fill_rect(ic_r, (*icon, ic_r, ScaleType::Fit));
                            tx += icon_size + pad_in;

                            let title = if entry.replay_name.is_empty() {
                                format!("{:07}", entry.score)
                            } else {
                                entry.replay_name.clone()
                            };
                            ui.text(title)
                                .pos(tx, item_r.y + 0.018)
                                .size(0.55)
                                .max_width(cell_w - icon_size - 0.18)
                                .draw();
                            let acc_text = if entry.replay_name.is_empty() {
                                format!(
                                    "{:.2}%   {}{}",
                                    entry.accuracy * 100.,
                                    if entry.full_combo { "FC " } else { "" },
                                    fmt_timestamp(entry.timestamp)
                                )
                            } else {
                                format!(
                                    "{:07}  {:.2}%   {}{}",
                                    entry.score,
                                    entry.accuracy * 100.,
                                    if entry.full_combo { "FC " } else { "" },
                                    fmt_timestamp(entry.timestamp)
                                )
                            };
                            ui.text(acc_text)
                                .pos(tx, item_r.y + 0.07)
                                .size(0.3)
                                .max_width(cell_w - icon_size - 0.18)
                                .color(semi_white(0.8))
                                .draw();
                            if (entry.speed - 1.0).abs() > 1e-3 {
                                ui.text(format!("speed {:.2}x", entry.speed))
                                    .pos(tx, item_r.y + 0.105)
                                    .size(0.3)
                                    .color(semi_white(0.6))
                                    .draw();
                            }

                            let icon_size = 0.045;
                            let icon_x = item_r.right() - pad_in - icon_size;
                            let fav_r = Rect::new(icon_x, item_r.y + 0.018, icon_size, icon_size);
                            let fav_icon = if entry.favorite { &self.icons.star } else { &self.icons.star_outline };
                            ui.fill_rect(fav_r, (**fav_icon, fav_r, ScaleType::Fit, if entry.favorite { YELLOW } else { WHITE }));
                            self.favorite_btns[i].inner.set(ui, fav_r);

                            let edit_r = Rect::new(icon_x, item_r.y + 0.068, icon_size, icon_size);
                            ui.fill_rect(edit_r, (*self.icons.edit, edit_r, ScaleType::Fit));
                            self.rename_btns[i].inner.set(ui, edit_r);

                            let del_r = Rect::new(icon_x, item_r.y + 0.118, icon_size, icon_size);
                            ui.fill_rect(del_r, (*self.icons.delete, del_r, ScaleType::Fit));
                            self.delete_btns[i].inner.set(ui, del_r);
                        }
                        (r.w, total_h)
                    } else {
                        let n = self.folders.len();
                        let rows = n.div_ceil(cols);
                        let total_h = pad + rows as f32 * (ITEM_HEIGHT + pad);
                        if n == 0 {
                            ui.text("还没有任何回放")
                                .pos(r.w / 2., 0.15)
                                .anchor(0.5, 0.)
                                .size(0.6)
                                .color(semi_white(0.7))
                                .draw();
                            ui.text("打开 \"自动录制回放\" 后开始游玩，回放会出现在这里")
                                .pos(r.w / 2., 0.27)
                                .anchor(0.5, 0.)
                                .size(0.42)
                                .color(semi_white(0.5))
                                .draw();
                        }
                        for i in 0..n {
                            let col = i % cols;
                            let row = i / cols;
                            let x = pad + col as f32 * (cell_w + pad);
                            let y = pad + row as f32 * (ITEM_HEIGHT + pad);
                            let item_r = Rect::new(x, y, cell_w, ITEM_HEIGHT);
                            let folder = &self.folders[i];
                            self.folder_btns[i].render_shadow(ui, item_r, t, |ui, path| {
                                ui.fill_path(&path, semi_black(0.55));
                            });
                            ui.text(&folder.chart_name)
                                .pos(item_r.x + 0.025, item_r.y + 0.03)
                                .size(0.6)
                                .max_width(item_r.w - 0.08)
                                .draw();
                            ui.text(format!("{} 个回放", folder.count))
                                .pos(item_r.x + 0.025, item_r.y + 0.105)
                                .size(0.4)
                                .color(semi_white(0.7))
                                .draw();
                            ui.text(">")
                                .pos(item_r.right() - 0.025, item_r.center().y)
                                .anchor(1., 0.5)
                                .no_baseline()
                                .size(0.7)
                                .color(semi_white(0.5))
                                .draw();
                        }
                        (r.w, total_h)
                    }
                });
            });
        });
        Ok(())
    }

    fn next_page(&mut self) -> NextPage {
        NextPage::None
    }

    fn next_scene(&mut self, _s: &mut SharedState) -> NextScene {
        self.pending_scene.take().unwrap_or_default()
    }

    fn on_back_pressed(&mut self, _s: &mut SharedState) -> bool {
        // Inside a chart folder: back navigates up to the folder list
        // instead of popping the page.
        if self.current_folder.is_some() {
            self.current_folder = None;
            self.reload();
            return true;
        }
        false
    }
}
