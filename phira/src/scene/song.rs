prpr::tl_file!("song");

#[cfg(feature = "video")]
use super::UnlockScene;
use super::{
    confirm_delete, confirm_dialog, fs_from_path, gen_custom_dir, import_chart_to, render_ldb, LdbDisplayItem, ProfileScene, ASSET_CHART_INFO,
};
use crate::{
    charts_view::NEED_UPDATE,
    client::{basic_client_builder, recv_raw, Chart, Client, Permissions, Ptr, Record, UserManager, CLIENT_TOKEN},
    data::{BriefChartInfo, LocalChart},
    dir, get_data, get_data_mut,
    icons::Icons,
    page::{local_illustration, thumbnail_path, ChartItem, ChartType, Fader, Illustration, SFader},
    popup::Popup,
    rate::RateDialog,
    save_data,
    tags::TagsDialog,
};
use ::rand::{thread_rng, Rng};
use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use macroquad::prelude::*;
use phira_mp_common::{ClientCommand, CompactPos, JudgeEvent, TouchFrame};
use prpr::{
    config::Mods,
    core::{Tweenable, BOLD_FONT},
    ext::{
        open_url, poll_future, rect_shadow, semi_black, semi_white, unzip_into, JoinToString, LocalTask, RectExt, SafeTexture, ScaleType,
        BLACK_TEXTURE,
    },
    fs::{self},
    info::ChartInfo,
    judge::{icon_index, Judge},
    scene::{
        request_file, request_input, return_file, return_input, show_error, show_message, take_file, take_input, BasicPlayer, GameMode, LoadingScene,
        LocalSceneTask, NextScene, RecordUpdateState, Scene, SimpleRecord, UpdateFn, UploadFn,
    },
    task::Task,
    time::TimeManager,
    ui::{button_hit, render_chart_info, ChartInfoEdit, DRectButton, Dialog, LoadingParams, RectButton, Scroll, Ui, UI_AUDIO},
};
use reqwest::Method;
use sasa::{AudioClip, Frame, Music, MusicParams};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    any::Any,
    borrow::Cow,
    collections::{hash_map, HashMap, VecDeque},
    fs::File,
    io::{Cursor, Write},
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicI32, Ordering},
        Arc, Mutex, Weak,
    },
    thread_local,
};
use tokio::net::TcpStream;
use tracing::{error, warn};
use uuid::Uuid;
use walkdir::WalkDir;
use zip::{write::FileOptions, CompressionMethod, ZipWriter};

// Things that need to be reloaded for chart info updates
type LocalTuple = (String, ChartInfo, AudioClip, Illustration);

const FADE_IN_TIME: f32 = 0.3;
const EDIT_TRANSIT: f32 = 0.32;

static CONFIRM_CKSUM: AtomicBool = AtomicBool::new(false);
static UPLOAD_NOT_SAVED: AtomicBool = AtomicBool::new(false);
static CONFIRM_OVERWRITE: AtomicBool = AtomicBool::new(false);
static CONFIRM_UPLOAD: AtomicBool = AtomicBool::new(false);
pub static RECORD_ID: AtomicI32 = AtomicI32::new(-1);

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

async fn load_local_tuple(local_path: &str, def_illu: SafeTexture, info: ChartInfo) -> Result<LocalTuple> {
    let dir = prpr::dir::Dir::new(format!("{}/{local_path}", dir::charts()?))?;
    let bytes = dir.read(&info.music)?;
    let (frames, sample_rate) = AudioClip::decode(bytes)?;
    let length = frames.len() as f32 / sample_rate as f32;
    if info.preview_end.unwrap_or(info.preview_start + 1.) > length {
        tl!(bail "edit-preview-invalid");
    }
    let preview = with_effects((frames, sample_rate), Some((info.preview_start, info.preview_end.unwrap_or(info.preview_start + 15.))))?;
    let illu = local_illustration(local_path.to_owned(), def_illu, true);
    illu.notify.notify_one();

    Ok((local_path.to_owned(), info, preview, illu))
}

pub struct Downloading {
    info: BriefChartInfo,
    local_path: Option<String>,
    loading_last: f32,
    cancel_download_btn: DRectButton,
    status: Arc<Mutex<Cow<'static, str>>>,
    prog: Arc<Mutex<Option<f32>>>,
    task: Task<Result<(LocalChart, LocalTuple)>>,
}

impl Downloading {
    pub fn touch(&mut self, touch: &Touch, t: f32) -> bool {
        self.cancel_download_btn.touch(touch, t)
    }

    pub fn render(&mut self, ui: &mut Ui, t: f32) {
        ui.fill_rect(ui.screen_rect(), semi_black(0.6));
        ui.loading(0., -0.06, t, WHITE, (*self.prog.lock().unwrap(), &mut self.loading_last));
        ui.text(self.status.lock().unwrap().clone())
            .pos(0., 0.02)
            .anchor(0.5, 0.)
            .size(0.6)
            .draw();
        let size = 0.7;
        let r = ui.text(tl!("dl-cancel")).pos(0., 0.12).anchor(0.5, 0.).size(size).measure().feather(0.02);
        self.cancel_download_btn.render_text(ui, r, t, tl!("dl-cancel"), 0.6, true);
    }

    pub fn check(&mut self) -> Result<Option<Option<LocalTuple>>> {
        if let Some(res) = self.task.take() {
            match res {
                Err(err) => {
                    let path = format!("{}/{}", dir::downloaded_charts()?, self.info.id.unwrap());
                    let path = Path::new(&path);
                    if path.exists() {
                        std::fs::remove_dir_all(path)?;
                    }
                    show_error(err.context(tl!("dl-failed")));
                    Ok(Some(None))
                }
                Ok((chart, tuple)) => {
                    self.info = chart.info.clone();
                    if let Some(local_path) = &self.local_path {
                        // update
                        SongScene::global_update_chart_info(local_path, self.info.clone())?;
                    } else {
                        NEED_UPDATE.store(true, Ordering::Relaxed);
                        self.local_path = Some(chart.local_path.clone());
                        get_data_mut().charts.push(chart);
                    }
                    save_data()?;
                    show_message(tl!("dl-success")).ok();
                    Ok(Some(Some(tuple)))
                }
            }
        } else {
            Ok(None)
        }
    }
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
struct LdbItem {
    #[serde(flatten)]
    pub inner: Record,
    pub rank: u32,
    #[serde(skip, default)]
    pub btn: RectButton,
}

pub struct SongScene {
    illu: Illustration,

