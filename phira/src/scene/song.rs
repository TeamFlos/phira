prpr::tl_file!("song");

use super::{confirm_delete, confirm_dialog, fs_from_path};
use crate::{
    client::{recv_raw, Chart, Client, Permissions, Ptr, Record, UserManager},
    data::{BriefChartInfo, LocalChart},
    dir, get_data, get_data_mut,
    page::{thumbnail_path, ChartItem, Fader, Illustration, NEED_UPDATE},
    popup::Popup,
    rate::RateDialog,
    save_data,
    tags::TagsDialog,
};
use ::rand::{thread_rng, Rng};
use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use macroquad::prelude::*;
use prpr::{
    config::Mods,
    core::Tweenable,
    ext::{screen_aspect, semi_black, semi_white, unzip_into, JoinToString, RectExt, SafeTexture, ScaleType},
    fs,
    info::ChartInfo,
    judge::icon_index,
    scene::{
        load_scene, loading_scene, request_input, return_input, show_error, show_message, take_input, take_loaded_scene, BasicPlayer, GameMode,
        LoadingScene, NextScene, RecordUpdateState, Scene, SimpleRecord,
    },
    task::Task,
    time::TimeManager,
    ui::{button_hit, render_chart_info, ChartInfoEdit, DRectButton, Dialog, LoadingParams, RectButton, Scroll, Ui, UI_AUDIO},
};
use reqwest::Method;
use sasa::{AudioClip, Frame, Music, MusicParams};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    any::Any,
    borrow::Cow,
    fs::File,
    io::{Cursor, Write},
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, Weak,
    },
};
use walkdir::WalkDir;
use zip::{write::FileOptions, CompressionMethod, ZipWriter};

const FADE_IN_TIME: f32 = 0.3;
const EDIT_TRANSIT: f32 = 0.32;

static CONFIRM_UPLOAD: AtomicBool = AtomicBool::new(false);

fn create_music(clip: AudioClip) -> Result<Music> {
    let mut music = UI_AUDIO.with(|it| {
        it.borrow_mut().create_music(
            clip,
            MusicParams {
                amplifier: 0.7,
                loop_mix_time: 0.,
                ..Default::default()
            },
        )
    })?;
    music.play()?;
    Ok(music)
}

fn with_effects((mut frames, sample_rate): (Vec<Frame>, u32), range: Option<(f32, f32)>) -> Result<AudioClip> {
    if let Some((begin, end)) = range {
        frames.drain(((end * sample_rate as f32) as usize).min(frames.len())..);
        frames.drain(..((begin * sample_rate as f32) as usize));
    }
    let len = (0.8 * sample_rate as f64) as usize;
    let len = len.min(frames.len() / 2);
    for (i, frame) in frames[..len].iter_mut().enumerate() {
        let s = i as f32 / len as f32;
        frame.0 *= s;
        frame.1 *= s;
    }
    let st = frames.len() - len;
    for (i, frame) in frames[st..].iter_mut().rev().enumerate() {
        let s = i as f32 / len as f32;
        frame.0 *= s;
        frame.1 *= s;
    }
    Ok(AudioClip::from_raw(frames, sample_rate))
}

struct Downloading {
    status: Arc<Mutex<Cow<'static, str>>>,
    prog: Arc<Mutex<Option<f32>>>,
    task: Task<Result<LocalChart>>,
}

enum SideContent {
    Edit,
    Leaderboard,
    Info,
    Mods,
}

impl SideContent {
    fn width(&self) -> f32 {
        match self {
            Self::Edit => 0.84,
            Self::Leaderboard => 0.94,
            Self::Info => 0.75,
            Self::Mods => 0.8,
        }
    }
}

#[derive(Deserialize)]
struct StableR {
    status: i8,
}

#[derive(Deserialize)]
struct LeaderboardItem {
    #[serde(flatten)]
    pub inner: Record,
    pub rank: u32,
}

pub struct SongScene {
    illu: Illustration,

    first_in: bool,

    back_btn: RectButton,
    play_btn: DRectButton,

    icon_back: SafeTexture,
    icon_play: SafeTexture,
    icon_download: SafeTexture,
    icon_menu: SafeTexture,
    icon_edit: SafeTexture,
    icon_ldb: SafeTexture,
    icon_user: SafeTexture,
    icon_info: SafeTexture,
    icon_mod: SafeTexture,

    next_scene: Option<NextScene>,

    preview: Option<Music>,
    preview_task: Option<Task<Result<AudioClip>>>,

    load_task: Option<Task<Result<Option<Arc<Chart>>>>>,
    entity: Option<Chart>,
    info: BriefChartInfo,
    local_path: Option<String>,

    downloading: Option<Downloading>,
    cancel_download_btn: DRectButton,
    loading_last: f32,

    icons: [SafeTexture; 8],
    record: Option<SimpleRecord>,

    fetch_best_task: Option<Task<Result<SimpleRecord>>>,

    menu: Popup,
    menu_btn: RectButton,
    need_show_menu: bool,
    should_delete: Arc<AtomicBool>,
    menu_options: Vec<&'static str>,

    info_edit: Option<ChartInfoEdit>,
    edit_btn: RectButton,
    edit_scroll: Scroll,

    mods: Mods,
    mod_btn: RectButton,
    mod_scroll: Scroll,
    mod_btns: Vec<(DRectButton, bool)>,

    side_content: SideContent,
    side_enter_time: f32,

    save_task: Option<Task<Result<(ChartInfo, AudioClip)>>>,
    upload_task: Option<Task<Result<BriefChartInfo>>>,

    ldb: Option<(Option<u32>, Vec<LeaderboardItem>)>,
    ldb_task: Option<Task<Result<Vec<LeaderboardItem>>>>,
    ldb_btn: RectButton,
    ldb_scroll: Scroll,
    ldb_fader: Fader,
    ldb_type_btn: DRectButton,
    ldb_std: bool,

    info_btn: RectButton,
    info_scroll: Scroll,

    review_task: Option<Task<Result<String>>>,
    chart_should_delete: Arc<AtomicBool>,

    edit_tags_task: Option<Task<Result<()>>>,
    tags: TagsDialog,

    rate_dialog: RateDialog,
    rate_task: Option<Task<Result<()>>>,

    should_update: Arc<AtomicBool>,

    my_rating_task: Option<Task<Result<i16>>>,
    my_rate_score: Option<i16>,

    stabilize_task: Option<Task<Result<()>>>,
}

