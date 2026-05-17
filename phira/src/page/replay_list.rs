//! Replay browser. Lists all `data/replays/*.json` files grouped by chart,
//! lets the user replay them or export to MP4.

use super::{NextPage, Page, SharedState};
use crate::{dir, get_data, icons::Icons, scene::fs_from_path};
use anyhow::Result;
use chrono::{Local, TimeZone};
use macroquad::prelude::*;
use prpr::{
    ext::{poll_future, semi_black, semi_white, LocalTask, SafeTexture, ScaleType},
    fs,
    replay::ReplayData,
    scene::{show_error, show_message, BasicPlayer, GameMode, LoadingScene, NextScene},
    ui::{button_hit, DRectButton, Scroll, Ui},
};
use std::{borrow::Cow, collections::HashMap, sync::Arc};

const ITEM_HEIGHT: f32 = 0.18;

pub struct ReplayListPage {
    rank_icons: [SafeTexture; 8],

    /// Replays grouped by chart name (root view) or list of single-replay
    /// entries (folder view).
    folders: Vec<FolderEntry>,
    entries: Vec<ReplayEntry>,
    /// Folder we're currently inside (None = root).
    current_folder: Option<String>,

    folder_btns: Vec<DRectButton>,
    play_btns: Vec<DRectButton>,
    export_btns: Vec<DRectButton>,
    delete_btns: Vec<DRectButton>,
    refresh_btn: DRectButton,

    scroll: Scroll,

    /// Async task building a `LoadingScene` for a selected replay.
    load_task: LocalTask<Result<NextScene>>,
    /// Scene to push to the main scene loop.
    pending_scene: Option<NextScene>,
    /// `true` if the load_task is preparing an export rather than a replay.
    loading_for_export: bool,
}

struct FolderEntry {
    chart_name: String,
    chart_id: Option<i32>,
    count: usize,
}

struct ReplayEntry {
    file_name: String,
    timestamp: i64,
    score: i32,
    accuracy: f32,
    full_combo: bool,
    chart_name: String,
    speed: f32,
}

impl ReplayListPage {
    pub fn new(_icons: Arc<Icons>, rank_icons: [SafeTexture; 8]) -> Result<Self> {
        let mut this = Self {
            rank_icons,
            folders: Vec::new(),
            entries: Vec::new(),
            current_folder: None,
            folder_btns: Vec::new(),
            play_btns: Vec::new(),
            export_btns: Vec::new(),
            delete_btns: Vec::new(),
            refresh_btn: DRectButton::new(),
            scroll: Scroll::new(),
            load_task: None,
            pending_scene: None,
            loading_for_export: false,
        };
        this.reload();
        Ok(this)
    }

    fn reload(&mut self) {
        if let Some(folder) = self.current_folder.clone() {
            self.entries = read_chart_replays(&folder);
            self.entries.sort_by_key(|b| std::cmp::Reverse(b.timestamp));
            self.play_btns = (0..self.entries.len()).map(|_| DRectButton::new()).collect();
            self.export_btns = (0..self.entries.len()).map(|_| DRectButton::new()).collect();
            self.delete_btns = (0..self.entries.len()).map(|_| DRectButton::new()).collect();
        } else {
            self.folders = read_all_folders();
            self.folders.sort_by(|a, b| a.chart_name.cmp(&b.chart_name));
            self.folder_btns = (0..self.folders.len()).map(|_| DRectButton::new()).collect();
        }
    }

    fn replay_path(file_name: &str) -> Result<std::path::PathBuf> {
        Ok(std::path::PathBuf::from(dir::replays()?).join(file_name))
    }

    fn launch_replay_async(&mut self, file_name: String, export_output: Option<std::path::PathBuf>) -> Result<()> {
        let path = Self::replay_path(&file_name)?;
        let content = std::fs::read_to_string(&path)?;
        let replay: ReplayData = serde_json::from_str(&content)?;

        // Find local chart matching this replay.
        let local_path = get_data()
            .charts
            .iter()
            .find(|c| {
                if let Some(id) = replay.chart_id {
                    if c.info.id == Some(id) {
                        return true;
                    }
                }
                c.info.name == replay.chart_name
            })
            .map(|c| c.local_path.clone())
            .ok_or_else(|| anyhow::anyhow!("找不到对应的铺面: {}", replay.chart_name))?;

        // If exporting, build the Exporter on the main thread before the
        // async block so the GL render target is created on the main thread.
        if let Some(output) = export_output.as_ref() {
            let cfg = prpr::export::ExportConfig {
                width: 1920,
                height: 1080,
                fps: 60,
                output: output.clone(),
                audio_path: None,
            };
            match prpr::export::Exporter::new(cfg) {
                Ok(exp) => prpr::export::set_pending_exporter(exp),
                Err(e) => {
                    show_message(format!("启动 ffmpeg 失败: {}", e)).error();
                    return Ok(());
                }
            }
        }

        // Queue replay playback.
        prpr::replay::set_pending_playback(replay.clone());

        let exporting = export_output.is_some();
        self.loading_for_export = exporting;
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
            if exporting {
                config.aspect_ratio = Some(16.0 / 9.0);
            }

            let preload = LoadingScene::load(fs_obj.as_mut(), &info.illustration).await?;
            let player = get_data().me.as_ref().map(|it| BasicPlayer {
                avatar: crate::client::UserManager::get_avatar(it.id).flatten(),
                id: it.id,
                rks: it.rks,
                historic_best: 0,
            });

            let scene = LoadingScene::new(GameMode::Normal, info, config, fs_obj, player, None, None, None, Some(preload)).await?;
            Ok(NextScene::Overlay(Box::new(scene)))
        }));
        Ok(())
    }
}

