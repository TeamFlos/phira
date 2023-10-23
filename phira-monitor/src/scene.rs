use crate::{cloud::download, dir, launch::launch_task, Config};
use anyhow::{Context, Result};
use chrono::{DateTime, Local, Utc};
use log::{debug, error, info, warn};
use macroquad::prelude::*;
use phira_mp_client::Client;
use phira_mp_common::{JudgeEvent, Message, RoomId, RoomState, TouchFrame, UserInfo};
use prpr::{
    core::{BadNote, Chart, ParticleEmitter, Resource, Tweenable, Vector},
    ext::{poll_future, semi_white, LocalTask, RectExt},
    info::ChartInfo,
    judge::{Judge, JudgeStatus},
    scene::{show_error, GameScene, Scene},
    task::Task,
    time::TimeManager,
    ui::Ui,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    fs::File,
    path::Path,
    sync::Arc,
};
use tokio::net::TcpStream;

const ASPECT_MIN: f32 = 3. / 2.;
const ASPECT_MAX: f32 = 9. / 5.;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartEntity {
    pub id: i32,
    pub name: String,
    pub file: String,
    pub chart_updated: DateTime<Utc>,
    pub updated: DateTime<Utc>,
    pub created: DateTime<Utc>,
    pub uploader: i32,
}

async fn fetch_chart(id: i32) -> Result<ChartEntity> {
    Ok(reqwest::get(format!("https://api.phira.cn/chart/{id}"))
        .await?
        .error_for_status()?
        .json()
        .await?)
}

pub struct PlayerView {
    id: i32,
    name: String,
    chart: Chart,
    judge: Judge,
    emitter: ParticleEmitter,
    last_update_time: f64,
    touch_points: Vec<(f32, f32)>,
    bad_notes: Vec<BadNote>,

    touches: VecDeque<TouchFrame>,
    judges: VecDeque<JudgeEvent>,

    current_touches: HashMap<i8, Vec2>,
    current_time: f32,

    latest_time: Option<f32>,
}

impl PlayerView {
    pub fn new(info: UserInfo, chart: Chart, emitter: ParticleEmitter) -> Self {
        let judge = Judge::new(&chart);
        Self {
            id: info.id,
            name: info.name,
            chart,
            judge,
            emitter,
            last_update_time: 0.,
            touch_points: Vec::new(),
            bad_notes: Vec::new(),

            touches: VecDeque::new(),
            judges: VecDeque::new(),

            current_touches: HashMap::new(),
            current_time: 0.,

            latest_time: None,
        }
    }

    pub fn update(&mut self, client: &Client) {
        let player = client.live_player(self.id);

        let mut guard = player.touch_frames.blocking_lock();
        if guard.len() != 0 {
            debug!("received {} touch frames from {}", guard.len(), self.id);
        }
        self.touches.extend(guard.drain(..));
        drop(guard);

        if let Some(back) = self.touches.back() {
            self.latest_time = Some(back.time);
        }

        let mut guard = player.judge_events.blocking_lock();
        if guard.len() != 0 {
            debug!("received {} judge events from {}", guard.len(), self.id);
        }
        self.judges.extend(guard.drain(..));
        drop(guard);
    }