impl SongScene {
    pub fn new(
        mut chart: ChartItem,
        local_illu: Option<Illustration>,
        local_path: Option<String>,
        icon_back: SafeTexture,
        icon_play: SafeTexture,
        icon_download: SafeTexture,
        icon_menu: SafeTexture,
        icon_edit: SafeTexture,
        icon_ldb: SafeTexture,
        icon_user: SafeTexture,
        icon_info: SafeTexture,
        icons: [SafeTexture; 8],
        icon_mod: SafeTexture,
        icon_star: SafeTexture,
        mods: Mods,
    ) -> Self {
        if let Some(path) = &local_path {
            if let Some(id) = path.strip_prefix("download/") {
                chart.info.id = Some(id.parse().unwrap());
            }
        }
        let illu = if let Some(illu) = local_illu {
            illu
        } else if let Some(id) = chart.info.id {
            Illustration {
                texture: chart.illu.texture.clone(),
                task: Some(Task::new({
                    async move {
                        let chart = Ptr::<Chart>::new(id).load().await?;
                        let image = chart.illustration.load_image().await?;
                        Ok((image, None))
                    }
                })),
                loaded: Arc::default(),
                load_time: f32::NAN,
            }
        } else {
            chart.illu
        };
        let record = get_data()
            .charts
            .iter()
            .find(|it| Some(&it.local_path) == local_path.as_ref())
            .and_then(|it| it.record.clone());
        let fetch_best_task = if get_data().me.is_some() {
            chart.info.id.map(|id| Task::new(Client::best_record(id)))
        } else {
            None
        };
        let id = chart.info.id.clone();
        let offline_mode = get_data().config.offline_mode;
        Self {
            illu,

            first_in: true,

            back_btn: RectButton::new(),
            play_btn: DRectButton::new(),

            icon_back,
            icon_play,
            icon_download,
            icon_menu,
            icon_edit,
            icon_ldb,
            icon_user,
            icon_info,
            icon_mod,

            next_scene: None,

            preview: None,
            preview_task: Some(Task::new({
                let local_path = local_path.clone();
                async move {
                    if let Some(path) = local_path {
                        let mut fs = fs_from_path(&path)?;
                        let info = fs::load_info(fs.as_mut()).await?;
                        with_effects(
                            AudioClip::decode(fs.load_file(&info.music).await?)?,
                            Some((info.preview_start, info.preview_end.unwrap_or(info.preview_start + 15.))),
                        )
                    } else {
                        let chart = Ptr::<Chart>::new(id.unwrap()).fetch().await?;
                        with_effects(AudioClip::decode(chart.preview.fetch().await?.to_vec())?, None)
                    }
                }
            })),

            load_task: if offline_mode {
                None
            } else {
                id.map(|it| Task::new(async move { Ptr::new(it).fetch_opt().await }))
            },
            entity: None,
            info: chart.info,
            local_path,

            downloading: None,
            cancel_download_btn: DRectButton::new(),
            loading_last: 0.,

            icons,
            record,

            fetch_best_task,

            menu: Popup::new(),
            menu_btn: RectButton::new(),
            need_show_menu: false,
            should_delete: Arc::new(AtomicBool::default()),
            menu_options: Vec::new(),

            info_edit: None,
            edit_btn: RectButton::new(),
            edit_scroll: Scroll::new(),

            mods,
            mod_btn: RectButton::new(),
            mod_scroll: Scroll::new(),
            mod_btns: Vec::new(),

            side_content: SideContent::Edit,
            side_enter_time: f32::INFINITY,

            save_task: None,
            upload_task: None,

            ldb: None,
            ldb_task: None,
            ldb_btn: RectButton::new(),
            ldb_scroll: Scroll::new(),
            ldb_fader: Fader::new().with_distance(0.12),
            ldb_type_btn: DRectButton::new(),
            ldb_std: false,

            info_btn: RectButton::new(),
            info_scroll: Scroll::new(),

            review_task: None,
            chart_should_delete: Arc::default(),

            edit_tags_task: None,
            tags: TagsDialog::new(false),

            rate_dialog: RateDialog::new(icon_star, false),
            rate_task: None,

            should_update: Arc::default(),

            my_rating_task: if offline_mode {
                None
            } else {
                id.map(|id| {
                    Task::new(async move {
                        #[derive(Deserialize)]
                        struct Resp {
                            score: i16,
                        }
                        let resp: Resp = recv_raw(Client::get(format!("/chart/{id}/rate"))).await?.json().await?;
                        Ok(resp.score)
                    })
                })
            },
            my_rate_score: None,

            stabilize_task: None,
        }
    }

    fn start_download(&mut self) -> Result<()> {
        let chart = self.info.clone();
        let Some(entity) = self.entity.clone() else {
            show_error(anyhow!(tl!("no-chart-for-download")));
            return Ok(());
        };
        let progress = Arc::new(Mutex::new(None));
        let prog_wk = Arc::downgrade(&progress);
        let status = Arc::new(Mutex::new(tl!("dl-status-fetch")));
        let status_shared = Arc::clone(&status);
        self.loading_last = 0.;
        self.downloading = Some(Downloading {
            prog: progress,
            status: status_shared,
            task: Task::new({
                let path = format!("{}/{}", dir::downloaded_charts()?, uuid7::uuid7());
                async move {
                    let path = std::path::Path::new(&path);
                    tokio::fs::create_dir(path).await?;
                    let dir = prpr::dir::Dir::new(path)?;

                    let chart = chart;
                    async fn download(mut file: impl Write, url: &str, prog_wk: &Weak<Mutex<Option<f32>>>) -> Result<()> {
                        let Some(prog) = prog_wk.upgrade() else { return Ok(()) };
                        *prog.lock().unwrap() = None;
                        let res = reqwest::get(url).await.with_context(|| tl!("request-failed"))?;
                        let size = res.content_length();
                        let mut stream = res.bytes_stream();
                        let mut count = 0;
                        while let Some(chunk) = stream.next().await {
                            let chunk = chunk?;
                            file.write_all(&chunk)?;
                            count += chunk.len() as u64;
                            if let Some(size) = size {
                                *prog.lock().unwrap() = Some(count.min(size) as f32 / size as f32);
                            }
                            if prog_wk.strong_count() == 1 {
                                // cancelled
                                break;
                            }
                        }
                        Ok(())
                    }

                    *status.lock().unwrap() = tl!("dl-status-chart");
                    let mut bytes = Vec::new();
                    download(Cursor::new(&mut bytes), &entity.file.url, &prog_wk).await?;
                    *status.lock().unwrap() = tl!("dl-status-extract");
                    if prog_wk.strong_count() != 0 {
                        unzip_into(Cursor::new(bytes), &dir, false)?;
                    }
                    *status.lock().unwrap() = tl!("dl-status-saving");
                    if let Some(prog) = prog_wk.upgrade() {
                        *prog.lock().unwrap() = None;
                    }
                    let mut info: ChartInfo = serde_yaml::from_reader(dir.open("info.yml")?)?;
                    info.id = Some(entity.id);
                    info.created = Some(entity.created);
                    info.updated = Some(entity.updated);
                    info.chart_updated = Some(entity.chart_updated);
                    info.uploader = Some(entity.uploader.id);
                    serde_yaml::to_writer(dir.create("info.yml")?, &info)?;

                    if prog_wk.strong_count() == 0 {
                        // cancelled
                        drop(dir);
                        tokio::fs::remove_dir_all(&path).await?;
                    }

                    let local_path = format!("download/{}", chart.id.unwrap());
                    let to_path = format!("{}/{local_path}", dir::charts()?);
                    let to_path = Path::new(&to_path);
                    if to_path.exists() {
                        if to_path.is_file() {
                            tokio::fs::remove_file(to_path).await?;
                        } else {
                            tokio::fs::remove_dir_all(to_path).await?;
                        }
                    }
                    tokio::fs::rename(path, to_path).await?;

                    Ok(LocalChart {
                        info: entity.to_info(),
                        local_path,
                        record: None,
                        mods: Mods::default(),
                    })
                }
            }),
        });
        Ok(())
    }

    fn load_ldb(&mut self) {
        if get_data().config.offline_mode {
            return;
        }
        let Some(id) = self.info.id else { return };
        self.ldb = None;
        let std = self.ldb_std;
        self.ldb_task = Some(Task::new(async move {
            Ok(recv_raw(Client::get(format!("/record/list15/{id}")).query(&[("std", std)]))
                .await?
                .json()
                .await?)
        }));
    }

    fn update_record(&mut self, new_rec: SimpleRecord) -> Result<()> {
        let chart = get_data_mut()
            .charts
            .iter_mut()
            .find(|it| Some(&it.local_path) == self.local_path.as_ref());
        let Some(chart) = chart else {
            if let Some(rec) = &mut self.record {
                rec.update(&new_rec);
            } else {
                self.record = Some(new_rec);
            }
            return Ok(());
        };
        if let Some(rec) = &mut chart.record {
            if rec.update(&new_rec) {
                save_data()?;
            }
        } else {
            chart.record = Some(new_rec);
            save_data()?;
        }
        self.record = chart.record.clone();
        Ok(())
    }