    first_in: bool,

    back_btn: RectButton,
    play_btn: DRectButton,

    icons: Arc<Icons>,

    next_scene: Option<NextScene>,

    preview: Option<Music>,
    preview_task: Option<Task<Result<AudioClip>>>,

    load_task: Option<Task<Result<Option<Arc<Chart>>>>>,
    entity: Option<Chart>,
    info: BriefChartInfo,
    local_path: Option<String>,

    downloading: Option<Downloading>,
    loading_last: f32,

    rank_icons: [SafeTexture; 8],
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

    save_task: Option<Task<Result<LocalTuple>>>,
    upload_task: Option<Task<Result<BriefChartInfo>>>,

    ldb: Option<(Option<u32>, Vec<LdbItem>)>,
    ldb_task: Option<Task<Result<Vec<LdbItem>>>>,
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
    should_stabilize: Arc<AtomicBool>,

    scene_task: LocalTask<Result<NextScene>>,

    uploader_btn: RectButton,

    sf: SFader,
    fade_start: f32,

    background: Arc<Mutex<Option<SafeTexture>>>,
    tr_start: f32,

    open_web_btn: DRectButton,

    // Imported chart for overwriting
    overwrite_from: Option<String>,
    overwrite_task: Option<Task<Result<LocalTuple>>>,

