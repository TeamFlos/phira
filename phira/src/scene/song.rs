prpr::tl_file!("song");

use super::{confirm_delete, fs_from_path};
use crate::{
    client::{recv_raw, Chart, Client, Ptr, UserManager},
    data::{BriefChartInfo, LocalChart},
    dir, get_data, get_data_mut,
    page::{ChartItem, Illustration},
    popup::Popup,
    save_data,
};
use anyhow::{anyhow, Context, Result};
use cap_std::ambient_authority;
use futures_util::StreamExt;
use macroquad::prelude::*;
use prpr::{
    config::Config,
    core::Tweenable,
    ext::{screen_aspect, semi_black, semi_white, unzip_into, RectExt, SafeTexture, ScaleType},
    fs,
    info::ChartInfo,
    judge::icon_index,
    scene::{
        load_scene, loading_scene, show_error, show_message, take_loaded_scene, BasicPlayer, GameMode, LoadingScene, NextScene, RecordUpdateState,
        Scene, SimpleRecord,
    },
    task::Task,
    time::TimeManager,
    ui::{button_hit, render_chart_info, ChartInfoEdit, DRectButton, Dialog, RectButton, Scroll, Ui, UI_AUDIO},
};
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
                loop_: true,
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
}

impl SideContent {
    fn width(&self) -> f32 {
        match self {
            Self::Edit => 0.84,
        }
    }
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

    next_scene: Option<NextScene>,

    preview: Option<Music>,
    preview_task: Option<Task<Result<AudioClip>>>,

    load_task: Option<Task<Result<Arc<Chart>>>>,
    entity: Option<Arc<Chart>>,
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

    side_content: SideContent,
    side_enter_time: f32,

    save_task: Option<Task<Result<(ChartInfo, AudioClip)>>>,
    upload_task: Option<Task<Result<()>>>,
}