    fn update_menu(&mut self) {
        self.menu_options.clear();
        if self.local_path.is_some() {
            self.menu_options.push("delete");
        }
        if self.info.id.is_some() {
            self.menu_options.push("rate");
        }
        if self.local_path.is_some() {
            self.menu_options.push("exercise");
            self.menu_options.push("offset");
        }
        let perms = get_data().me.as_ref().map(|it| it.perms()).unwrap_or_default();
        let is_uploader = get_data()
            .me
            .as_ref()
            .map_or(false, |it| Some(it.id) == self.info.uploader.as_ref().map(|it| it.id));
        if self.info.id.is_some() && perms.contains(Permissions::REVIEW) {
            if self.entity.as_ref().map_or(false, |it| !it.reviewed && !it.stable_request) {
                self.menu_options.push("review-approve");
                self.menu_options.push("review-deny");
            }
            self.menu_options.push("review-edit-tags");
        }
        if self.info.id.is_some() && is_uploader && self.entity.as_ref().map_or(false, |it| !it.stable && !it.stable_request) {
            self.menu_options.push("stabilize");
        }
        if self.info.id.is_some() && self.entity.as_ref().map_or(false, |it| it.stable_request) && perms.contains(Permissions::STABILIZE_CHART) {
            self.menu_options.push("stabilize-approve");
            self.menu_options.push("stabilize-deny");
        }
        if self.info.id.is_some()
            && (perms.contains(Permissions::DELETE_UNSTABLE) || is_uploader)
            && self.entity.as_ref().map_or(false, |it| !it.stable)
        {
            self.menu_options.push("review-del");
        }
        self.menu.set_options(self.menu_options.iter().map(|it| tl!(it).into_owned()).collect());
    }

    fn launch(&mut self, mode: GameMode) -> Result<()> {
        let mut fs = fs_from_path(self.local_path.as_ref().unwrap())?;
        #[cfg(feature = "closed")]
        let rated = {
            let config = &get_data().config;
            !config.offline_mode && self.info.id.is_some() && !self.mods.contains(Mods::AUTOPLAY) && config.speed >= 1.0 - 1e-3
        };
        #[cfg(not(feature = "closed"))]
        let rated = false;
        if !rated && self.info.id.is_some() && mode == GameMode::Normal {
            show_message(tl!("warn-unrated")).warn();
        }
        let id = self.info.id;
        let mods = self.mods;
        load_scene(async move {
            let mut info = fs::load_info(fs.as_mut()).await?;
            info.id = id;
            let mut config = get_data().config.clone();
            config.player_name = get_data()
                .me
                .as_ref()
                .map(|it| it.name.clone())
                .unwrap_or_else(|| tl!("guest").to_string());
            config.res_pack_path = {
                let id = get_data().respack_id;
                if id == 0 {
                    None
                } else {
                    Some(format!("{}/{}", dir::respacks()?, get_data().respacks[id - 1]))
                }
            };
            let chart_updated = info.chart_updated;
            config.mods = mods;
            LoadingScene::new(
                mode,
                info,
                config,
                fs,
                get_data().me.as_ref().map(|it| BasicPlayer {
                    avatar: UserManager::get_avatar(it.id).flatten(),
                    id: it.id,
                    rks: it.rks,
                }),
                None,
                Some(Arc::new(move |data| {
                    Task::new(async move {
                        #[derive(Serialize)]
                        #[serde(rename_all = "camelCase")]
                        struct Req {
                            chart: i32,
                            token: String,
                            chart_updated: Option<DateTime<Utc>>,
                        }
                        #[derive(Deserialize)]
                        #[serde(rename_all = "camelCase")]
                        struct Resp {
                            exp_delta: f64,
                            new_best: bool,
                            improvement: u32,
                            new_rks: f32,
                        }
                        let resp: Resp = recv_raw(Client::post(
                            "/play/upload",
                            &Req {
                                chart: id.unwrap(),
                                token: base64::encode(data),
                                chart_updated,
                            },
                        ))
                        .await?
                        .json()
                        .await?;
                        Ok(RecordUpdateState {
                            best: resp.new_best,
                            improvement: resp.improvement,
                            gain_exp: resp.exp_delta as f32,
                            new_rks: resp.new_rks,
                        })
                    })
                })),
            )
            .await
        });
        Ok(())
    }

    fn is_owner(&self) -> bool {
        self.info.id.is_none()
            || (self.info.created.is_some() && self.info.uploader.as_ref().map(|it| it.id) == get_data().me.as_ref().map(|it| it.id))
    }

    fn side_chart_info(&mut self, ui: &mut Ui, rt: f32) -> Result<()> {
        let h = 0.11;
        let pad = 0.03;
        let width = self.side_content.width() - pad;

        let is_owner = self.is_owner();
        let vpad = 0.02;
        let hpad = 0.01;
        let dx = width / if is_owner { 3. } else { 2. };
        let mut r = Rect::new(hpad, ui.top * 2. - h + vpad, dx - hpad * 2., h - vpad * 2.);
        if ui.button("cancel", r, tl!("edit-cancel")) {
            self.side_enter_time = -rt;
        }
        if is_owner {
            r.x += dx;
            if ui.button(
                "upload",
                r,
                if self.info.id.is_none() {
                    tl!("edit-upload")
                } else {
                    tl!("edit-update")
                },
            ) {
                let path = self.local_path.as_ref().unwrap();
                if get_data().me.is_none() {
                    show_message(tl!("upload-login-first"));
                } else if path.starts_with(':') {
                    show_message(tl!("upload-builtin"));
                } else {
                    Dialog::plain(tl!("upload-rules"), tl!("upload-rules-content"))
                        .buttons(vec![tl!("upload-cancel").to_string(), tl!("upload-confirm").to_string()])
                        .listener(|pos| {
                            if pos == 1 {
                                CONFIRM_UPLOAD.store(true, Ordering::SeqCst);
                            }
                        })
                        .show();
                }
            }
        }
        r.x += dx;
        if ui.button("save", r, tl!("edit-save")) {
            self.save_edit();
        }

        ui.ensure_touches()
            .retain(|it| !matches!(it.phase, TouchPhase::Started) || self.edit_scroll.contains(it));

        self.edit_scroll.size((width, ui.top * 2. - h));
        self.edit_scroll.render(ui, |ui| {
            let (w, mut h) = render_chart_info(ui, self.info_edit.as_mut().unwrap(), width);
            h += 0.06;
            ui.dy(h);
            if ui.button("edit_tags", Rect::new(0.04, 0., 0.2, 0.07), tl!("edit-tags")) {
                self.tags.tags.set(self.info_edit.as_ref().unwrap().info.tags.clone());
                self.tags.enter(rt);
            }
            (w, h + 0.1)
        });
        Ok(())
    }