    update_cksum_passed: Option<bool>,
    update_cksum_task: Option<Task<Result<bool>>>,
    chart_type: ChartType,
}

impl SongScene {
    pub fn new(mut chart: ChartItem, local_path: Option<String>, icons: Arc<Icons>, rank_icons: [SafeTexture; 8], mods: Mods) -> Self {
        if let Some(path) = &local_path {
            if let Some(id) = path.strip_prefix("download/") {
                chart.info.id = Some(id.parse().unwrap());
            }
        }
        let illu = if let Some(path) = &chart.local_path {
            let illu = local_illustration(path.clone(), chart.illu.texture.1.clone(), true);
            illu.notify.notify_one();
            illu
        } else if let Some(id) = chart.info.id {
            Illustration {
                texture: chart.illu.texture.clone(),
                notify: Arc::default(),
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
            .and_then(|it| it.record.clone())
            .or_else(|| local_path.as_ref().and_then(|path| get_data().local_records.get(path).cloned().flatten()));
        let fetch_best_task = if get_data().me.is_some() {
            chart.info.id.map(|id| Task::new(Client::best_record(id)))
        } else {
            None
        };
        let id = chart.info.id;
        let offline_mode = get_data().config.offline_mode;
        let icon_star = icons.star.clone();
        Self {
            illu,

            first_in: true,

            back_btn: RectButton::new(),
            play_btn: DRectButton::new(),

            icons,

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
            loading_last: 0.,

            rank_icons,
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
            should_stabilize: Arc::default(),

            scene_task: None,

            uploader_btn: RectButton::new(),

            sf: SFader::new(),
            fade_start: 0.,

            tr_start: f32::NAN,
            background: Arc::default(),

            open_web_btn: DRectButton::new(),

            overwrite_from: None,
            overwrite_task: None,

            update_cksum_passed: None,
            update_cksum_task: None,
            chart_type: chart.chart_type,
        }
    }

    fn start_download(&mut self) -> Result<()> {
        let chart = self.info.clone();
        let Some(entity) = self.entity.clone() else {
            show_error(anyhow!(tl!("no-chart-for-download")));
            return Ok(());
        };
        self.loading_last = 0.;
        self.downloading = Some(Self::global_start_download(chart, entity, self.local_path.clone())?);
        Ok(())
    }

    pub fn global_start_download(chart: BriefChartInfo, entity: Chart, local_path: Option<String>) -> Result<Downloading> {
        let progress = Arc::new(Mutex::new(None));
        let prog_wk = Arc::downgrade(&progress);
        let status = Arc::new(Mutex::new(tl!("dl-status-fetch")));
        let status_shared = Arc::clone(&status);
        Ok(Downloading {
            info: chart.clone(),
            local_path,
            loading_last: 0.,
            cancel_download_btn: DRectButton::new(),
            prog: progress,
            status: status_shared,
            task: Task::new({
                let path = format!("{}/{}", dir::downloaded_charts()?, Uuid::new_v4());
                async move {
                    let path = std::path::Path::new(&path);
                    tokio::fs::create_dir(path).await?;
                    let dir = prpr::dir::Dir::new(path)?;

                    let chart = chart;
                    async fn download(mut file: impl Write, url: &str, prog_wk: &Weak<Mutex<Option<f32>>>) -> Result<()> {
                        let Some(prog) = prog_wk.upgrade() else { return Ok(()) };
                        *prog.lock().unwrap() = None;
                        let req = basic_client_builder().build().unwrap().get(url);
                        let req = if let Some(token) = CLIENT_TOKEN.load().as_ref() {
                            req.header("Authorization", format!("Bearer {token}"))
                        } else {
                            req
                        };
                        let res = req.send().await.with_context(|| tl!("request-failed"))?.error_for_status()?;
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

                    let tuple = load_local_tuple(&local_path, BLACK_TEXTURE.clone(), info).await?;

                    Ok((
                        LocalChart {
                            info: entity.to_info(),
                            local_path,
                            record: None,
                            mods: Mods::default(),
                            played_unlock: false,
                        },
                        tuple,
                    ))
                }
            }),
        })
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
        let rec = get_data_mut()
            .charts
            .iter_mut()
            .find(|it| Some(&it.local_path) == self.local_path.as_ref())
            .map(|it| &mut it.record)
            .or_else(|| {
                self.local_path
                    .clone()
                    .map(|path| get_data_mut().local_records.entry(path).or_insert(None))
            });
        let Some(rec) = rec else {
            if let Some(rec) = &mut self.record {
                rec.update(&new_rec);
            } else {
                self.record = Some(new_rec);
            }
            return Ok(());
        };
        if let Some(rec) = rec {
            if rec.update(&new_rec) {
                save_data()?;
            }
        } else {
            *rec = Some(new_rec);
            save_data()?;
        }
        self.record = rec.clone();
        Ok(())
    }

    fn update_menu(&mut self) {
        self.menu_options.clear();
        if self.local_path.as_ref().map_or(false, |it| !it.starts_with(':')) {
            self.menu_options.push("delete");
        }
        if self.info.id.is_some() {
            self.menu_options.push("rate");
        }
        if let Some(local_path) = &self.local_path {
            self.menu_options.push("exercise");
            self.menu_options.push("offset");
            if get_data()
                .charts
                .iter()
                .find(|it| it.local_path == *local_path)
                .map_or(false, |it| it.played_unlock)
            {
                self.menu_options.push("unlock");
            }
        }
        let perms = get_data().me.as_ref().map(|it| it.perms()).unwrap_or_default();
        let is_uploader = get_data()
            .me
            .as_ref()
            .map_or(false, |it| Some(it.id) == self.info.uploader.as_ref().map(|it| it.id));
        if self.info.id.is_some() && (perms.contains(Permissions::REVIEW) || perms.contains(Permissions::REVIEW_PECJAM)) {
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
            self.menu_options.push("stabilize-approve-ranked");
            self.menu_options.push("stabilize-comment");
            self.menu_options.push("stabilize-deny");
        }
        if self.info.id.is_some()
            && self.entity.as_ref().map_or(false, |it| {
                if it.stable {
                    perms.contains(Permissions::DELETE_STABLE)
                } else {
                    is_uploader || perms.contains(Permissions::DELETE_UNSTABLE)
                }
            })
        {
            self.menu_options.push("review-del");
        }
        self.menu.set_options(self.menu_options.iter().map(|it| tl!(*it).into_owned()).collect());
    }

    fn launch(&mut self, mode: GameMode, force_unlock: bool) -> Result<()> {
        let local_path = self.local_path.as_ref().unwrap();
        let is_unlock = force_unlock
            || (mode == GameMode::Normal
                && get_data()
                    .charts
                    .iter()
                    .find(|it| it.local_path == *local_path)
                    .map_or(false, |it| it.info.has_unlock && !it.played_unlock));

        self.scene_task =
            Self::global_launch(self.info.id, local_path, self.mods, mode, None, Some(self.background.clone()), self.record.clone(), is_unlock)?;

        Ok(())
    }

    #[must_use]
    pub fn global_launch(
        id: Option<i32>,
        local_path: &str,
        mods: Mods,
        mode: GameMode,
        client: Option<Arc<phira_mp_client::Client>>,
        background_output: Option<Arc<Mutex<Option<SafeTexture>>>>,
        record: Option<SimpleRecord>,
        is_unlock: bool,
    ) -> Result<LocalSceneTask> {
        let mut fs = fs_from_path(local_path)?;
        let can_rated = id.is_some() || local_path.starts_with(':');
        #[cfg(feature = "closed")]
        let rated = {
            let config = &get_data().config;
            !config.offline_mode && can_rated && !mods.contains(Mods::AUTOPLAY) && config.speed >= 1.0 - 1e-3
        };
        #[cfg(not(feature = "closed"))]
        let rated = false;
        if !rated && can_rated && mode == GameMode::Normal {
            show_message(tl!("warn-unrated")).warn();
        }
        let update_fn = client.and_then(|mut client| {
            let live = client.blocking_state().unwrap().live;
            let token = get_data().tokens.as_ref().map(|it| it.0.clone()).unwrap();
            let addr = get_data().config.mp_address.clone();
            let mut reconnect_task: Option<Task<Result<phira_mp_client::Client>>> = None;
            let update_fn: Option<UpdateFn> = if live {
                Some(Box::new({
                    let mut touch_ids: HashMap<u64, i8> = HashMap::new();
                    let mut touch_last_update: HashMap<i8, f32> = HashMap::new();
                    let mut touches: VecDeque<TouchFrame> = VecDeque::new();
                    let mut judges: VecDeque<JudgeEvent> = VecDeque::new();
                    let mut last_send_touch_time: f32 = 0.;
                    move |t, res, judge| {
                        if client.ping_fail_count() >= 1 && reconnect_task.is_none() {
                            warn!("lost connection, auto re-connect");
                            let token = token.clone();
                            let addr = addr.clone();
                            reconnect_task = Some(Task::new(async move {
                                let client = phira_mp_client::Client::new(TcpStream::connect(addr).await?).await?;
                                client.authenticate(token).await?;
                                Ok(client)
                            }));
                        }
                        if let Some(task) = &mut reconnect_task {
                            if let Some(res) = task.take() {
                                match res {
                                    Err(err) => {
                                        warn!(?err, "failed to reconnect");
                                    }
                                    Ok(new) => {
                                        warn!("reconnected!");
                                        client = new.into();
                                    }
                                }
                                reconnect_task = None;
                            }
                        }
                        let points: Vec<_> = Judge::get_touches()
                            .into_iter()
                            .filter_map(|it| {
                                if matches!(it.phase, TouchPhase::Stationary) {
                                    return None;
                                }
                                let len = touch_ids.len();
                                let mut id = match touch_ids.entry(it.id) {
                                    hash_map::Entry::Occupied(val) => *val.get(),
                                    hash_map::Entry::Vacant(place) => *place.insert(len.try_into().ok()?),
                                };
                                if matches!(it.phase, TouchPhase::Moved) && touch_last_update.get(&id).map_or(false, |it| *it + 1. / 20. >= t) {
                                    return None;
                                }
                                touch_last_update.insert(id, t);
                                if matches!(it.phase, TouchPhase::Ended | TouchPhase::Cancelled) {
                                    touch_ids.remove(&it.id);
                                    id = !id;
                                }
                                Some((id, CompactPos::new(it.position.x, it.position.y * res.aspect_ratio)))
                            })
                            .collect();
                        if !points.is_empty() {
                            touches.push_back(TouchFrame { time: t, points });
                        }
                        if last_send_touch_time + 1. < t || touches.len() > 20 {
                            if touches.is_empty() {
                                touches.push_back(TouchFrame { time: t, points: Vec::new() });
                            }
                            let frames = Arc::new(touches.drain(..).collect());
                            client.blocking_send(ClientCommand::Touches { frames }).unwrap();
                            last_send_touch_time = t;
                        }
                        judges.extend(judge.judgements.borrow_mut().drain(..).map(|it| JudgeEvent {
                            time: it.0,
                            line_id: it.1,
                            note_id: it.2,
                            judgement: {
                                use phira_mp_common::Judgement::*;
                                use prpr::judge::Judgement as OJ;
                                match it.3 {
                                    Ok(OJ::Perfect) => Perfect,
                                    Ok(OJ::Good) => Good,
                                    Ok(OJ::Bad) => Bad,
                                    Ok(OJ::Miss) => Miss,
                                    Err(true) => HoldPerfect,
                                    Err(false) => HoldGood,
                                }
                            },
                        }));
                        if judges.len() > 10 || judges.front().map_or(false, |it| it.time + 0.6 < t) {
                            let judges = Arc::new(judges.drain(..).collect());
                            client.blocking_send(ClientCommand::Judges { judges }).unwrap();
                        }
                    }
                }))
            } else {
                None
            };
            update_fn
        });

        let local_path = local_path.to_owned();
        Ok(Some(Box::pin(async move {
            let mut info = fs::load_info(fs.as_mut()).await?;
            info.id = id;
            let mut config = get_data().config.clone();
            config.player_name = get_data()
                .me
                .as_ref()
                .map(|it| it.name.clone())
                .unwrap_or_else(|| tl!("guest").into_owned());
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
            let preload = LoadingScene::load(fs.as_mut(), &info.illustration).await?;
            if let Some(output) = background_output {
                *output.lock().unwrap() = Some(preload.1.clone());
            }
            let player = get_data().me.as_ref().map(|it| BasicPlayer {
                avatar: UserManager::get_avatar(it.id).flatten(),
                id: it.id,
                rks: it.rks,
                historic_best: record.map_or(0, |it| it.score as u32),
            });
            let upload_fn: Option<UploadFn> = Some(Arc::new(move |data: Vec<u8>| {
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
                        id: i32,
                        exp_delta: f64,
                        new_best: bool,
                        improvement: u32,
                        new_rks: f32,
                    }
                    let resp: Resp = recv_raw(Client::post(
                        "/play/upload",
                        &Req {
                            chart: id.unwrap(),
                            token: STANDARD.encode(data),
                            chart_updated,
                        },
                    ))
                    .await?
                    .json()
                    .await?;
                    RECORD_ID.store(resp.id, Ordering::Relaxed);
                    Ok(RecordUpdateState {
                        best: resp.new_best,
                        improvement: resp.improvement,
                        gain_exp: resp.exp_delta as f32,
                        new_rks: Some(resp.new_rks),
                    })
                })
            }));
            if is_unlock {
                #[cfg(not(feature = "video"))]
                {
                    warn!("this build does not support unlock video.");
                    LoadingScene::new(mode, info, config, fs, player, upload_fn, update_fn, Some(preload))
                    .await
                    .map(|it| NextScene::Overlay(Box::new(it)))
                }
                #[cfg(feature = "video")]
                {
                    let chart = get_data_mut().charts.iter_mut().find(|it| it.local_path == local_path).unwrap();
                    if !chart.played_unlock {
                        chart.played_unlock = true;
                        save_data()?;
                    }

                    UnlockScene::new(mode, info, config, fs, player, upload_fn, update_fn, Some(preload))
                        .await
                        .map(|it| NextScene::Overlay(Box::new(it)))
                }
            } else {
                LoadingScene::new(mode, info, config, fs, player, upload_fn, update_fn, Some(preload))
                    .await
                    .map(|it| NextScene::Overlay(Box::new(it)))
            }
        })))
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
        let online = self.info.id.is_some();
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
                if self.info_edit.as_ref().unwrap().updated && !UPLOAD_NOT_SAVED.load(Ordering::SeqCst) {
                    Dialog::simple(tl!("upload-not-saved"))
                        .buttons(vec![ttl!("cancel").into_owned(), ttl!("confirm").into_owned()])
                        .listener(|_dialog, pos| {
                            if pos == 1 {
                                UPLOAD_NOT_SAVED.store(true, Ordering::SeqCst);
                            }
                            false
                        })
                        .show();
                } else {
                    let path = self.local_path.as_ref().unwrap();
                    if get_data().me.is_none() {
                        show_message(tl!("upload-login-first"));
                    } else if path.starts_with(':') {
                        show_message(tl!("upload-builtin"));
                    } else {
                        self.update_cksum_passed = None;
                        Dialog::plain(tl!("upload-rules"), tl!("upload-rules-content"))
                            .buttons(vec![ttl!("cancel").into_owned(), ttl!("confirm").into_owned()])
                            .listener(|_dialog, pos| {
                                if pos == 1 {
                                    CONFIRM_UPLOAD.store(true, Ordering::SeqCst);
                                }
                                false
                            })
                            .show();
                    }
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
            let mut r = Rect::new(0.04, 0., 0.23, 0.07);
            if ui.button("edit_tags", r, tl!("edit-tags")) {
                self.tags.set(self.info_edit.as_ref().unwrap().info.tags.clone());
                self.tags.enter(rt);
            }
            if is_owner && online {
                r.x += r.w + 0.01;
                if ui.button("overwrite", r, tl!("edit-overwrite")) {
                    request_file("overwrite");
                }
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
            Rect::new(width - 0.24, 0.01, 0.23, 0.08),
            rt,
            if self.ldb_std { tl!("ldb-std") } else { tl!("ldb-score") },
            0.6,
            true,
        );
        render_ldb(
            ui,
            &tl!("ldb"),
            self.side_content.width(),
            rt,
            &mut self.ldb_scroll,
            &mut self.ldb_fader,
            &self.icons.user,
            self.ldb.as_mut().map(|it| {
                it.1.iter_mut().map(|it| LdbDisplayItem {
                    player_id: it.inner.player.id,
                    rank: it.rank,
                    score: if self.ldb_std {
                        format!("{:07}", it.inner.std_score.unwrap_or(0.) as i64)
                    } else {
                        format!("{:07}", it.inner.score)
                    },
                    alt: Some(if self.ldb_std {
                        format!("{}ms", (it.inner.std.unwrap_or(0.) * 1000.) as i32)
                    } else {
                        format!("{:.2}%", it.inner.accuracy * 100.)
                    }),
                    btn: &mut it.btn,
                })
            }),
        );
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
            let mw = width - pad * 3.;
            if self.info.id.is_some() {
                let r = Rect::new(0.03, 0., mw, 0.12).nonuniform_feather(-0.03, -0.01);
                self.open_web_btn.render_text(ui, r, rt, ttl!("open-in-web"), 0.6, true);
                dy!(r.h + 0.04);
            }
            if let Some(uploader) = &self.info.uploader {
                let c = 0.06;
                let s = 0.05;
                let r = ui.avatar(c, c, s, rt, UserManager::opt_avatar(uploader.id, &self.icons.user));
                self.uploader_btn.set(ui, Rect::new(c - s, c - s, s * 2., s * 2.));
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
            item(tl!("info-name"), self.info.name.as_str().into());
            item(tl!("info-composer"), self.info.composer.as_str().into());
            item(tl!("info-charter"), self.info.charter.as_str().into());
            item(tl!("info-difficulty"), format!("{} ({:.1})", self.info.level, self.info.difficulty).into());
            item(tl!("info-desc"), self.info.intro.as_str().into());
            if let Some(entity) = &self.entity {
                item(tl!("info-rating"), entity.rating.map_or(Cow::Borrowed("NaN"), |r| format!("{:.2} / 5.00", r * 5.).into()));
                item(
                    tl!("info-type"),
                    format!(
                        "{}{}",
                        if entity.reviewed { tl!("reviewed") } else { tl!("unreviewed") },
                        match (entity.stable, entity.ranked) {
                            (true, true) => ttl!("chart-ranked"),
                            (true, false) => ttl!("chart-special"),
                            (false, _) => ttl!("chart-unstable"),
                        }
                    )
                    .into(),
                );
                item(tl!("info-tags"), entity.tags.iter().map(|it| format!("#{it}")).join(" ").into());
            }
            if let Some(id) = self.info.id {
                item("ID".into(), id.to_string().into());
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
            dy!(ui.text(tl!("mods")).size(0.9).draw_using(&BOLD_FONT).h + 0.02);
            let rh = ITEM_HEIGHT * 3. / 5.;
            let rr = Rect::new(width - 0.24, (ITEM_HEIGHT - rh) / 2., 0.2, rh);
            let mut index = 0;
            let mut item = |title: Cow<'_, str>, subtitle: Option<Cow<'_, str>>, flag: Mods| {
                const TITLE_SIZE: f32 = 0.6;
                const SUBTITLE_SIZE: f32 = 0.35;
                const LEFT: f32 = 0.03;
                const PAD: f32 = 0.01;
                const SUB_MAX_WIDTH: f32 = 0.46;
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
                        .pos(LEFT, (ITEM_HEIGHT + h) / 2. - r2.h)
                        .size(SUBTITLE_SIZE)
                        .max_width(SUB_MAX_WIDTH)
                        .multiline()
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
                btn.build(ui, rt, rr, |ui, path| {
                    let ct = rr.center();
                    ui.fill_path(&path, if on { WHITE } else { ui.background() });
                    ui.text(if on { ttl!("switch-on") } else { ttl!("switch-off") })
                        .pos(ct.x, ct.y)
                        .anchor(0.5, 0.5)
                        .no_baseline()
                        .size(0.5 * (1. - (1. - rr.h / oh).powf(1.3)))
                        .max_width(rr.w)
                        .color(if on { Color::new(0.3, 0.3, 0.3, 1.) } else { WHITE })
                        .draw();
                });
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
        let def_illu = self.illu.texture.1.clone();
        self.save_task = Some(Task::new(async move {
            let dir = prpr::dir::Dir::new(format!("{}/{path}", dir::charts()?))?;
            let patches = edit.to_patches().await.with_context(|| tl!("edit-load-file-failed"))?;
            if !is_owner && patches.contains_key(&info.chart) {
                bail!(tl!("edit-downloaded"));
            }
            for (name, bytes) in patches.into_iter() {
                dir.create(name)?.write_all(&bytes)?;
            }
            let _ = std::fs::remove_file(thumbnail_path(&path)?);
            load_local_tuple(&path, def_illu, info).await
        }));
    }

    fn update_chart_info(&self) -> Result<()> {
        Self::global_update_chart_info(self.local_path.as_ref().unwrap(), self.info.clone())
    }

    fn global_update_chart_info(local_path: &str, info: BriefChartInfo) -> Result<()> {
        let _ = std::fs::remove_file(thumbnail_path(local_path)?);
        get_data_mut().charts[get_data().find_chart_by_path(local_path).unwrap()].info = info;
        NEED_UPDATE.store(true, Ordering::Relaxed);
        save_data()?;
        Ok(())
    }

    fn load_tuple(&mut self, (local_path, info, preview, illu): LocalTuple) -> Result<()> {
        self.local_path = Some(local_path);
        if let Some(preview) = &mut self.preview {
            preview.pause()?;
        }
        self.preview = Some(create_music(preview)?);
        self.info = info.into();
        self.illu = illu;
        self.update_chart_info()?;

        Ok(())
    }
}

impl Scene for SongScene {
    fn on_result(&mut self, tm: &mut TimeManager, res: Box<dyn Any>) -> Result<()> {
        let res = match res.downcast::<SimpleRecord>() {
            Err(res) => res,
            Ok(rec) => {
                self.fade_start = tm.now() as f32 + FADE_IN_TIME;
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
                    let dir = format!("{}/{}", dir::charts()?, self.local_path.as_ref().unwrap().replace(':', "_"));
                    let path = std::path::Path::new(&dir);
                    if !path.exists() {
                        std::fs::create_dir_all(path)?;
                    }
                    let dir = prpr::dir::Dir::new(dir)?;
                    match self.chart_type {
                        ChartType::Integrated => {
                            dir.create("offset")?.write_all(&offset.to_be_bytes())?;
                            if let Ok(Some(info)) = ASSET_CHART_INFO.lock().as_deref_mut() {
                                info.offset = offset;
                            }
                        }
                        _ => {
                            let mut info: ChartInfo = serde_yaml::from_reader(&dir.open("info.yml")?)?;
                            info.offset = offset;
                            dir.create("info.yml")?.write_all(serde_yaml::to_string(&info)?.as_bytes())?;
                            let path = thumbnail_path(self.local_path.as_ref().unwrap())?;
                            if path.exists() {
                                std::fs::remove_file(path)?;
                            }
                        }
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
        if self.scene_task.is_some()
            || self.save_task.is_some()
            || self.upload_task.is_some()
            || self.review_task.is_some()
            || self.edit_tags_task.is_some()
            || self.rate_task.is_some()
            || self.overwrite_task.is_some()
            || self.update_cksum_task.is_some()
        {
            return Ok(true);
        }
        if self.downloading.is_some() {
            if let Some(dl) = &mut self.downloading {
                if dl.touch(touch, t) {
                    self.downloading = None;
                    return Ok(true);
                }
            }
            return Ok(false);
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
                        if let Some(index) = get_data().find_chart_by_path(self.local_path.as_deref().unwrap()) {
                            let chart = &mut get_data_mut().charts[index];
                            if chart.mods != self.mods {
                                chart.mods = self.mods;
                                save_data()?;
                            }
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
                        if self.ldb_type_btn.touch(touch, rt) {
                            self.ldb_std ^= true;
                            self.ldb_scroll.y_scroller.offset = 0.;
                            self.load_ldb();
                            return Ok(true);
                        }
                        if self.ldb_scroll.touch(touch, t) {
                            return Ok(true);
                        }
                        if let Some((_, ldb)) = &mut self.ldb {
                            for item in ldb {
                                if item.btn.touch(touch) {
                                    button_hit();
                                    self.sf
                                        .goto(t, ProfileScene::new(item.inner.player.id, self.icons.user.clone(), self.rank_icons.clone()));
                                    return Ok(true);
                                }
                            }
                        }
                    }
                    SideContent::Info => {
                        if self.info_scroll.touch(touch, t) {
                            return Ok(true);
                        }
                        if self.uploader_btn.touch(touch) {
                            button_hit();
                            self.sf.goto(
                                t,
                                ProfileScene::new(self.info.uploader.as_ref().unwrap().id, self.icons.user.clone(), self.rank_icons.clone()),
                            );
                            return Ok(true);
                        }
                        if self.open_web_btn.touch(touch, rt) {
                            open_url(&format!("https://phira.moe/chart/{}", self.info.id.unwrap()))?;
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
        if self.back_btn.touch(touch) {
            button_hit();
            self.next_scene = Some(NextScene::PopWithResult(Box::new(false)));
            return Ok(true);
        }
        if self.play_btn.touch(touch, t) {
            if self.local_path.is_some() {
                self.launch(GameMode::Normal, false)?;
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
                UPLOAD_NOT_SAVED.store(false, Ordering::SeqCst);
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
            let mut tags = self.tags.tags.tags().to_vec();
            tags.push(self.tags.division.to_owned());
            if !self.side_enter_time.is_infinite() && matches!(self.side_content, SideContent::Edit) {
                let edit = self.info_edit.as_mut().unwrap();
                edit.info.tags = tags;
                edit.updated = true;
            } else {
                let id = self.info.id.unwrap();
                self.entity.as_mut().unwrap().tags = tags.clone();
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
            if let Some(id) = self.info.id {
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
                                && self.local_path.is_some()
                            {
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
            if let Some(tuple) = dl.check()? {
                self.local_path = dl.local_path.take();
                self.downloading = None;
                if let Some(tuple) = tuple {
                    self.load_tuple(tuple)?;
                }
                self.update_menu();
            }
        }
        if let Some(task) = &mut self.scene_task {
            if let Some(res) = poll_future(task.as_mut()) {
                match res {
                    Err(err) => {
                        error!(?err, "failed to play");
                        *self.background.lock().unwrap() = None;
                        self.tr_start = f32::NAN;
                        let error = format!("{err:?}");
                        Dialog::plain(tl!("failed-to-play"), error)
                            .buttons(vec![tl!("play-cancel").into_owned(), tl!("play-switch-to-offline").into_owned()])
                            .listener(move |_dialog, pos| {
                                if pos == 1 {
                                    get_data_mut().config.offline_mode = true;
                                    let _ = save_data();
                                    show_message(tl!("switched-to-offline")).ok();
                                }
                                false
                            })
                            .show();
                    }
                    Ok(scene) => self.next_scene = Some(scene),
                }
                self.scene_task = None;
            }
        }
        if let Some(task) = &mut self.fetch_best_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        warn!(?err, "failed to fetch best record");
                    }
                    Ok(rec) => {
                        self.update_record(rec)?;
                    }
                }
                self.fetch_best_task = None;
            }
        }
        if self.menu.changed() {
            let option = self.menu_options[self.menu.selected()];
            match option {
                "delete" => {
                    confirm_delete(self.should_delete.clone());
                }
                "rate" => {
                    self.rate_dialog.enter(tm.real_time() as _);
                }
                "exercise" => {
                    self.launch(GameMode::Exercise, false)?;
                }
                "offset" => {
                    self.launch(GameMode::TweakOffset, false)?;
                }
                "unlock" => {
                    self.launch(GameMode::Normal, true)?;
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
                    self.tags.set(entity.tags.clone());
                    self.tags.enter(tm.real_time() as _);
                }
                "stabilize" => {
                    confirm_dialog(tl!("stabilize"), tl!("stabilize-warn"), Arc::clone(&self.should_stabilize));
                }
                "stabilize-approve" | "stabilize-approve-ranked" => {
                    let kind = if option == "stabilize-approve-ranked" { 1 } else { 0 };
                    let id = self.info.id.unwrap();
                    self.review_task = Some(Task::new(async move {
                        let resp: StableR = recv_raw(Client::post(
                            format!("/chart/{id}/stabilize"),
                            &json!({
                                "kind": kind,
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
                "stabilize-comment" => {
                    request_input("stabilize-comment", "");
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
        if self.should_stabilize.fetch_and(false, Ordering::Relaxed) {
            let id = self.info.id.unwrap();
            self.stabilize_task = Some(Task::new(async move {
                recv_raw(Client::post(format!("/chart/{id}/req-stabilize"), &())).await?;
                Ok(())
            }));
        }
        if let Some(task) = &mut self.save_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        show_error(err.context(tl!("edit-save-failed")));
                    }
                    Ok(tuple) => {
                        self.info_edit.as_mut().unwrap().updated = false;
                        self.load_tuple(tuple)?;
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
            let local_path = self.local_path.clone().unwrap();
            let id = self.info.id.clone();
            self.update_cksum_task = Some(Task::new(async move {
                if let Some(id) = id {
                    use hex::ToHex;
                    let mut fs = fs_from_path(&local_path)?;
                    let info = prpr::fs::load_info(fs.as_mut()).await?;
                    let chart = fs.load_file(&info.chart).await?;
                    let cksum: String = Sha256::digest(&chart).encode_hex();
                    #[derive(Deserialize)]
                    struct VerifyR {
                        ok: bool,
                    }
                    let resp: VerifyR = recv_raw(Client::get(format!("/chart/{id}/verify-cksum?checksum={cksum}")))
                        .await?
                        .json()
                        .await?;
                    Ok(resp.ok)
                } else {
                    Ok(true)
                }
            }));
        }
        if let Some(task) = &mut self.update_cksum_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        show_error(err.context(tl!("upload-failed")));
                    }
                    Ok(ok) => {
                        if ok {
                            CONFIRM_CKSUM.store(true, Ordering::Relaxed);
                        } else {
                            Dialog::simple(tl!("upload-confirm-clear-ldb"))
                                .buttons(vec![ttl!("cancel").into_owned(), ttl!("confirm").into_owned()])
                                .listener(move |_dialog, pos| {
                                    if pos == 1 {
                                        CONFIRM_CKSUM.store(true, Ordering::Relaxed);
                                    }
                                    false
                                })
                                .show();
                        }
                    }
                }
                self.update_cksum_task = None;
            }
        }
        if CONFIRM_CKSUM.fetch_and(false, Ordering::Relaxed) {
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
                "stabilize-comment" => {
                    let id = self.info.id.unwrap();
                    self.review_task = Some(Task::new(async move {
                        recv_raw(Client::post(
                            format!("/chart/{id}/stabilize-comment"),
                            &json!({
                                "comment": text,
                            }),
                        ))
                        .await?;
                        Ok(tl!("stabilize-commented").into())
                    }));
                }
                "stabilize-deny-reason" => {
                    let id = self.info.id.unwrap();
                    self.review_task = Some(Task::new(async move {
                        let resp: StableR = recv_raw(Client::post(
                            format!("/chart/{id}/stabilize"),
                            &json!({
                                "kind": -1,
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
        if let Some((id, file)) = take_file() {
            if id == "overwrite" {
                self.overwrite_from = Some(file);
                CONFIRM_OVERWRITE.store(false, Ordering::SeqCst);
                Dialog::simple(tl!("edit-overwrite-confirm"))
                    .buttons(vec![ttl!("cancel").into_owned(), ttl!("confirm").into_owned()])
                    .listener(move |_dialog, pos| {
                        if pos == 1 {
                            CONFIRM_OVERWRITE.store(true, Ordering::SeqCst);
                        }
                        false
                    })
                    .show();
            } else {
                return_file(id, file);
            }
        }
        if CONFIRM_OVERWRITE.fetch_and(false, Ordering::Relaxed) {
            let path = self.overwrite_from.take().unwrap();
            let local_path = self.local_path.clone().unwrap();
            let def_illu = self.illu.texture.1.clone();
            let chart_id = self.info.id.unwrap();
            let owner = self.info.uploader.as_ref().unwrap().id;
            self.overwrite_task = Some(Task::new(async move {
                let (dir, id) = gen_custom_dir()?;
                let to_path = format!("{}/{}/", dir::charts()?, local_path);
                if let Err(err) = import_chart_to(&dir, id, path).await {
                    std::fs::remove_dir_all(dir)?;
                    return Err(err);
                }
                let mut fs = prpr::fs::fs_from_file(&dir)?;
                let mut info = prpr::fs::load_info(fs.as_mut()).await?;
                drop(fs);
                info.id = Some(chart_id);
                info.uploader = Some(owner);
                serde_yaml::to_writer(File::create(dir.join("info.yml"))?, &info)?;

                std::fs::remove_dir_all(&to_path)?;
                std::fs::rename(&dir, &to_path)?;

                load_local_tuple(&local_path, def_illu, info).await
            }));
        }
        if let Some(task) = &mut self.overwrite_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        show_error(err.context(tl!("edit-overwrite-failed")));
                    }
                    Ok(tuple) => {
                        self.load_tuple(tuple)?;
                        show_message(tl!("edit-overwrite-success")).ok();
                    }
                }
                self.overwrite_task = None;
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
                        warn!(?err, "failed to fetch my rating status");
                    }
                    Ok(score) => {
                        self.rate_dialog.rate.score = score;
                        self.my_rate_score = Some(score);
                    }
                }
                self.my_rating_task = None;
            }
        }
        if let Some(task) = &mut self.scene_task {
            if let Some(res) = poll_future(task.as_mut()) {
                self.next_scene = Some(res?);
                self.scene_task = None;
            }
        }
        if self.tr_start.is_nan() && self.background.lock().unwrap().is_some() {
            self.tr_start = rt;
        }

        Ok(())
    }

    fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        set_camera(&ui.camera());
        let t = tm.now() as f32;
        ui.fill_rect(ui.screen_rect(), (*self.illu.texture.1, ui.screen_rect()));
        ui.fill_rect(ui.screen_rect(), semi_black(0.55));

        let r = ui.back_rect();
        self.back_btn.set(ui, r);
        ui.fill_rect(r, (*self.icons.back, r, ScaleType::Fit));

        ui.alpha::<Result<()>>(((t - self.fade_start) / FADE_IN_TIME).clamp(-1., 0.) + 1., |ui| {
            let r = ui
                .text(&self.info.name)
                .max_width(0.57 - r.right())
                .size(1.2)
                .pos(r.right() + 0.02, r.y)
                .draw();
            ui.text(&self.info.composer)
                .size(0.5)
                .pos(r.x + 0.02, r.bottom() + 0.03)
                .color(semi_white(0.8))
                .draw();

            // bottom bar
            let s = 0.25;
            let r = Rect::new(-0.94, ui.top - s - 0.06, s, s);
            let icon = self.record.as_ref().map_or(0, |it| icon_index(it.score as _, it.full_combo));
            ui.fill_rect(r, (*self.rank_icons[icon], r, ScaleType::Fit));
            let score = self.record.as_ref().map(|it| it.score).unwrap_or_default();
            let accuracy = self.record.as_ref().map(|it| it.accuracy).unwrap_or_default();
            let r = ui
                .text(format!("{score:07}"))
                .pos(r.right() + 0.01, r.center().y)
                .anchor(0., 1.)
                .size(1.2)
                .draw();
            ui.text(format!("{:.2}%", accuracy * 100.))
                .pos(r.x, r.bottom() + 0.01)
                .anchor(0., 0.)
                .size(0.7)
                .color(semi_white(0.7))
                .draw();

            if self.info.id.is_some() {
                let h = 0.09;
                let mut r = Rect::new(r.x, r.y - h, h, h);
                ui.fill_rect(r, (*self.icons.ldb, r, ScaleType::Fit));
                if let Some((rank, _)) = &self.ldb {
                    ui.text(if let Some(rank) = rank {
                        format!("#{rank}")
                    } else {
                        tl!("ldb-no-rank").into_owned()
                    })
                    .pos(r.right() + 0.01, r.center().y)
                    .anchor(0., 0.5)
                    .no_baseline()
                    .size(0.7)
                    .draw();
                } else {
                    ui.loading(
                        r.right() + 0.04,
                        r.center().y,
                        t,
                        WHITE,
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
            self.play_btn.render_shadow(ui, r, t, |ui, path| {
                ui.fill_path(&path, semi_white(0.3));
                let r = r.feather(-0.04);
                ui.fill_rect(
                    r,
                    (
                        if self.local_path.is_some() {
                            *self.icons.play
                        } else {
                            *self.icons.download
                        },
                        r,
                        ScaleType::Fit,
                    ),
                );
            });

            ui.scope(|ui| {
                ui.dx(1. - 0.03);
                ui.dy(-ui.top + 0.03);
                let s = 0.08;
                let r = Rect::new(-s, 0., s, s);
                let cc = semi_white(0.4);
                ui.fill_rect(r, (*self.icons.menu, r, ScaleType::Fit, if self.menu_options.is_empty() { cc } else { WHITE }));
                self.menu_btn.set(ui, r);
                if self.need_show_menu {
                    self.need_show_menu = false;
                    self.menu.set_bottom(true);
                    self.menu.set_selected(usize::MAX);
                    let d = 0.28;
                    self.menu.show(ui, t, Rect::new(r.x - d, r.bottom() + 0.02, r.w + d, 0.5));
                }
                ui.dx(-r.w - 0.03);
                ui.fill_rect(r, (*self.icons.info, r, ScaleType::Fit));
                self.info_btn.set(ui, r);
                ui.dx(-r.w - 0.03);
                if self.local_path.as_ref().map_or(true, |it| !it.starts_with(':')) {
                    ui.fill_rect(r, (*self.icons.edit, r, ScaleType::Fit, if self.local_path.is_some() { WHITE } else { cc }));
                    self.edit_btn.set(ui, r);
                    ui.dx(-r.w - 0.03);
                }
                ui.fill_rect(r, (*self.icons.r#mod, r, ScaleType::Fit, if self.local_path.is_some() { WHITE } else { cc }));
                self.mod_btn.set(ui, r);
            });

            if let Some(dl) = &mut self.downloading {
                dl.render(ui, t);
            }

            let rt = tm.real_time() as f32;
            if self.side_enter_time.is_finite() {
                let p = ((rt - self.side_enter_time.abs()) / EDIT_TRANSIT).min(1.);
                let p = 1. - (1. - p).powi(3);
                let p = if self.side_enter_time < 0. { 1. - p } else { p };
                ui.fill_rect(ui.screen_rect(), semi_black(p * 0.6));
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

            Ok(())
        })?;

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
        if self.edit_tags_task.is_some() || self.rate_task.is_some() || self.overwrite_task.is_some() || self.update_cksum_task.is_some() {
            ui.full_loading("", t);
        }
        let rt = tm.real_time() as f32;
        self.tags.render(ui, rt);
        self.rate_dialog.render(ui, rt);

        if !self.tr_start.is_nan() {
            let p = ((rt - self.tr_start - 0.2) / 0.4).clamp(0., 1.);
            if p >= 1. {
                self.tr_start = f32::NAN;
            }
            let p = 1. - (1. - p).powi(3);
            let mut r = ui.screen_rect();
            r.y += r.h * (1. - p);
            rect_shadow(r, 0.01, 0.5);
            ui.fill_rect(r, (**self.background.lock().unwrap().as_ref().unwrap(), r));
            ui.fill_rect(r, semi_black(0.3));
        }

        self.sf.render(ui, t);

        Ok(())
    }

    fn next_scene(&mut self, tm: &mut TimeManager) -> NextScene {
        if !self.tr_start.is_nan() {
            return NextScene::None;
        }
        if let Some(scene) = self.next_scene.take().or_else(|| self.sf.next_scene(tm.now() as _)) {
            *self.background.lock().unwrap() = None;
            if let Some(music) = &mut self.preview {
                let _ = music.pause();
            }
            scene
        } else {
            NextScene::None
        }
    }
}