impl SongScene {
    pub fn new(
        mut chart: ChartItem,
        local_path: Option<String>,
        icon_back: SafeTexture,
        icon_play: SafeTexture,
        icon_download: SafeTexture,
        icon_menu: SafeTexture,
        icon_edit: SafeTexture,
        icons: [SafeTexture; 8],
    ) -> Self {
        if let Some(path) = &local_path {
            if let Some(id) = path.strip_prefix("download/") {
                chart.info.id = Some(id.parse().unwrap());
            }
        }
        let illu = if let Some(id) = chart.info.id {
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

            next_scene: None,

            preview: None,
            preview_task: Some(Task::new({
                let id = chart.info.id.clone();
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

            load_task: chart.info.id.clone().map(|it| Task::new(async move { Ptr::new(it).fetch().await })),
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

            side_content: SideContent::Edit,
            side_enter_time: f32::INFINITY,

            save_task: None,
            upload_task: None,
        }
    }

    fn start_download(&mut self) -> Result<()> {
        let chart = self.info.clone();
        let Some(entity) = self.entity.as_ref().map(Arc::clone) else {
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
                let path = format!("{}/{}", dir::downloaded_charts()?, chart.id.unwrap());
                async move {
                    let path = std::path::Path::new(&path);
                    if path.exists() {
                        if !path.is_dir() {
                            tokio::fs::remove_file(path).await?;
                        }
                    } else {
                        tokio::fs::create_dir(path).await?;
                    }
                    let dir = cap_std::fs::Dir::open_ambient_dir(path, ambient_authority())?;

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
                        unzip_into(Cursor::new(bytes), &dir)?;
                    }
                    // tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    *status.lock().unwrap() = tl!("dl-status-saving");
                    if let Some(prog) = prog_wk.upgrade() {
                        *prog.lock().unwrap() = None;
                    }

                    if prog_wk.strong_count() == 0 {
                        // cancelled
                        drop(dir);
                        tokio::fs::remove_dir_all(&path).await?;
                    }

                    let local_path = format!("download/{}", chart.id.unwrap());
                    Ok(LocalChart {
                        info: entity.to_info(),
                        local_path,
                        record: None,
                    })
                }
            }),
        });
        Ok(())
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
            self.menu_options.push("exercise");
            self.menu_options.push("offset");
        }
        self.menu.set_options(self.menu_options.iter().map(|it| tl!(it).into_owned()).collect());
    }

    fn launch(&mut self, mode: GameMode) -> Result<()> {
        let mut fs = fs_from_path(self.local_path.as_ref().unwrap())?;
        #[cfg(feature = "closed")]
        let rated = {
            let config = &get_data().config;
            !config.offline_mode && self.info.id.is_some() && !config.autoplay && config.speed >= 1.0 - 1e-3
        };
        #[cfg(not(feature = "closed"))]
        let rated = false;
        if !rated && self.info.id.is_some() && mode == GameMode::Normal {
            show_message(tl!("warn-unrated")).warn();
        }
        let id = self.info.id.clone();
        load_scene(async move {
            let mut info = fs::load_info(fs.as_mut()).await?;
            info.id = id;
            LoadingScene::new(
                mode,
                info,
                Config {
                    player_name: get_data()
                        .me
                        .as_ref()
                        .map(|it| it.name.clone())
                        .unwrap_or_else(|| tl!("guest").to_string()),
                    res_pack_path: {
                        let id = get_data().respack_id;
                        if id == 0 {
                            None
                        } else {
                            Some(format!("{}/{}", dir::respacks()?, get_data().respacks[id - 1]))
                        }
                    },
                    ..get_data().config.clone()
                },
                fs,
                get_data().me.as_ref().map(|it| BasicPlayer {
                    avatar: UserManager::get_avatar(it.id),
                    id: it.id,
                    rks: it.rks,
                }),
                None,
                Some(Arc::new(move |data| {
                    Task::new(async move {
                        #[derive(Serialize)]
                        struct Req {
                            chart: i32,
                            token: String,
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
                                token: base64::encode(data),
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

    fn side_chart_info(&mut self, ui: &mut Ui, rt: f32) -> Result<()> {
        let h = 0.11;
        let pad = 0.03;
        let width = self.side_content.width() - pad;

        let vpad = 0.02;
        let hpad = 0.01;
        let dx = width / 3.;
        let mut r = Rect::new(hpad, ui.top * 2. - h + vpad, dx - hpad * 2., h - vpad * 2.);
        if ui.button("cancel", r, tl!("edit-cancel")) {
            self.side_enter_time = -rt;
        }
        r.x += dx;
        if ui.button("upload", r, tl!("edit-upload")) && self.upload_task.is_none() && self.save_task.is_none() {
            let path = self.local_path.as_ref().unwrap();
            if get_data().me.is_none() {
                show_message(tl!("upload-login-first"));
            } else if path.starts_with(':') {
                show_message(tl!("upload-builtin"));
            } else if self.info.id.is_some() {
                show_message(tl!("upload-downloaded"));
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
        r.x += dx;
        if ui.button("save", r, tl!("edit-save")) && self.upload_task.is_none() && self.save_task.is_none() {
            self.save_edit();
        }

        ui.ensure_touches().retain(|it| !matches!(it.phase, TouchPhase::Started) || self.edit_scroll.contains(it));

        self.edit_scroll.size((width, ui.top * 2. - h));
        self.edit_scroll.render(ui, |ui| {
            let (w, h) = render_chart_info(ui, self.info_edit.as_mut().unwrap(), width);
            (w, h + 0.1)
        });
        Ok(())
    }

    fn save_edit(&mut self) {
        let Some(edit) = &self.info_edit else { unreachable!() };
        let info = edit.info.clone();
        let path = self.local_path.clone().unwrap();
        let edit = edit.clone();
        self.save_task = Some(Task::new(async move {
            let dir = cap_std::fs::Dir::open_ambient_dir(format!("{}/{path}", dir::charts()?), ambient_authority())?;
            let patches = edit.to_patches().await.with_context(|| tl!("edit-load-file-failed"))?;
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
        save_data()?;
        Ok(())
    }
}

impl Scene for SongScene {
    fn on_result(&mut self, _tm: &mut TimeManager, res: Box<dyn Any>) -> Result<()> {
        let _res = match res.downcast::<SimpleRecord>() {
            Err(res) => res,
            Ok(rec) => {
                self.update_record(*rec)?;
                return Ok(());
            }
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
        if loading_scene() {
            return Ok(false);
        }
        if self.menu.showing() {
            self.menu.touch(touch, t);
            return Ok(true);
        }
        if !self.side_enter_time.is_infinite() {
            if self.side_enter_time > 0. && tm.real_time() as f32 > self.side_enter_time + EDIT_TRANSIT {
                if touch.position.x < 1. - self.side_content.width() && touch.phase == TouchPhase::Started && self.save_task.is_none() {
                    self.side_enter_time = -tm.real_time() as _;
                    return Ok(true);
                }
                match self.side_content {
                    SideContent::Edit => {
                        if self.edit_scroll.touch(touch, t) {
                            return Ok(true);
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
        if self.local_path.is_some() && self.edit_btn.touch(touch) {
            let Some(path) = self.local_path.as_ref() else { unreachable!() };
            let mut info: ChartInfo = serde_yaml::from_str(&std::fs::read_to_string(format!("{}/{path}/info.yml", dir::charts()?))?)?;
            info.id = self.info.id;
            self.info_edit = Some(ChartInfoEdit::new(info));
            self.side_content = SideContent::Edit;
            self.side_enter_time = tm.real_time() as _;
            return Ok(true);
        }
        Ok(false)
    }

    fn update(&mut self, tm: &mut TimeManager) -> Result<()> {
        let t = tm.now() as f32;
        self.menu.update(t);
        self.illu.settle(t);
        if self.side_enter_time < 0. && -tm.real_time() as f32 + EDIT_TRANSIT < self.side_enter_time {
            self.side_enter_time = f32::INFINITY;
        }
        if let Some(task) = &mut self.load_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => show_error(err.context(tl!("load-charts-failed"))),
                    Ok(chart) => {
                        self.entity = Some(chart);
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
                        self.local_path = Some(chart.local_path.clone());
                        get_data_mut().charts.push(chart);
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
                "exercise" => {
                    self.launch(GameMode::Normal)?;
                }
                "offset" => {
                    self.launch(GameMode::TweakOffset)?;
                }
                _ => {}
            }
        }
        if self.should_delete.fetch_and(false, Ordering::Relaxed) {
            self.next_scene = Some(NextScene::PopWithResult(Box::new(true)));
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
                    Ok(_) => {
                        show_message(tl!("upload-success")).ok();
                    }
                }
                self.upload_task = None;
            }
        }
        match self.side_content {
            SideContent::Edit => {
                self.edit_scroll.update(t);
            }
        }
        if CONFIRM_UPLOAD.fetch_and(false, Ordering::Relaxed) {
            let path = self.local_path.clone().unwrap();
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
                let id = Client::upload_file("chart.zip", chart_bytes)
                    .await
                    .with_context(|| tl!("upload-chart-failed"))?;
                recv_raw(Client::post(
                    "/chart/upload",
                    &json!({
                        "file": id,
                    }),
                ))
                .await?;
                Ok(())
            }));
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
            .max_width(0.8 - r.right())
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
                self.menu.show(ui, t, Rect::new(r.x - d, r.bottom(), r.w + d, 0.4));
            }
            ui.dx(-r.w - 0.03);
            ui.fill_rect(r, (*self.icon_edit, r, ScaleType::Fit, if self.local_path.is_some() { c } else { cc }));
            self.edit_btn.set(ui, r);
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
                ui.fill_rect(r, (Color::default(), (r.x, r.y), Color::new(0., 0., 0., p * 0.6), (r.right(), r.y)));

                match self.side_content {
                    SideContent::Edit => self.side_chart_info(ui, rt),
                }
            })?;
        }

        self.menu.render(ui, t, 1.);

        if self.save_task.is_some() {
            ui.full_loading(tl!("edit-saving"), t);
        }
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