    fn side_ldb(&mut self, ui: &mut Ui, rt: f32) {
        let pad = 0.03;
        let width = self.side_content.width() - pad;
        ui.dy(0.03);
        self.ldb_type_btn.render_text(
            ui,
            Rect::new(width - 0.24, -0.01, 0.23, 0.09),
            rt,
            1.,
            if self.ldb_std { tl!("ldb-std") } else { tl!("ldb-score") },
            0.6,
            true,
        );
        let r = ui.text(tl!("ldb")).size(0.8).draw();
        ui.dy(r.h + 0.03);
        let sh = ui.top * 2. - r.h - 0.08;
        let Some((_, items)) = &self.ldb else {
            ui.loading(width / 2., sh / 2., rt, WHITE, ());
            return;
        };
        self.ldb_scroll.size((width, sh));
        self.ldb_scroll.render(ui, |ui| {
            ui.text(ttl!("release-to-refresh"))
                .pos(width / 2., -0.13)
                .anchor(0.5, 0.)
                .size(0.8)
                .draw();
            let s = 0.14;
            let mut h = 0.;
            ui.dx(0.02);
            self.ldb_fader.reset();
            let me = get_data().me.as_ref().map(|it| it.id);
            self.ldb_fader.for_sub(|f| {
                for item in items {
                    f.render(ui, rt, |ui, c| {
                        if me == Some(item.inner.player.id) {
                            ui.fill_path(&Rect::new(-0.02, 0., width, s).feather(-0.01).rounded(0.02), Color { a: c.a, ..ui.background() });
                        }
                        let r = s / 2. - 0.02;
                        ui.text(format!("#{}", item.rank))
                            .pos((0.18 - r) / 2., s / 2.)
                            .anchor(0.5, 0.5)
                            .no_baseline()
                            .size(0.52)
                            .color(c)
                            .draw();
                        ui.avatar(0.18, s / 2., r, c, rt, UserManager::opt_avatar(item.inner.player.id, &self.icon_user));
                        let mut rt = width - 0.04;
                        let r = ui
                            .text(if self.ldb_std {
                                format!("{}ms", (item.inner.std.unwrap_or(0.) * 1000.) as i32)
                            } else {
                                format!("{:.2}%", item.inner.accuracy * 100.)
                            })
                            .pos(rt, s / 2.)
                            .anchor(1., 0.5)
                            .no_baseline()
                            .size(0.4)
                            .color(semi_white(c.a * 0.6))
                            .draw();
                        rt -= r.w + 0.01;
                        let r = ui
                            .text(if self.ldb_std {
                                format!("{:07}", item.inner.std_score.unwrap_or(0.) as i64)
                            } else {
                                format!("{:07}", item.inner.score)
                            })
                            .pos(rt, s / 2.)
                            .anchor(1., 0.5)
                            .no_baseline()
                            .size(0.6)
                            .color(c)
                            .draw();
                        rt -= r.w + 0.03;
                        let lt = 0.24;
                        if let Some(name) = UserManager::get_name(item.inner.player.id) {
                            ui.text(name)
                                .pos(lt, s / 2.)
                                .anchor(0., 0.5)
                                .no_baseline()
                                .max_width(rt - lt - 0.01)
                                .size(0.5)
                                .color(c)
                                .draw();
                        }
                    });
                    ui.dy(s);
                    h += s;
                }
            });
            (width, h)
        });
    }

    fn side_info(&mut self, ui: &mut Ui, rt: f32) {
        let pad = 0.03;
        ui.dx(pad);
        ui.dy(0.03);
        let width = self.side_content.width() - pad;
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
            if let Some(uploader) = &self.info.uploader {
                let r = ui.avatar(0.06, 0.06, 0.05, WHITE, rt, UserManager::opt_avatar(uploader.id, &self.icon_user));
                if let Some(name) = UserManager::get_name(uploader.id) {
                    ui.text(name)
                        .pos(r.right() + 0.02, r.center().y)
                        .anchor(0., 0.5)
                        .no_baseline()
                        .max_width(width - 0.15)
                        .size(0.6)
                        .draw();
                }
                dy!(0.14);
            }
            let mw = width - pad * 3.;
            let mut item = |title: Cow<'_, str>, content: Cow<'_, str>| {
                dy!(ui.text(title).size(0.4).color(semi_white(0.7)).draw().h + 0.02);
                dy!(ui.text(content).pos(pad, 0.).size(0.6).multiline().max_width(mw).draw().h + 0.03);
            };
            item(tl!("info-name"), self.info.name.as_str().into());
            item(tl!("info-composer"), self.info.composer.as_str().into());
            item(tl!("info-charter"), self.info.charter.as_str().into());
            item(tl!("info-difficulty"), format!("{} ({:.1})", self.info.level, self.info.difficulty).into());
            item(tl!("info-desc"), self.info.intro.as_str().into());
            if let Some(entity) = &self.entity {
                item(tl!("info-rating"), entity.rating.map_or(Cow::Borrowed("NaN"), |r| format!("{:.2} / 5.00", r * 5.).into()));
                item(
                    tl!("info-type"),
                    format!("{}{}", if entity.reviewed { "" } else { "[Unreviewed] " }, if entity.stable { "Stable" } else { "Unstable" }).into(),
                );
                item(tl!("info-tags"), entity.tags.iter().map(|it| format!("#{it}")).join(" ").into());
            }
            (width, h)
        });
    }

    fn side_mods(&mut self, ui: &mut Ui, rt: f32) {
        let pad = 0.03;
        ui.dx(pad);
        ui.dy(0.03);
        let width = self.side_content.width() - pad;
        self.mod_scroll.size((width - pad, ui.top * 2. - 0.06));
        self.mod_scroll.render(ui, |ui| {
            const ITEM_HEIGHT: f32 = 0.15;
            let mut h = 0.;
            macro_rules! dy {
                ($e:expr) => {{
                    let dy = $e;
                    h += dy;
                    ui.dy(dy);
                }};
            }
            dy!(ui.text(tl!("mods")).size(0.8).draw().h + 0.02);
            let rh = ITEM_HEIGHT * 3. / 5.;
            let rr = Rect::new(width - 0.24, (ITEM_HEIGHT - rh) / 2., 0.2, rh);
            let mut index = 0;
            let mut item = |title: Cow<'_, str>, subtitle: Option<Cow<'_, str>>, flag: Mods| {
                const TITLE_SIZE: f32 = 0.6;
                const SUBTITLE_SIZE: f32 = 0.35;
                const LEFT: f32 = 0.03;
                const PAD: f32 = 0.01;
                const SUB_MAX_WIDTH: f32 = 1.;
                if let Some(subtitle) = subtitle {
                    let r1 = ui.text(Cow::clone(&title)).size(TITLE_SIZE).measure();
                    let r2 = ui
                        .text(Cow::clone(&subtitle))
                        .size(SUBTITLE_SIZE)
                        .max_width(SUB_MAX_WIDTH)
                        .no_baseline()
                        .measure();
                    let h = r1.h + PAD + r2.h;
                    ui.text(subtitle)
                        .pos(LEFT, (ITEM_HEIGHT + h) / 2.)
                        .anchor(0., 1.)
                        .size(SUBTITLE_SIZE)
                        .max_width(SUB_MAX_WIDTH)
                        .color(semi_white(0.6))
                        .draw();
                    ui.text(title).pos(LEFT, (ITEM_HEIGHT - h) / 2.).no_baseline().size(TITLE_SIZE).draw();
                } else {
                    ui.text(title)
                        .pos(LEFT, ITEM_HEIGHT / 2.)
                        .anchor(0., 0.5)
                        .no_baseline()
                        .size(TITLE_SIZE)
                        .draw();
                }
                if self.mod_btns.len() <= index {
                    self.mod_btns.push(Default::default());
                }
                let (btn, clicked) = &mut self.mod_btns[index];
                if *clicked {
                    *clicked = false;
                    self.mods.toggle(flag);
                }
                let on = self.mods.contains(flag);
                let oh = rr.h;
                let (r, path) = btn.build(ui, rt, rr);
                let ct = r.center();
                ui.fill_path(&path, if on { WHITE } else { ui.background() });
                ui.text(if on { ttl!("switch-on") } else { ttl!("switch-off") })
                    .pos(ct.x, ct.y)
                    .anchor(0.5, 0.5)
                    .no_baseline()
                    .size(0.5 * (1. - (1. - r.h / oh).powf(1.3)))
                    .max_width(r.w)
                    .color(if on { Color::new(0.3, 0.3, 0.3, 1.) } else { WHITE })
                    .draw();
                dy!(ITEM_HEIGHT);
                index += 1;
            };
            item(tl!("mods-autoplay"), Some(tl!("mods-autoplay-sub")), Mods::AUTOPLAY);
            item(tl!("mods-flip-x"), Some(tl!("mods-flip-x-sub")), Mods::FLIP_X);
            item(tl!("mods-fade-out"), Some(tl!("mods-fade-out-sub")), Mods::FADE_OUT);
            (width, h)
        });
    }

    fn save_edit(&mut self) {
        let Some(edit) = &self.info_edit else { unreachable!() };
        let info = edit.info.clone();
        let path = self.local_path.clone().unwrap();
        let edit = edit.clone();
        let is_owner = self.is_owner();
        self.save_task = Some(Task::new(async move {
            let dir = prpr::dir::Dir::new(format!("{}/{path}", dir::charts()?))?;
            let patches = edit.to_patches().await.with_context(|| tl!("edit-load-file-failed"))?;
            if !is_owner && patches.contains_key(&info.chart) {
                bail!(tl!("edit-downloaded"));
            }
            let bytes = if let Some(bytes) = patches.get(&info.music) {
                bytes.clone()
            } else {
                dir.read(&info.music)?
            };
            let (frames, sample_rate) = AudioClip::decode(bytes)?;
            let length = frames.len() as f32 / sample_rate as f32;
            if info.preview_end.unwrap_or(info.preview_start + 1.) > length {
                tl!(bail "edit-preview-invalid");
            }
            let preview = with_effects((frames, sample_rate), Some((info.preview_start, info.preview_end.unwrap_or(info.preview_start + 15.))))?;
            for (name, bytes) in patches.into_iter() {
                dir.create(name)?.write_all(&bytes)?;
            }
            Ok((info, preview))
        }));
    }

    fn update_chart_info(&mut self) -> Result<()> {
        get_data_mut().charts[get_data().find_chart_by_path(self.local_path.as_deref().unwrap()).unwrap()].info = self.info.clone();
        NEED_UPDATE.store(true, Ordering::SeqCst);
        save_data()?;
        Ok(())
    }
}