fn rank_index(score: i32, full_combo: bool) -> usize {
    if score >= 1_000_000 {
        7 // phi
    } else if score >= 960_000 && full_combo {
        6 // V
    } else if score >= 960_000 {
        5 // S
    } else if score >= 920_000 {
        4 // A
    } else if score >= 880_000 {
        3 // B
    } else if score >= 820_000 {
        2 // C
    } else {
        1 // F
    }
}

fn read_all_folders() -> Vec<FolderEntry> {
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
                    let g = groups.entry(replay.chart_name.clone()).or_insert_with(|| FolderEntry {
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

fn read_chart_replays(chart_name: &str) -> Vec<ReplayEntry> {
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
                    if replay.chart_name == chart_name {
                        out.push(ReplayEntry {
                            file_name: entry.file_name().to_string_lossy().to_string(),
                            timestamp: replay.timestamp,
                            score: replay.score,
                            accuracy: replay.accuracy,
                            full_combo: replay.full_combo,
                            chart_name: replay.chart_name,
                            speed: replay.speed,
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
        if self.current_folder.is_some() {
            self.current_folder.clone().unwrap_or_default().into()
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

        // Refresh button (top right)
        if self.refresh_btn.touch(touch, t) {
            button_hit();
            self.reload();
            return Ok(true);
        }

        if self.scroll.touch(touch, t) {
            return Ok(true);
        }

        if self.current_folder.is_some() {
            // Folder view: handle play / export / delete buttons
            let entries_len = self.entries.len();
            for i in 0..entries_len {
                if self.export_btns[i].touch(touch, t) {
                    button_hit();
                    #[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))]
                    {
                        show_message("MP4 导出仅支持桌面端（需要本机的 ffmpeg）").error();
                        return Ok(true);
                    }
                    #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
                    {
                        let file_name = self.entries[i].file_name.clone();
                        let default = format!("{}_{}.mp4", self.entries[i].chart_name, self.entries[i].timestamp);
                        let chosen = pick_save_path(&default);
                        if let Some(out) = chosen {
                            if let Err(e) = self.launch_replay_async(file_name, Some(out)) {
                                show_error(e);
                            }
                        }
                        return Ok(true);
                    }
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
                    if let Err(e) = self.launch_replay_async(file_name, None) {
                        show_error(e);
                    }
                    return Ok(true);
                }
            }
        } else {
            // Root view: open folder
            for i in 0..self.folders.len() {
                if self.folder_btns[i].touch(touch, t) {
                    button_hit();
                    self.current_folder = Some(self.folders[i].chart_name.clone());
                    self.reload();
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }

    fn update(&mut self, s: &mut SharedState) -> Result<()> {
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
            // Header: back button + label + refresh
            let top = ui.top;

            // Refresh button (always visible top-right)
            let rr = Rect::new(0.78, -top + 0.04, 0.12, 0.07);
            self.refresh_btn.render_shadow(ui, rr, t, |ui, path| {
                ui.fill_path(&path, semi_black(0.5));
                ui.text("刷新")
                    .pos(rr.center().x, rr.center().y)
                    .anchor(0.5, 0.5)
                    .no_baseline()
                    .size(0.45)
                    .draw();
            });
        });

        // Body: scroll list
        let r = ui.content_rect();
        s.render_fader(ui, |ui| {
            ui.scope(|ui| {
                ui.dx(r.x);
                ui.dy(r.y);
                self.scroll.size((r.w, r.h));
                self.scroll.render(ui, |ui| {
                    let pad = 0.02;
                    let item_w = r.w - pad * 2.;

                    if self.current_folder.is_some() {
                        let n = self.entries.len();
                        let total_h = pad + n as f32 * (ITEM_HEIGHT + pad);
                        if n == 0 {
                            ui.text("此谱面暂无回放")
                                .pos(r.w / 2., 0.2)
                                .anchor(0.5, 0.)
                                .size(0.5)
                                .color(semi_white(0.7))
                                .draw();
                        }
                        for i in 0..n {
                            let entry = &self.entries[i];
                            let y = pad + i as f32 * (ITEM_HEIGHT + pad);
                            let item_r = Rect::new(pad, y, item_w, ITEM_HEIGHT);
                            self.play_btns[i].render_shadow(ui, item_r, t, |ui, path| {
                                ui.fill_path(&path, semi_black(0.55));
                            });

                            let pad_in = 0.025;
                            let mut tx = item_r.x + pad_in;
                            // Rank icon
                            let icon = self.rank_icons[rank_index(entry.score, entry.full_combo)].clone();
                            let icon_size = ITEM_HEIGHT - 0.04;
                            let ic_r = Rect::new(tx, item_r.y + 0.02, icon_size, icon_size);
                            ui.fill_rect(ic_r, (*icon, ic_r, ScaleType::Fit));
                            tx += icon_size + pad_in;

                            // Score + accuracy
                            ui.text(format!("{:07}", entry.score)).pos(tx, item_r.y + 0.018).size(0.65).draw();
                            let acc_text = format!(
                                "{:.2}%   {}{}",
                                entry.accuracy * 100.,
                                if entry.full_combo { "FC " } else { "" },
                                fmt_timestamp(entry.timestamp)
                            );
                            ui.text(acc_text).pos(tx, item_r.y + 0.075).size(0.42).color(semi_white(0.8)).draw();
                            if (entry.speed - 1.0).abs() > 1e-3 {
                                ui.text(format!("speed {:.2}x", entry.speed))
                                    .pos(tx, item_r.y + 0.115)
                                    .size(0.38)
                                    .color(semi_white(0.6))
                                    .draw();
                            }

                            // Right side: 导出 / 删除 buttons
                            let btn_w = 0.16;
                            let btn_h = 0.06;
                            let bx = item_r.right() - btn_w - pad_in;
                            let by = item_r.y + 0.02;
                            let exp_r = Rect::new(bx, by, btn_w, btn_h);
                            self.export_btns[i].render_text(ui, exp_r, t, "导出 MP4", 0.45, true);
                            let del_r = Rect::new(bx, by + btn_h + 0.005, btn_w, btn_h);
                            self.delete_btns[i].render_text(ui, del_r, t, "删除", 0.45, false);
                        }
                        (item_w, total_h)
                    } else {
                        let n = self.folders.len();
                        let total_h = pad + n as f32 * (ITEM_HEIGHT + pad);
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
                            let folder = &self.folders[i];
                            let y = pad + i as f32 * (ITEM_HEIGHT + pad);
                            let item_r = Rect::new(pad, y, item_w, ITEM_HEIGHT);
                            self.folder_btns[i].render_shadow(ui, item_r, t, |ui, path| {
                                ui.fill_path(&path, semi_black(0.55));
                            });
                            ui.text(&folder.chart_name)
                                .pos(item_r.x + 0.03, item_r.y + 0.03)
                                .size(0.7)
                                .max_width(item_r.w - 0.3)
                                .draw();
                            ui.text(format!("{} 个回放", folder.count))
                                .pos(item_r.x + 0.03, item_r.y + 0.115)
                                .size(0.45)
                                .color(semi_white(0.7))
                                .draw();
                            ui.text(">")
                                .pos(item_r.right() - 0.04, item_r.center().y)
                                .anchor(1., 0.5)
                                .no_baseline()
                                .size(0.8)
                                .color(semi_white(0.6))
                                .draw();
                        }
                        (item_w, total_h)
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

/// Open a "save as" dialog. On macOS we fall back to AppleScript if rfd
/// silently returns None (Sequoia sandboxing for non-bundled apps).
fn pick_save_path(default_name: &str) -> Option<std::path::PathBuf> {
    #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
    {
        #[allow(unused_mut)]
        let mut out = prpr::rfd::FileDialog::new()
            .add_filter("MP4 Video", &["mp4"])
            .set_file_name(default_name)
            .save_file();
        #[cfg(target_os = "macos")]
        if out.is_none() {
            let script = format!("POSIX path of (choose file name default name \"{}\")", default_name.replace('"', "\\\""));
            if let Ok(o) = std::process::Command::new("osascript").arg("-e").arg(script).output() {
                if o.status.success() {
                    let p = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    if !p.is_empty() {
                        out = Some(std::path::PathBuf::from(p));
                    }
                }
            }
        }
        out
    }
    #[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))]
    {
        let _ = default_name;
        None
    }
}