    fn update_with_res(&mut self, res: &mut Resource) {
        let t = res.time;

        let mut updated = false;
        while self.touches.front().map_or(false, |it| it.time < t) {
            let Some(frame) = self.touches.pop_front() else { unreachable!() };
            for (id, pos) in frame.points {
                if id >= 0 {
                    self.current_touches.insert(id, Vec2::new(pos.x(), pos.y()));
                } else {
                    self.current_touches.remove(&!id);
                }
            }
            self.current_time = frame.time;
            updated = true;
        }
        if updated {
            self.touch_points.clear();
            if let Some(frame) = self.touches.front() {
                let mut current = self.current_touches.clone();
                self.touch_points.extend(frame.points.iter().filter_map(|(id, pos)| {
                    let pos = vec2(pos.x(), pos.y());
                    let id = if *id >= 0 { *id } else { !*id };
                    let pos = if let Some(old) = current.remove(&id) {
                        Vec2::tween(&old, &pos, (t - self.current_time) / (frame.time - self.current_time))
                    } else {
                        return None;
                    };
                    Some((pos.x, pos.y / res.aspect_ratio))
                }));
                self.touch_points.extend(current.into_values().map(|it| (it.x, it.y)));
            }
        }

        std::mem::swap(&mut self.emitter, &mut res.emitter);
        self.chart.update(res);

        while let Some(event) = self.judges.front() {
            if event.time > t {
                break;
            }
            let Some(event) = self.judges.pop_front() else { unreachable!() };
            use phira_mp_common::Judgement::*;
            use prpr::judge::Judgement as TJ;
            let kind = match event.judgement {
                Perfect => Ok(TJ::Perfect),
                Good => Ok(TJ::Good),
                Bad => Ok(TJ::Bad),
                Miss => Ok(TJ::Miss),
                HoldPerfect => Err(true),
                HoldGood => Err(false),
            };
            let note = &mut self.chart.lines[event.line_id as usize].notes[event.note_id as usize];
            match kind {
                Ok(tj) => {
                    note.judge = JudgeStatus::Judged;
                    let line = &self.chart.lines[event.line_id as usize];
                    let line_tr = line.now_transform(res, &self.chart.lines);
                    let note = &line.notes[event.note_id as usize];
                    self.judge.commit(t, tj, event.line_id, event.note_id, 0.);
                    match tj {
                        TJ::Perfect => {
                            res.with_model(line_tr * note.object.now(res), |res| {
                                res.emit_at_origin(note.rotation(line), res.res_pack.info.fx_perfect())
                            });
                        }
                        TJ::Good => {
                            res.with_model(line_tr * note.object.now(res), |res| {
                                res.emit_at_origin(note.rotation(line), res.res_pack.info.fx_good())
                            });
                        }
                        TJ::Bad => {
                            self.bad_notes.push(BadNote {
                                time: t,
                                kind: note.kind.clone(),
                                matrix: {
                                    let mut mat = line_tr;
                                    if !note.above {
                                        mat.append_nonuniform_scaling_mut(&Vector::new(1., -1.));
                                    }
                                    let incline_sin = line.incline.now_opt().map(|it| it.to_radians().sin()).unwrap_or_default();
                                    mat *= note.now_transform(
                                        res,
                                        &line.ctrl_obj.borrow_mut(),
                                        (note.height - line.height.now()) / res.aspect_ratio * note.speed,
                                        incline_sin,
                                    );
                                    mat
                                },
                            });
                        }
                        _ => {}
                    }
                }
                Err(perfect) => {
                    note.judge = JudgeStatus::Hold(perfect, t, 0., false, f32::INFINITY);
                }
            }
        }

        std::mem::swap(&mut self.emitter, &mut res.emitter);
    }

    fn swap(&mut self, scene: &mut GameScene) {
        use std::mem::swap;
        swap(&mut self.chart, &mut scene.chart);
        swap(&mut self.judge, &mut scene.judge);
        swap(&mut self.emitter, &mut scene.res.emitter);
        swap(&mut self.last_update_time, &mut scene.last_update_time);
        swap(&mut self.touch_points, &mut scene.touch_points);
        swap(&mut self.bad_notes, &mut scene.bad_notes);
    }