impl Scene for SongScene {
    fn on_result(&mut self, tm: &mut TimeManager, res: Box<dyn Any>) -> Result<()> {
        let res = match res.downcast::<SimpleRecord>() {
            Err(res) => res,
            Ok(rec) => {
                if self.my_rate_score == Some(0) && thread_rng().gen_ratio(2, 5) {
                    self.rate_dialog.enter(tm.real_time() as _);
                }
                self.update_record(*rec)?;
                self.load_ldb();
                return Ok(());
            }
        };
        let res = match res.downcast::<anyhow::Error>() {
            Ok(error) => {
                show_error(error.context(tl!("load-chart-failed")));
                return Ok(());
            }
            Err(res) => res,
        };
        let _res = match res.downcast::<Option<f32>>() {
            Ok(offset) => {
                if let Some(offset) = *offset {
                    let dir = prpr::dir::Dir::new(format!("{}/{}", dir::charts()?, self.local_path.as_ref().unwrap()))?;
                    let mut info: ChartInfo = serde_yaml::from_reader(&dir.open("info.yml")?)?;
                    info.offset = offset;
                    dir.create("info.yml")?.write_all(serde_yaml::to_string(&info)?.as_bytes())?;
                    let path = thumbnail_path(self.local_path.as_ref().unwrap())?;
                    if path.exists() {
                        std::fs::remove_file(path)?;
                    }
                    show_message(tl!("edit-saved")).ok();
                }
                return Ok(());
            }
            Err(res) => res,
        };
        Ok(())
    }

    fn pause(&mut self, _tm: &mut TimeManager) -> Result<()> {
        if let Some(preview) = &mut self.preview {
            preview.pause()?;
        }
        Ok(())
    }

    fn resume(&mut self, _tm: &mut TimeManager) -> Result<()> {
        if let Some(preview) = &mut self.preview {
            preview.play()?;
        }
        Ok(())
    }

    fn enter(&mut self, tm: &mut TimeManager, _target: Option<RenderTarget>) -> Result<()> {
        if self.first_in {
            self.first_in = false;
            tm.seek_to(-FADE_IN_TIME as _);
            self.load_ldb();
        }
        if let Some(music) = &mut self.preview {
            music.seek_to(0.)?;
            music.play()?;
        }
        self.update_menu();
        Ok(())
    }

    fn touch(&mut self, tm: &mut TimeManager, touch: &Touch) -> Result<bool> {
        let t = tm.now() as f32;
        if loading_scene()
            || self.save_task.is_some()
            || self.upload_task.is_some()
            || self.review_task.is_some()
            || self.edit_tags_task.is_some()
            || self.rate_task.is_some()
        {
            return Ok(true);
        }
        let rt = tm.real_time() as f32;
        if self.tags.touch(touch, rt) {
            return Ok(true);
        }
        if self.rate_dialog.touch(touch, rt) {
            return Ok(true);
        }
        if self.menu.showing() {
            self.menu.touch(touch, t);
            return Ok(true);
        }
        if !self.side_enter_time.is_infinite() {
            if self.side_enter_time > 0. && tm.real_time() as f32 > self.side_enter_time + EDIT_TRANSIT {
                if touch.position.x < 1. - self.side_content.width() && touch.phase == TouchPhase::Started && self.save_task.is_none() {
                    if matches!(self.side_content, SideContent::Mods) {
                        let chart = &mut get_data_mut().charts[get_data().find_chart_by_path(self.local_path.as_deref().unwrap()).unwrap()];
                        if chart.mods != self.mods {
                            chart.mods = self.mods;
                            save_data()?;
                        }
                    }
                    self.side_enter_time = -tm.real_time() as _;
                    return Ok(true);
                }
                match self.side_content {
                    SideContent::Edit => {
                        if self.edit_scroll.touch(touch, t) {
                            return Ok(true);
                        }
                    }
                    SideContent::Leaderboard => {
                        if self.ldb_type_btn.touch(touch, t) {
                            self.ldb_std ^= true;
                            self.ldb_scroll.y_scroller.offset = 0.;
                            self.load_ldb();
                            return Ok(true);
                        }
                        if self.ldb_scroll.touch(touch, t) {
                            return Ok(true);
                        }
                    }
                    SideContent::Info => {
                        if self.info_scroll.touch(touch, t) {
                            return Ok(true);
                        }
                    }
                    SideContent::Mods => {
                        if self.mod_scroll.touch(touch, t) {
                            return Ok(true);
                        }
                        let rt = tm.real_time() as _;
                        for (btn, clicked) in &mut self.mod_btns {
                            if btn.touch(touch, rt) {
                                *clicked = true;
                                return Ok(true);
                            }
                        }
                    }
                }
            }
            return Ok(false);
        }
        if self.downloading.is_some() {
            if self.cancel_download_btn.touch(touch, t) {
                self.downloading = None;
                return Ok(true);
            }
            return Ok(false);
        }
        if self.back_btn.touch(touch) {
            button_hit();
            self.next_scene = Some(NextScene::PopWithResult(Box::new(false)));
            return Ok(true);
        }
        if self.play_btn.touch(touch, t) {
            if self.local_path.is_some() {
                self.launch(GameMode::Normal)?;
            } else {
                self.start_download()?;
            }
            return Ok(true);
        }
        if !self.menu_options.is_empty() && self.menu_btn.touch(touch) {
            button_hit();
            self.need_show_menu = true;
            return Ok(true);
        }
        if let Some(path) = &self.local_path {
            if self.edit_btn.touch(touch) {
                button_hit();
                let mut info: ChartInfo = serde_yaml::from_str(&std::fs::read_to_string(format!("{}/{path}/info.yml", dir::charts()?))?)?;
                info.id = self.info.id;
                self.info_edit = Some(ChartInfoEdit::new(info));
                self.side_content = SideContent::Edit;
                self.side_enter_time = tm.real_time() as _;
                return Ok(true);
            }
            if self.mod_btn.touch(touch) {
                button_hit();
                self.side_content = SideContent::Mods;
                self.side_enter_time = tm.real_time() as _;
                return Ok(true);
            }
        }
        if self.info.id.is_some() && self.ldb_btn.touch(touch) {
            button_hit();
            self.side_content = SideContent::Leaderboard;
            self.side_enter_time = tm.real_time() as _;
        }
        if self.info_btn.touch(touch) {
            button_hit();
            if let Some(uploader) = &self.info.uploader {
                UserManager::request(uploader.id);
            }
            self.side_content = SideContent::Info;
            self.side_enter_time = tm.real_time() as _;
            return Ok(true);
        }
        Ok(false)
    }

