prpr::tl_file!("song");

use crate::{
    client::{recv_raw, Client, PZChart, PZUser, Ptr, UserManager},
    data::{BriefChartInfo, LocalChart},
    dir, get_data, get_data_mut,
    page::{ChartItem, Illustration},
    save_data,
};
use anyhow::{anyhow, Context, Result};
use cap_std::ambient_authority;
use futures_util::StreamExt;
use macroquad::prelude::*;
use prpr::{
    config::Config,
    ext::{screen_aspect, semi_black, semi_white, RectExt, SafeTexture, ScaleType},
    fs,
    scene::{
        load_scene, loading_scene, show_error, show_message, take_loaded_scene, BasicPlayer, GameMode, LoadingScene, NextScene, RecordUpdateState,
        Scene,
    },
    task::Task,
    time::TimeManager,
    ui::{button_hit, DRectButton, Dialog, RectButton, Ui, UI_AUDIO},
};
use sasa::{AudioClip, Music, MusicParams};
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    io::{Cursor, Write},
    path::Path,
    sync::{Arc, Mutex, Weak},
};
use zip::ZipArchive;

use super::fs_from_path;

const FADE_IN_TIME: f32 = 0.3;
const CHART_ITEM_H: f32 = 0.11;

fn with_effects(data: Vec<u8>, range: Option<(u32, u32)>) -> Result<AudioClip> {
    let (mut frames, sample_rate) = AudioClip::decode(data)?;
    if let Some((begin, end)) = range {
        frames.drain((end as usize * sample_rate as usize).min(frames.len())..);
        frames.drain(..(begin as usize * sample_rate as usize));
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

pub struct SongScene {
    illu: Illustration,

    first_in: bool,

    back_btn: RectButton,
    play_btn: DRectButton,

    icon_back: SafeTexture,
    icon_play: SafeTexture,
    icon_download: SafeTexture,

    next_scene: Option<NextScene>,

    preview: Option<Music>,
    preview_task: Option<Task<Result<AudioClip>>>,

    load_task: Option<Task<Result<Arc<PZChart>>>>,
    entity: Option<Arc<PZChart>>,
    info: BriefChartInfo,
    local_path: Option<String>,

    downloading: Option<Downloading>,
    cancel_download_btn: DRectButton,
    loading_last: f32,
}

impl SongScene {
    pub fn new(chart: ChartItem, local_path: Option<String>, icon_back: SafeTexture, icon_play: SafeTexture, icon_download: SafeTexture) -> Self {
        let illu = if let Some(id) = chart.info.id {
            Illustration {
                texture: chart.illu.texture.clone(),
                task: Some(Task::new({
                    async move {
                        let chart = Ptr::<PZChart>::new(id).load().await?;
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
        Self {
            illu,

            first_in: true,

            back_btn: RectButton::new(),
            play_btn: DRectButton::new(),

            icon_back,
            icon_play,
            icon_download,

            next_scene: None,

            preview: None,
            preview_task: Some(Task::new({
                let id = chart.info.id.clone();
                async move {
                    if let Some(id) = id {
                        let chart = Ptr::<PZChart>::new(id).fetch().await?;
                        with_effects(chart.preview.fetch().await?.to_vec(), None)
                    } else {
                        // let mut fs = fs_from_path(&path)?;
                        // let info = fs::load_info(fs.deref_mut()).await?;
                        // if let Some(preview) = info.preview {
                        // with_effects(fs.load_file(&preview).await?, None)
                        // } else {
                        // with_effects(
                        // fs.load_file(&info.music).await?,
                        // Some((info.preview_start as u32, info.preview_end.ceil() as u32)),
                        // )
                        // }
                        todo!()
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
                        let mut zip = ZipArchive::new(Cursor::new(bytes))?;
                        for i in 0..zip.len() {
                            let mut entry = zip.by_index(i)?;
                            if entry.is_dir() {
                                dir.create_dir_all(entry.name())?;
                            } else {
                                let mut file = dir.create(entry.name())?;
                                std::io::copy(&mut entry, &mut file)?;
                            }
                        }
                    }
                    *status.lock().unwrap() = tl!("dl-status-saving");
                    if let Some(prog) = prog_wk.upgrade() {
                        *prog.lock().unwrap() = None;
                    }
                    // if let Some(preview) = &song.preview {
                    // download(&dir, "preview", &preview.url, &prog_wk).await?;
                    // }

                    if prog_wk.strong_count() == 0 {
                        // cancelled
                        drop(dir);
                        tokio::fs::remove_dir_all(&path).await?;
                    }

                    let local_path = format!("download/{}", chart.id.unwrap());
                    Ok(LocalChart {
                        info: entity.to_info(),
                        local_path,
                    })
                }
            }),
        });
        Ok(())
    }
}

impl Scene for SongScene {
    fn enter(&mut self, tm: &mut TimeManager, _target: Option<RenderTarget>) -> Result<()> {
        if self.first_in {
            self.first_in = false;
            tm.seek_to(-FADE_IN_TIME as _);
        }
        if let Some(music) = &mut self.preview {
            music.seek_to(0.)?;
            music.play()?;
        }
        Ok(())
    }

    fn touch(&mut self, tm: &mut TimeManager, touch: &Touch) -> Result<bool> {
        let t = tm.now() as f32;
        if loading_scene() {
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
            self.next_scene = Some(NextScene::PopWithResult(Box::new(())));
            return Ok(true);
        }
        if self.play_btn.touch(touch, t) {
            if let Some(local_path) = &self.local_path {
                let mut fs = fs_from_path(local_path)?;
                #[cfg(feature = "closed")]
                let rated = {
                    let config = &get_data().config;
                    !config.offline_mode && self.info.id.is_some() && !config.autoplay && config.speed >= 1.0 - 1e-3
                };
                #[cfg(not(feature = "closed"))]
                let rated = false;
                if !rated && self.info.id.is_some() {
                    show_message(tl!("warn-unrated")).warn();
                }
                let id = self.info.id.clone();
                load_scene(async move {
                    let mut info = fs::load_info(fs.as_mut()).await?;
                    info.id = id;
                    LoadingScene::new(
                        GameMode::Normal,
                        info,
                        Config {
                            player_name: get_data()
                                .me
                                .as_ref()
                                .map(|it| it.name.clone())
                                .unwrap_or_else(|| tl!("guest").to_string()),
                            res_pack_path: get_data()
                                .config
                                .res_pack_path
                                .as_ref()
                                .map(|it| format!("{}/{it}", dir::root().unwrap())),
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
                                let resp: Resp = recv_raw(
                                    Client::post(
                                        "/play/upload",
                                        &Req {
                                            chart: id.unwrap(),
                                            token: base64::encode(data),
                                        },
                                    )
                                    .await,
                                )
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
            } else {
                self.start_download()?;
            }
            return Ok(true);
        }
        Ok(false)
    }

    fn update(&mut self, tm: &mut TimeManager) -> Result<()> {
        let t = tm.now() as f32;
        self.illu.settle(t);
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
                        self.preview = Some(music);
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
            .size(1.2)
            .pos(r.right() + 0.02, r.bottom() - 0.06)
            .color(c)
            .draw();
        ui.text(&self.info.composer)
            .size(0.5)
            .pos(r.x + 0.02, r.bottom() + 0.03)
            .color(Color { a: c.a * 0.8, ..c })
            .draw();

        // bottom bar
        let h = 0.16;
        let r = Rect::new(-1., ui.top - h, 1.7, h);
        ui.fill_rect(r, (Color::from_hex(0xff283593), (r.x, r.y), Color::default(), (r.right(), r.y)));

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

        if let Some(dl) = &self.downloading {
            ui.fill_rect(ui.screen_rect(), semi_black(0.6));
            ui.loading(0., -0.06, t, WHITE, (*dl.prog.lock().unwrap(), &mut self.loading_last));
            ui.text(dl.status.lock().unwrap().clone()).pos(0., 0.02).anchor(0.5, 0.).size(0.6).draw();
            let size = 0.7;
            let r = ui.text(tl!("dl-cancel")).pos(0., 0.12).anchor(0.5, 0.).size(size).measure().feather(0.02);
            self.cancel_download_btn.render_text(ui, r, t, 1., tl!("dl-cancel"), 0.6, true);
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