    pub fn render(&mut self, ui: &mut Ui, r: Rect, tm: &mut TimeManager, game_scene: Option<&mut GameScene>) -> Result<()> {
        if let Some(scene) = game_scene {
            self.update_with_res(&mut scene.res);
            let r = ui.rect_to_global(r);
            let vw = screen_width();
            let x = (r.x + 1.) / 2. * vw;
            let y = (r.y + ui.top) / 2. * vw;
            let w = r.w * vw / 2.;
            let h = r.h * vw / 2.;
            let mut ui = Ui::new(ui.text_painter, Some((x as _, (screen_height() - y - h) as _, w as _, h as _)));

            push_camera_state();
            self.swap(scene);
            scene.render(tm, &mut ui)?;
            self.swap(scene);
            pop_camera_state();

            unsafe { get_internal_gl() }.quad_gl.viewport(None);
        }

        ui.text(&self.name)
            .pos(r.right() - 0.013, r.bottom() - 0.016)
            .anchor(1., 1.)
            .size(0.7)
            .draw();

        Ok(())
    }
}

struct InitResult {
    client: Client,
    chart: Option<(i32, String)>,
    token: String,
}

fn create_init_task(config: Config, token: Option<String>) -> Task<Result<InitResult>> {
    Task::new(async move {
        #[derive(Serialize)]
        struct LoginP<'a> {
            email: &'a str,
            password: &'a str,
        }

        let token = if let Some(token) = token {
            token
        } else {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct LoginR {
                token: String,
            }
            info!("登录中…");
            let resp: LoginR = reqwest::Client::new()
                .post("https://api.phira.cn/login")
                .json(&LoginP {
                    email: &config.email,
                    password: &config.password,
                })
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            resp.token
        };

        info!("连接 & 鉴权中…");
        let client = Client::new(TcpStream::connect(&config.server).await.context("连接到服务器失败")?)
            .await
            .context("连接失败")?;
        client.authenticate(token.clone()).await?;

        info!("加入房间…");
        let room_id: RoomId = config.room_id.clone().try_into().context("房间 ID 不合法")?;
        if client.room_state().await.is_none() {
            client.join_room(room_id, true).await?;
        }

        let chart = if let RoomState::SelectChart(Some(id)) = client.room_state().await.unwrap() {
            Some((id, fetch_chart(id).await?.name))
        } else {
            None
        };

        info!("初始化完成");

        Ok(InitResult { client, chart, token })
    })
}

pub struct MainScene {
    config: Config,
    client: Option<Arc<Client>>,

    token: Option<String>,
    init_task: Option<Task<Result<InitResult>>>,
    messages: Vec<String>,

    scene_task: LocalTask<Result<(GameScene, Vec<PlayerView>)>>,

    selected_chart: Option<(i32, String)>,

    get_ready_task: Option<Task<Result<()>>>,

    game_scene: Option<GameScene>,
    tm: TimeManager,
    render_started: bool,

    players: Vec<PlayerView>,
    start_playing_time: f32,

    scores: HashMap<String, (u32, f32, bool)>,
    game_end: bool,
}

impl MainScene {
    pub async fn new(config: Config) -> Result<Self> {
        Ok(Self {
            config: config.clone(),
            client: None,

            token: None,
            init_task: Some(create_init_task(config, None)),
            messages: Vec::new(),

            scene_task: None,

            selected_chart: None,

            get_ready_task: None,

            game_scene: None,
            tm: TimeManager::default(),
            render_started: false,

            players: Vec::new(),
            start_playing_time: f32::NAN,

            scores: HashMap::new(),
            game_end: false,
        })
    }

    fn start_get_ready(&mut self) {
        let client = self.client.as_ref().map(Arc::clone).unwrap();
        let id = self.selected_chart.as_ref().unwrap().0;
        let token = self.token.clone().unwrap();
        self.render_started = false;
        self.game_scene = None;
        self.get_ready_task = Some(Task::new(async move {
            let entity = fetch_chart(id).await?;
            info!("谱面信息：{entity:?}");
            let path = format!("download/{id}");
            let info_path = format!("{}/{path}/info.yml", dir::charts()?);
            let should_download = if Path::new(&info_path).exists() {
                let local_info: ChartInfo = serde_yaml::from_reader(File::open(info_path)?)?;
                local_info
                    .updated
                    .map_or(entity.updated != entity.created, |local_updated| local_updated != entity.updated)
            } else {
                true
            };
            if should_download {
                let local_path = download(entity, token).await?;
                info!("已下载到 {local_path}");
            } else {
                info!("无需下载");
            }

            client.ready().await?;
            Ok(())
        }));
    }
}