    fn update(&mut self, tm: &mut TimeManager) -> Result<()> {
        let t = tm.now() as f32;
        self.menu.update(t);
        self.illu.settle(t);
        let rt = tm.real_time() as f32;
        self.tags.update(rt);
        self.rate_dialog.update(rt);
        if self.tags.confirmed.take() == Some(true) {
            if !self.side_enter_time.is_infinite() && matches!(self.side_content, SideContent::Edit) {
                self.info_edit.as_mut().unwrap().info.tags = self.tags.tags.tags().to_vec();
            } else {
                let id = self.info.id.unwrap();
                let tags = self.tags.tags.tags().to_vec();
                self.entity.as_mut().unwrap().tags = tags.to_vec();
                self.edit_tags_task = Some(Task::new(async move {
                    recv_raw(Client::post(
                        format!("/chart/{id}/edit-tags"),
                        &json!({
                            "tags": tags,
                        }),
                    ))
                    .await?;
                    Ok(())
                }));
            }
        }
        if self.rate_dialog.confirmed.take() == Some(true) {
            let id = self.info.id.unwrap();
            let score = self.rate_dialog.rate.score;
            self.rate_task = Some(Task::new(async move {
                recv_raw(Client::post(
                    format!("/chart/{id}/rate"),
                    &json!({
                        "score": score,
                    }),
                ))
                .await?;
                Ok(())
            }));
        }
        if self.side_enter_time < 0. && -tm.real_time() as f32 + EDIT_TRANSIT < self.side_enter_time {
            self.side_enter_time = f32::INFINITY;
        }
        if let Some(task) = &mut self.load_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        show_error(err.context(tl!("load-charts-failed")));
                    }
                    Ok(chart) => {
                        if let Some(chart) = chart {
                            self.entity = Some(chart.as_ref().clone());
                            if self
                                .info
                                .updated
                                .map_or(chart.updated != chart.created, |local_updated| local_updated != chart.updated)
                            {
                                if self.local_path.is_some() {
                                    let chart_updated = self
                                        .info
                                        .chart_updated
                                        .map_or(chart.chart_updated != chart.created, |local_updated| local_updated != chart.chart_updated);
                                    confirm_dialog(
                                        tl!("need-update"),
                                        if chart_updated {
                                            tl!("need-update-content")
                                        } else {
                                            tl!("need-update-info-only-content")
                                        },
                                        Arc::clone(&self.should_update),
                                    );
                                }
                            }
                        } else if let Some(local) = &self.local_path {
                            let conf = format!("{}/{}/info.yml", dir::charts()?, local);
                            let mut info: ChartInfo = serde_yaml::from_reader(File::open(&conf)?)?;
                            info.id = None;
                            info.uploader = None;
                            info.created = None;
                            info.updated = None;
                            info.chart_updated = None;
                            serde_yaml::to_writer(File::create(conf)?, &info)?;
                            self.info = info.into();
                            self.update_chart_info()?;
                        }
                        self.update_menu();
                    }
                }
                self.load_task = None;
            }
        }
        if let Some(task) = &mut self.preview_task {
            if let Some(result) = task.take() {
                match result {
                    Err(err) => {
                        show_error(err.context(tl!("load-preview-failed")));
                    }
                    Ok(clip) => {
                        self.preview = Some(create_music(clip)?);
                    }
                }
                self.preview_task = None;
            }
        }
        if let Some(dl) = &mut self.downloading {
            if let Some(res) = dl.task.take() {
                match res {
                    Err(err) => {
                        let path = format!("{}/{}", dir::downloaded_charts()?, self.info.id.unwrap());
                        let path = Path::new(&path);
                        if path.exists() {
                            std::fs::remove_dir_all(path)?;
                        }
                        show_error(err.context(tl!("dl-failed")));
                    }
                    Ok(chart) => {
                        self.info = chart.info.clone();
                        if self.local_path.is_none() {
                            NEED_UPDATE.store(true, Ordering::SeqCst);
                            self.local_path = Some(chart.local_path.clone());
                            get_data_mut().charts.push(chart);
                        } else {
                            // update
                            self.update_chart_info()?;
                        }
                        save_data()?;
                        self.update_menu();
                        show_message(tl!("dl-success")).ok();
                    }
                }
                self.downloading = None;
            }
        }
        if let Some(res) = take_loaded_scene() {
            match res {
                Err(err) => {
                    let error = format!("{err:?}");
                    Dialog::plain(tl!("failed-to-play"), error)
                        .buttons(vec![tl!("play-cancel").to_string(), tl!("play-switch-to-offline").to_string()])
                        .listener(move |pos| {
                            if pos == 1 {
                                get_data_mut().config.offline_mode = true;
                                let _ = save_data();
                                show_message(tl!("switched-to-offline")).ok();
                            }
                        })
                        .show();
                }
                Ok(scene) => self.next_scene = Some(scene),
            }
        }
        if let Some(task) = &mut self.fetch_best_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        warn!("failed to fetch best record: {:?}", err);
                    }
                    Ok(rec) => {
                        self.update_record(rec)?;
                    }
                }
                self.fetch_best_task = None;
            }
        }
        if self.menu.changed() {
            match self.menu_options[self.menu.selected()] {
                "delete" => {
                    confirm_delete(self.should_delete.clone());
                }
                "rate" => {
                    self.rate_dialog.enter(tm.real_time() as _);
                }
                "exercise" => {
                    self.launch(GameMode::Exercise)?;
                }
                "offset" => {
                    self.launch(GameMode::TweakOffset)?;
                }
                "review-approve" => {
                    let id = self.info.id.unwrap();
                    self.review_task = Some(Task::new(async move {
                        #[derive(Deserialize)]
                        struct Resp {
                            passed: bool,
                        }
                        let resp: Resp = recv_raw(Client::post(
                            format!("/chart/{id}/review"),
                            &json!({
                                "approve": true
                            }),
                        ))
                        .await?
                        .json()
                        .await?;
                        Ok((if resp.passed { tl!("review-passed") } else { tl!("review-approved") }).into_owned())
                    }));
                }
                "review-deny" => {
                    request_input("deny-reason", "");
                }
                "review-del" => {
                    confirm_delete(self.chart_should_delete.clone());
                }
                "review-edit-tags" => {
                    let Some(entity) = self.entity.as_ref() else {
                        show_message(tl!("review-not-loaded")).warn();
                        return Ok(());
                    };
                    self.tags.tags.set(entity.tags.clone());
                    self.tags.enter(tm.real_time() as _);
                }
                "stabilize" => {
                    let id = self.info.id.unwrap();
                    self.stabilize_task = Some(Task::new(async move {
                        recv_raw(Client::post(format!("/chart/{id}/req-stabilize"), &())).await?;
                        Ok(())
                    }));
                }
                "stabilize-approve" => {
                    let id = self.info.id.unwrap();
                    self.review_task = Some(Task::new(async move {
                        let resp: StableR = recv_raw(Client::post(
                            format!("/chart/{id}/stabilize"),
                            &json!({
                                "approve": true
                            }),
                        ))
                        .await?
                        .json()
                        .await?;
                        Ok((if resp.status == 0 {
                            tl!("stabilize-approved")
                        } else {
                            tl!("stabilize-approved-passed")
                        })
                        .into())
                    }));
                }
                "stabilize-deny" => {
                    request_input("stabilize-deny-reason", "");
                }
                _ => {}
            }
        }
        if self.should_delete.fetch_and(false, Ordering::Relaxed) {
            self.next_scene = Some(NextScene::PopWithResult(Box::new(true)));
        }
        if self.chart_should_delete.fetch_and(false, Ordering::Relaxed) {
            let id = self.info.id.unwrap();
            self.review_task = Some(Task::new(async move {
                recv_raw(Client::delete(format!("/chart/{id}"))).await?;
                Ok(tl!("review-deleted").into_owned())
            }));
        }
        if let Some(task) = &mut self.save_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        show_error(err.context(tl!("edit-save-failed")));
                    }
                    Ok((info, preview)) => {
                        if let Some(preview) = &mut self.preview {
                            preview.pause()?;
                        }
                        self.preview = Some(create_music(preview)?);
                        self.info = info.into();
                        self.update_chart_info()?;
                        show_message(tl!("edit-saved")).duration(1.).ok();
                    }
                }
                self.save_task = None;
            }
        }
        if let Some(task) = &mut self.upload_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        show_error(err.context(tl!("upload-failed")));
                    }
                    Ok(info) => {
                        show_message(tl!("upload-success")).ok();
                        self.info = info;
                        self.update_chart_info()?;
                        self.side_enter_time = -tm.real_time() as _;
                    }
                }
                self.upload_task = None;
            }
        }
        match self.side_content {
            SideContent::Edit => {
                self.edit_scroll.update(t);
            }
            SideContent::Leaderboard => {
                if self.ldb_scroll.y_scroller.pulled {
                    self.ldb_scroll.y_scroller.offset = 0.;
                    self.load_ldb();
                }
                self.ldb_scroll.update(t);
            }
            SideContent::Info => {
                self.info_scroll.update(t);
            }
            SideContent::Mods => {
                self.mod_scroll.update(t);
            }
        }
        if CONFIRM_UPLOAD.fetch_and(false, Ordering::Relaxed) {
            let path = self.local_path.clone().unwrap();
            let info = self.info.clone();
            self.upload_task = Some(Task::new(async move {
                let root = format!("{}/{path}", dir::charts()?);
                let root = Path::new(&root);
                let chart_bytes = {
                    let mut bytes = Vec::new();
                    let mut zip = ZipWriter::new(Cursor::new(&mut bytes));
                    let options = FileOptions::default()
                        .compression_method(CompressionMethod::Deflated)
                        .unix_permissions(0o755);
                    #[allow(deprecated)]
                    for entry in WalkDir::new(root) {
                        let entry = entry?;
                        let path = entry.path();
                        let name = path.strip_prefix(root)?;
                        if path.is_file() {
                            zip.start_file_from_path(name, options)?;
                            let mut f = File::open(path)?;
                            std::io::copy(&mut f, &mut zip)?;
                        } else if !name.as_os_str().is_empty() {
                            zip.add_directory_from_path(name, options)?;
                        }
                    }
                    zip.finish()?;
                    drop(zip);
                    bytes
                };
                let file = Client::upload_file("chart.zip", chart_bytes)
                    .await
                    .with_context(|| tl!("upload-chart-failed"))?;
                if let Some(id) = info.id {
                    #[derive(Deserialize)]
                    #[serde(rename_all = "camelCase")]
                    struct Resp {
                        updated: DateTime<Utc>,
                        chart_updated: DateTime<Utc>,
                    }
                    let resp: Resp = recv_raw(Client::request(Method::PATCH, format!("/chart/{id}")).json(&json!({
                        "file": file,
                        "created": info.created.unwrap(),
                    })))
                    .await?
                    .json()
                    .await?;
                    let conf = root.join("info.yml");
                    let mut info: ChartInfo = serde_yaml::from_reader(File::open(&conf)?)?;
                    info.updated = Some(resp.updated);
                    info.chart_updated = Some(resp.chart_updated);
                    serde_yaml::to_writer(File::create(conf)?, &info)?;
                    Ok(info.into())
                } else {
                    #[derive(Deserialize)]
                    struct Resp {
                        id: i32,
                        created: DateTime<Utc>,
                    }
                    let resp: Resp = recv_raw(Client::post(
                        "/chart/upload",
                        &json!({
                            "file": file,
                        }),
                    ))
                    .await?
                    .json()
                    .await?;
                    let conf = root.join("info.yml");
                    let mut info: ChartInfo = serde_yaml::from_reader(File::open(&conf)?)?;
                    info.id = Some(resp.id);
                    info.created = Some(resp.created);
                    info.updated = Some(resp.created);
                    info.chart_updated = Some(resp.created);
                    info.uploader = Some(get_data().me.as_ref().unwrap().id);
                    serde_yaml::to_writer(File::create(conf)?, &info)?;
                    Ok(info.into())
                }
            }));
        }
        if let Some(task) = &mut self.ldb_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        show_error(err.context(tl!("ldb-load-failed")));
                    }
                    Ok(items) => {
                        let rank = get_data()
                            .me
                            .as_ref()
                            .and_then(|me| items.iter().find(|it| it.inner.player.id == me.id).map(|it| it.rank));
                        for item in &items {
                            UserManager::request(item.inner.player.id);
                        }
                        self.ldb = Some((rank, items));
                        self.ldb_fader.sub(tm.real_time() as _);
                    }
                }
                self.ldb_task = None;
            }
        }
        if let Some((id, text)) = take_input() {
            match id.as_str() {
                "deny-reason" => {
                    let id = self.info.id.unwrap();
                    self.review_task = Some(Task::new(async move {
                        recv_raw(Client::post(
                            format!("/chart/{id}/review"),
                            &json!({
                                "approve": false,
                                "reason": text,
                            }),
                        ))
                        .await?;
                        Ok(tl!("review-denied").into_owned())
                    }));
                }
                "stabilize-deny-reason" => {
                    let id = self.info.id.unwrap();
                    self.review_task = Some(Task::new(async move {
                        let resp: StableR = recv_raw(Client::post(
                            format!("/chart/{id}/stabilize"),
                            &json!({
                                "approve": false,
                                "reason": text,
                            }),
                        ))
                        .await?
                        .json()
                        .await?;
                        Ok((if resp.status == 0 {
                            tl!("stabilize-denied")
                        } else {
                            tl!("stabilize-denied-passed")
                        })
                        .into())
                    }));
                }
                _ => return_input(id, text),
            }
        }
        if let Some(task) = &mut self.review_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        show_error(err.context(tl!("review-action-failed")));
                    }
                    Ok(msg) => {
                        show_message(msg).ok();
                    }
                }
                self.review_task = None;
            }
        }
        if let Some(task) = &mut self.stabilize_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        show_error(err.context(tl!("stabilize-failed")));
                    }
                    Ok(_) => {
                        show_message(tl!("stabilize-requested")).ok();
                    }
                }
                self.review_task = None;
            }
        }
        if let Some(task) = &mut self.edit_tags_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        show_error(err.context(tl!("review-edit-tags-failed")));
                    }
                    Ok(_) => {
                        show_message(tl!("review-edit-tags-done")).ok();
                    }
                }
                self.edit_tags_task = None;
            }
        }
        if let Some(task) = &mut self.rate_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        show_error(err.context(tl!("rate-failed")));
                    }
                    Ok(_) => {
                        show_message(tl!("rate-done")).ok();
                    }
                }
                self.rate_dialog.dismiss(rt);
                self.rate_task = None;
            }
        }
        if self.should_update.fetch_and(false, Ordering::Relaxed) {
            self.start_download()?;
        }
        if let Some(task) = &mut self.my_rating_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        warn!("failed to fetch my rating status: {:?}", err);
                    }
                    Ok(score) => {
                        self.rate_dialog.rate.score = score;
                        self.my_rate_score = Some(score);
                    }
                }
                self.my_rating_task = None;
            }
        }
        Ok(())
    }

    fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        set_camera(&Camera2D {
            zoom: vec2(1., -screen_aspect()),
            ..Default::default()
        });
        let t = tm.now() as f32;
        ui.fill_rect(ui.screen_rect(), (*self.illu.texture.1, ui.screen_rect()));
        ui.fill_rect(ui.screen_rect(), semi_black(0.55));

        let c = semi_white((t / FADE_IN_TIME).clamp(-1., 0.) + 1.);

        let r = ui.back_rect();
        self.back_btn.set(ui, r);
        ui.fill_rect(r, (*self.icon_back, r, ScaleType::Fit, c));

        let r = ui
            .text(&self.info.name)
            .max_width(0.57 - r.right())
            .size(1.2)
            .pos(r.right() + 0.02, r.y)
            .color(c)
            .draw();
        ui.text(&self.info.composer)
            .size(0.5)
            .pos(r.x + 0.02, r.bottom() + 0.03)
            .color(Color { a: c.a * 0.8, ..c })
            .draw();

        // bottom bar
        let s = 0.25;
        let r = Rect::new(-0.94, ui.top - s - 0.06, s, s);
        let icon = self.record.as_ref().map_or(0, |it| icon_index(it.score as _, it.full_combo));
        ui.fill_rect(r, (*self.icons[icon], r, ScaleType::Fit, c));
        let score = self.record.as_ref().map(|it| it.score).unwrap_or_default();
        let accuracy = self.record.as_ref().map(|it| it.accuracy).unwrap_or_default();
        let r = ui
            .text(format!("{score:07}"))
            .pos(r.right() + 0.01, r.center().y)
            .anchor(0., 1.)
            .size(1.2)
            .color(c)
            .draw();
        ui.text(format!("{:.2}%", accuracy * 100.))
            .pos(r.x, r.bottom() + 0.01)
            .anchor(0., 0.)
            .size(0.7)
            .color(semi_white(0.7 * c.a))
            .draw();

        if self.info.id.is_some() {
            let h = 0.09;
            let mut r = Rect::new(r.x, r.y - h, h, h);
            ui.fill_rect(r, (*self.icon_ldb, r, ScaleType::Fit, c));
            if let Some((rank, _)) = &self.ldb {
                ui.text(if let Some(rank) = rank {
                    format!("#{rank}")
                } else {
                    tl!("ldb-no-rank").into_owned()
                })
                .pos(r.right() + 0.01, r.center().y)
                .anchor(0., 0.5)
                .no_baseline()
                .color(c)
                .size(0.7)
                .draw();
            } else {
                ui.loading(
                    r.right() + 0.04,
                    r.center().y,
                    t,
                    c,
                    LoadingParams {
                        radius: 0.027,
                        width: 0.007,
                        ..Default::default()
                    },
                );
            }
            r.w += 0.13;
            self.ldb_btn.set(ui, r);
        }

        // play button
        let w = 0.26;
        let pad = 0.08;
        let r = Rect::new(1. - pad - w, ui.top - pad - w, w, w);
        let (r, _) = self.play_btn.render_shadow(ui, r, t, c.a, |_| semi_white(0.3 * c.a));
        let r = r.feather(-0.04);
        ui.fill_rect(
            r,
            (
                if self.local_path.is_some() {
                    *self.icon_play
                } else {
                    *self.icon_download
                },
                r,
                ScaleType::Fit,
                c,
            ),
        );

        ui.scope(|ui| {
            ui.dx(1. - 0.03);
            ui.dy(-ui.top + 0.03);
            let s = 0.08;
            let r = Rect::new(-s, 0., s, s);
            let cc = semi_white(c.a * 0.4);
            ui.fill_rect(r, (*self.icon_menu, r.feather(-0.02), ScaleType::Fit, if self.menu_options.is_empty() { cc } else { c }));
            self.menu_btn.set(ui, r);
            if self.need_show_menu {
                self.need_show_menu = false;
                self.menu.set_bottom(true);
                self.menu.set_selected(usize::MAX);
                let d = 0.28;
                self.menu.show(ui, t, Rect::new(r.x - d, r.bottom() + 0.02, r.w + d, 0.4));
            }
            ui.dx(-r.w - 0.03);
            ui.fill_rect(r, (*self.icon_info, r, ScaleType::Fit, c));
            self.info_btn.set(ui, r);
            ui.dx(-r.w - 0.03);
            ui.fill_rect(r, (*self.icon_edit, r, ScaleType::Fit, if self.local_path.is_some() { c } else { cc }));
            self.edit_btn.set(ui, r);
            ui.dx(-r.w - 0.03);
            ui.fill_rect(r, (*self.icon_mod, r, ScaleType::Fit, if self.local_path.is_some() { c } else { cc }));
            self.mod_btn.set(ui, r);
        });

        if let Some(dl) = &self.downloading {
            ui.fill_rect(ui.screen_rect(), semi_black(0.6));
            ui.loading(0., -0.06, t, WHITE, (*dl.prog.lock().unwrap(), &mut self.loading_last));
            ui.text(dl.status.lock().unwrap().clone()).pos(0., 0.02).anchor(0.5, 0.).size(0.6).draw();
            let size = 0.7;
            let r = ui.text(tl!("dl-cancel")).pos(0., 0.12).anchor(0.5, 0.).size(size).measure().feather(0.02);
            self.cancel_download_btn.render_text(ui, r, t, 1., tl!("dl-cancel"), 0.6, true);
        }

        let rt = tm.real_time() as f32;
        if self.side_enter_time.is_finite() {
            let p = ((rt - self.side_enter_time.abs()) / EDIT_TRANSIT).min(1.);
            let p = 1. - (1. - p).powi(3);
            let p = if self.side_enter_time < 0. { 1. - p } else { p };
            ui.fill_rect(ui.screen_rect(), Color::new(0., 0., 0., p * 0.6));
            let w = self.side_content.width();
            let lf = f32::tween(&1.04, &(1. - w), p);
            ui.scope(|ui| {
                ui.dx(lf);
                ui.dy(-ui.top);
                let r = Rect::new(-0.2, 0., 0.2 + w, ui.top * 2.);
                ui.fill_rect(r, (Color::default(), (r.x, r.y), Color::new(0., 0., 0., p * 0.7), (r.right(), r.y)));

                match self.side_content {
                    SideContent::Edit => self.side_chart_info(ui, rt),
                    SideContent::Leaderboard => {
                        self.side_ldb(ui, rt);
                        Ok(())
                    }
                    SideContent::Info => {
                        self.side_info(ui, rt);
                        Ok(())
                    }
                    SideContent::Mods => {
                        self.side_mods(ui, rt);
                        Ok(())
                    }
                }
            })?;
        }

        self.menu.render(ui, t, 1.);

        if self.save_task.is_some() {
            ui.full_loading(tl!("edit-saving"), t);
        }
        if self.upload_task.is_some() {
            ui.full_loading(tl!("uploading"), t);
        }
        if self.review_task.is_some() {
            ui.full_loading(tl!("review-doing"), t);
        }
        if self.edit_tags_task.is_some() || self.rate_task.is_some() {
            ui.full_loading("", t);
        }
        let rt = tm.real_time() as f32;
        self.tags.render(ui, rt);
        self.rate_dialog.render(ui, rt);
        Ok(())
    }

    fn next_scene(&mut self, _tm: &mut TimeManager) -> NextScene {
        if let Some(scene) = self.next_scene.take() {
            if let Some(music) = &mut self.preview {
                let _ = music.pause();
            }
            scene
        } else {
            NextScene::default()
        }
    }
}