impl Scene for MainScene {
    fn touch(&mut self, _tm: &mut TimeManager, _touch: &Touch) -> Result<bool> {
        if self.client.is_none() {
            return Ok(true);
        }
        Ok(false)
    }

    fn update(&mut self, tm: &mut TimeManager) -> Result<()> {
        let t = tm.now() as f32;

        if let Some(task) = &mut self.init_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        show_error(err.context("初始化失败"));
                    }
                    Ok(res) => {
                        self.client = Some(Arc::new(res.client));
                        self.selected_chart = res.chart;
                        self.token = Some(res.token);
                        // self.scene_task = launch_task(
                        //     self.selected_chart.as_ref().unwrap().0,
                        //     self.client.as_ref().map(Arc::clone).unwrap(),
                        //     self.client
                        //         .as_ref()
                        //         .unwrap()
                        //         .blocking_state()
                        //         .unwrap()
                        //         .users
                        //         .values()
                        //         .cloned()
                        //         .filter(|it| !it.monitor)
                        //         .collect(),
                        // )?;
                    }
                }
            }
        }

        if let Some(task) = &mut self.get_ready_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        warn!("下载谱面失败：{err:?}");
                    }
                    Ok(_) => {
                        self.scene_task = launch_task(
                            self.selected_chart.as_ref().unwrap().0,
                            self.client
                                .as_ref()
                                .unwrap()
                                .blocking_state()
                                .unwrap()
                                .users
                                .values()
                                .cloned()
                                .filter(|it| !it.monitor)
                                .collect(),
                        )?;
                    }
                }
                self.get_ready_task = None;
            }
        }

        if let Some(task) = &mut self.scene_task {
            if let Some(res) = poll_future(task.as_mut()) {
                match res {
                    Err(err) => {
                        error!("failed to load scene: {err:?}");
                    }
                    Ok((scene, players)) => {
                        self.game_scene = Some(scene);
                        self.players = players;
                        self.players.sort_by(|x, y| x.name.cmp(&y.name));
                    }
                }
                self.scene_task = None;
            }
        }

        if let Some(client) = &self.client {
            for msg in client.blocking_take_messages() {
                match msg {
                    Message::Chat { user, content, .. } => {
                        let user = client.user_name(user);
                        info!("[{user}] {content}");
                        self.messages.push(format!("[{}] [{user}] {content}", Local::now().format("%H:%M:%S")));
                    }
                    Message::SelectChart { id, name, .. } => {
                        self.selected_chart = Some((id, name));
                    }
                    Message::StartPlaying => {
                        self.start_playing_time = t;
                        self.scores.clear();
                        self.game_end = false;
                    }
                    Message::Played {
                        user,
                        score,
                        accuracy,
                        full_combo,
                    } => {
                        info!("{user} played: {score} {accuracy} {full_combo}");
                        self.scores.insert(client.user_name(user), (score as _, accuracy, full_combo));
                    }
                    Message::GameEnd => {
                        self.game_end = true;
                    }
                    _ => {
                        info!("{msg:?}");
                    }
                }
            }

            for player in &mut self.players {
                player.update(client);
            }

            if self.get_ready_task.is_none()
                && matches!(client.blocking_room_state().unwrap(), RoomState::WaitingForReady)
                && !client.blocking_is_ready().unwrap()
            {
                self.start_get_ready();
            }
        }

        if self.client.as_ref().map_or(false, |it| it.ping_fail_count() >= 2) && self.init_task.is_none() {
            warn!("lost connection, re-connecting…");
            self.init_task = Some(create_init_task(self.config.clone(), self.token.clone()));
        }

        if self.render_started {
            if let Some(scene) = &mut self.game_scene {
                scene.update(&mut self.tm)?;
            }
        }

        Ok(())
    }

    fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        set_camera(&ui.camera());
        let t = tm.now() as f32;

        ui.fill_rect(ui.screen_rect(), ui.background());

        let Some(client) = &self.client else {
            ui.full_loading_simple(t);
            return Ok(());
        };

        let width = 2.;

        /* let pad = 0.01;
        let r = Rect::new(width - 1., -ui.top, 2. - width, ui.top * 2.).feather(-pad);
        ui.fill_rect(r, semi_black(0.4));
        let r = r.feather(-pad);
        let mut pos = r.bottom();
        for (index, msg) in self.messages.iter().enumerate().rev() {
            if pos < r.y {
                self.messages.drain(..=index);
                break;
            }
            let r = ui.text(msg).pos(r.x, pos).anchor(0., 1.).multiline().max_width(r.w).size(0.34).draw();
            pos = r.y - pad;
        }*/

        let r = Rect::new(-1., -ui.top, width, ui.top * 2.);
        ui.fill_rect(r, semi_white(0.4));

        match client.blocking_room_state().unwrap() {
            RoomState::SelectChart(_) => {
                self.game_scene = None;
                let ct = r.center();
                let tr = ui.text("选曲中").pos(ct.x, ct.y).anchor(0.5, 0.5).no_baseline().size(1.6).draw();
                if let Some((id, name)) = &self.selected_chart {
                    ui.text(format!("{name} (#{id})"))
                        .pos(ct.x, tr.bottom() + 0.03)
                        .anchor(0.5, 0.)
                        .size(0.56)
                        .draw();
                }
                let r = ui.text("上一局成绩").pos(r.x + 0.01, r.y + 0.01).size(0.6).draw();
                ui.scope(|ui| {
                    let s = 0.5;
                    ui.dx(r.x + 0.06);
                    ui.dy(r.bottom() + 0.02);
                    let w = 0.24;
                    for (user, (score, accuracy, full_combo)) in self.scores.iter() {
                        let r = ui.text(user).max_width(w - 0.02).size(s).draw();
                        ui.text(format!("{score} ({:.2}%){}", accuracy * 100., if *full_combo { " 全连" } else { "" }))
                            .pos(w, 0.)
                            .size(s)
                            .draw();
                        ui.dy(r.h + 0.01);
                    }
                });
            }
            _ => {
                let (row_count, col_count) = if self.players.len() > 2 {
                    (2, (self.players.len() + 1) / 2)
                } else {
                    (1, self.players.len())
                };

                let r = Rect::new(r.x, r.y, r.w / col_count as f32, r.h / row_count as f32);
                let (w, h) = (r.w.min(r.h * ASPECT_MAX), r.h.min(r.w / ASPECT_MIN));
                let ct = r.center();
                let mut iter = self.players.iter_mut();
                for i in 0..row_count {
                    for j in 0..iter.len().min(col_count) {
                        let r = Rect::new(ct.x + j as f32 * r.w, ct.y + i as f32 * r.h, 0., 0.)
                            .nonuniform_feather(w / 2., h / 2.)
                            .feather(-0.01);
                        let player = iter.next().unwrap();
                        player.render(ui, r, &mut self.tm, if self.render_started { self.game_scene.as_mut() } else { None })?;
                    }
                }

                if let Some(scene) = &mut self.game_scene {
                    if !self.render_started
                        && !self.start_playing_time.is_nan()
                        && (self.players.iter().all(|it| it.latest_time.map_or(false, |it| it > 5.)) || self.start_playing_time + 10. < t)
                    {
                        self.start_playing_time = f32::NAN;
                        self.tm.speed = 1.;
                        self.tm.reset();
                        scene.enter(&mut self.tm, None)?;
                        self.render_started = true;
                        info!("Render start!");
                    }
                }
            }
        }

        Ok(())
    }
}
