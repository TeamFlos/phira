use crate::{
    client::{Chart, Ptr, UserManager},
    dir, get_data,
    mp::L10N_LOCAL,
    scene::{Downloading, SongScene, RECORD_ID},
};
use anyhow::{anyhow, Context, Result};
use inputbox::InputBox;
use macroquad::prelude::*;
use phira_mp_client::Client;
use phira_mp_common::{RoomId, RoomState};
use prpr::{
    config::Mods,
    core::{Smooth, Tweenable},
    ext::{poll_future, semi_black, semi_white, LocalTask, RectExt, SafeTexture},
    info::ChartInfo,
    scene::{request_input, return_input, show_error, show_message, take_input, GameMode, NextScene, mp_reset_result, mp_take_result},
    task::Task,
    time::TimeManager,
    ui::{DRectButton, DrawText},
    ui::{Scroll, Ui},
};
use smallvec::SmallVec;
use std::{
    fs::File,
    path::Path,
    sync::{atomic::Ordering, Arc},
};
use tracing::warn;

const ENTER_TRANSIT: f32 = 0.5;
const USER_LIST_TRANSIT: f32 = 0.4;
const WIDTH: f32 = 1.6;

const CHAT_ENABLED: bool = cfg!(feature = "chat");

fn screen_size() -> (u32, u32) {
    (screen_width() as u32, screen_height() as u32)
}

struct Message {
    content: String,
    y: f32,
    bottom: f32,
    color: Color,
}

impl Message {
    pub fn text<'a, 's, 'ui>(&'s self, ui: &'ui mut Ui<'a>, mw: f32) -> DrawText<'a, 's, 'ui> {
        ui.text(&self.content)
            .pos(0., self.y)
            .size(0.4)
            .color(self.color)
            .max_width(mw)
            .multiline()
    }
}

pub struct MPPanel {
    pub client: Option<Arc<Client>>,

    side_enter_time: f32,

    msg_scroll: Scroll,
    msgs: Vec<Message>,
    msgs_dirty_from: usize,
    last_screen_size: (u32, u32),

    connect_btn: DRectButton,
    connect_task: Option<Task<Result<Client>>>,

    create_room_btn: DRectButton,
    create_room_task: Option<Task<Result<()>>>,
    join_room_btn: DRectButton,
    join_room_task: Option<Task<Result<RoomState>>>,
    leave_room_btn: DRectButton,

    disconnect_btn: DRectButton,

    request_start_btn: DRectButton,
    lock_room_btn: DRectButton,
    cycle_room_btn: DRectButton,

    ready_btn: DRectButton,
    cancel_ready_btn: DRectButton,

    chat_text: String,
    chat_btn: DRectButton,
    chat_send_btn: DRectButton,
    chat_task: Option<Task<Result<()>>>,

    download_task: Option<Task<Result<Arc<Chart>>>>,
    downloading: Option<Downloading>,
    // true for request_start, false for ready
    download_next: bool,

    // LocalChart 本地谱面同步
    local_chart: Option<(String, String)>,   // 当前分享的本地谱面 (UUID id, name)
    serving: Option<Arc<crate::mp::serve::ChartServer>>, // 房主：本地下载服务器
    syncing: Option<Arc<crate::mp::serve::ChartSyncing>>, // 玩家：正在从房主同步谱面
    // 玩家收到下载指令但尚未点击"准备"时的待下载信息 (addr, port, chart_id, chart_name)
    pending_download: Option<(String, u16, String, String)>,
    local_chart_task: Option<Task<Result<()>>>,
    // 房主：已点击"开始游戏"，分享中（按钮显示"取消准备"）
    host_started: bool,
    // 玩家：已点击"准备"（按钮显示"取消准备"，直到进入游玩）
    local_ready: bool,
    // 玩家取消准备时，用于中止下载完成后的自动就绪
    local_download_cancel: Option<Arc<std::sync::atomic::AtomicBool>>,

    chart_id: Option<i32>,
    game_start_consumed: bool,
    need_upload: bool,
    entered: bool,

    next_scene: Option<NextScene>,

    task: Option<Task<Result<()>>>,

    scene_task: LocalTask<Result<NextScene>>,

    user_list_btn: DRectButton,
    user_list_p: Smooth<f32>,
    user_list_scroll: Scroll,
    icon_user: SafeTexture,
}

impl MPPanel {
    pub fn new(icon_user: SafeTexture) -> Self {
        Self {
            client: None,

            side_enter_time: f32::INFINITY,

            msg_scroll: Scroll::new(),
            msgs: Vec::new(),
            msgs_dirty_from: 0,
            last_screen_size: screen_size(),

            connect_btn: DRectButton::new(),
            connect_task: None,

            create_room_btn: DRectButton::new(),
            create_room_task: None,
            join_room_btn: DRectButton::new(),
            join_room_task: None,
            leave_room_btn: DRectButton::new(),

            disconnect_btn: DRectButton::new(),

            request_start_btn: DRectButton::new(),
            lock_room_btn: DRectButton::new(),
            cycle_room_btn: DRectButton::new(),

            ready_btn: DRectButton::new(),
            cancel_ready_btn: DRectButton::new(),

            chat_text: String::new(),
            chat_btn: DRectButton::new().with_delta(-0.002),
            chat_send_btn: DRectButton::new(),
            chat_task: None,

            download_task: None,
            downloading: None,
            download_next: false,

            local_chart: None,
            serving: None,
            syncing: None,
            pending_download: None,
            local_chart_task: None,
            host_started: false,
            local_ready: false,
            local_download_cancel: None,

            chart_id: None,
            game_start_consumed: false,
            need_upload: false,
            entered: false,

            next_scene: None,

            task: None,

            scene_task: None,

            user_list_btn: DRectButton::new(),
            user_list_p: Smooth::default(),
            user_list_scroll: Scroll::new(),
            icon_user,
        }
    }

    fn clone_client(&self) -> Arc<Client> {
        Arc::clone(self.client.as_ref().unwrap())
    }

    fn has_task(&self) -> bool {
        self.connect_task.is_some()
            || self.create_room_task.is_some()
            || self.chat_task.is_some()
            || self.download_task.is_some()
            || self.local_chart_task.is_some()
            || self.task.is_some()
            || self.scene_task.is_some()
    }

    fn connect(&mut self) {
        let Some(token) = get_data().tokens.as_ref().map(|it| it.0.clone()) else {
            show_message(mtl!("connect-must-login")).error();
            return;
        };
        let addr = get_data().config.mp_address.clone();
        self.connect_task = Some(Task::new(async move {
            let client = Client::from_address(&addr).await?;
            client
                .authenticate(token)
                .await
                .with_context(|| anyhow!(mtl!("connect-authenticate-failed")))?;
            Ok(client)
        }));
    }

    fn create_room(&mut self, id: RoomId) {
        let client = self.clone_client();
        self.create_room_task = Some(Task::new(async move {
            client.create_room(id).await?;
            Ok(())
        }));
    }

    pub fn select_chart(&mut self, id: i32) {
        let client = self.clone_client();
        if !client.blocking_is_host().unwrap() {
            show_message(mtl!("select-chart-host-only")).error();
            return;
        }
        if !matches!(client.blocking_room_state(), Some(RoomState::SelectChart(_) | RoomState::LocalChart)) {
            show_message(mtl!("select-chart-not-now")).error();
            return;
        }
        // 切换到在线谱面：清除之前选择的本地谱面分享状态
        self.local_chart = None;
        self.pending_download = None;
        self.syncing = None;
        self.local_download_cancel = None;
        self.host_started = false;
        self.local_ready = false;
        self.stop_serving();
        self.task = Some(Task::new(async move {
            client.select_online_chart(id).await.with_context(|| mtl!("select-chart-failed"))?;
            Ok(())
        }));
    }

    /// 判断当前服务器是否允许上传本地谱面（mp.tianstudio.top / mp.ratzen.top，忽略端口）
    fn server_allows_local_chart(&self) -> bool {
        const ALLOWED: &[&str] = &["mp.tianstudio.top", "mp.ratzen.top"];
        let addr = &get_data().config.mp_address;
        // 去掉 scheme（如 tcp:// 等）
        let addr = addr.rsplit_once("://").map(|(_, host)| host).unwrap_or(addr);
        // 若包含端口（最后一个 ':'），取 ':' 之前作为主机名
        let host = addr.rsplit_once(':').map(|(h, _)| h).unwrap_or(addr);
        let host = host.trim().trim_matches(|c| c == '[' || c == ']');
        ALLOWED.contains(&host)
    }

    /// 从谱面库中选择本地谱面（无在线 id）进行分享。
    /// 生成随机 UUID，把本地谱面目录复制到 `download/{uuid}`，并发送 `SelectLocalChart`。
    pub fn select_local_chart(&mut self, local_path: String, name: String) {
        // 仅允许在指定服务器上上传本地谱面
        if !self.server_allows_local_chart() {
            show_message(mtl!("local-chart-server-not-allowed")).error();
            return;
        }
        let client = self.clone_client();
        if !client.blocking_is_host().unwrap() {
            show_message(mtl!("select-chart-host-only")).error();
            return;
        }
        if !matches!(client.blocking_room_state(), Some(RoomState::SelectChart(_))) {
            show_message(mtl!("select-chart-not-now")).error();
            return;
        }
        self.task = Some(Task::new(async move {
            let uuid = uuid::Uuid::new_v4().to_string();
            // 把本地谱面包复制到 download/{uuid}（供 serve / download 使用）
            crate::mp::serve::stage_local_chart(&local_path, &uuid)?;
            client.select_local_chart(uuid, name).await.with_context(|| mtl!("select-chart-failed"))?;
            Ok(())
        }));
    }

    fn request_start(&mut self) {
        let client = self.clone_client();
        let state = client.blocking_room_state().unwrap();
        // LocalChart 状态下房主已选择本地谱面：直接请求开始（服务端会通知房主启动下载服务器）
        if matches!(state, RoomState::LocalChart) {
            self.host_started = true;
            self.task = Some(Task::new(async move {
                client.request_start().await.with_context(|| mtl!("request-start-failed"))?;
                Ok(())
            }));
            return;
        }
        if matches!(state, RoomState::SelectChart(None)) {
            show_message(mtl!("request-start-no-chart")).error();
            return;
        }
        self.check_download(true);
    }

    fn check_download(&mut self, next: bool) {
        let id = self.chart_id.unwrap();
        self.download_next = next;
        self.download_task = Some(Task::new(async move { Ptr::new(id).fetch().await }));
    }

    fn post_download(&mut self) {
        let client = self.clone_client();
        if self.download_next {
            self.task = Some(Task::new(async move {
                client.request_start().await.with_context(|| mtl!("request-start-failed"))?;
                Ok(())
            }));
        } else {
            self.task = Some(Task::new(async move {
                client.ready().await.with_context(|| mtl!("ready-failed"))?;
                Ok(())
            }));
        }
    }
}

impl MPPanel {
    #[inline]
    pub fn in_room(&self) -> bool {
        self.client.as_ref().is_some_and(|it| it.blocking_room_id().is_some())
    }

    #[inline]
    pub fn show(&mut self, rt: f32) {
        self.side_enter_time = rt;
    }

    pub fn enter(&mut self) {
        self.entered = true;
    }

    pub fn touch(&mut self, tm: &mut TimeManager, touch: &Touch) -> bool {
        let t = tm.now() as f32;
        if self.side_enter_time.is_infinite() {
            return false;
        }
        if self.user_list_p.transiting(t) {
            return true;
        }
        if *self.user_list_p.to() > 0.5 {
            if self.user_list_scroll.touch(touch, t) {
                return true;
            }
            if matches!(touch.phase, TouchPhase::Ended | TouchPhase::Cancelled) {
                self.user_list_p.goto(0., t, USER_LIST_TRANSIT);
            }
            return true;
        }
        if !(self.side_enter_time > 0. && tm.real_time() as f32 > self.side_enter_time + ENTER_TRANSIT) {
            return true;
        }
        if self.has_task() {
            return true;
        }
        if let Some(dl) = &mut self.downloading {
            if dl.touch(touch, t) {
                self.downloading = None;
                return true;
            }
        }
        if touch.position.x + 1. > WIDTH {
            self.side_enter_time = -tm.real_time() as f32;
            return true;
        }
        if self.client.is_none() && self.connect_btn.touch(touch, t) {
            self.connect();
            return true;
        }
        if let Some(client) = &self.client {
            if self.msg_scroll.touch(touch, t) {
                return true;
            }
            if let Some(state) = client.blocking_state() {
                if self.chat_btn.touch(touch, t) {
                    request_input("chat", InputBox::new().default_text(&self.chat_text));
                    return true;
                }
                if self.chat_send_btn.touch(touch, t) {
                    if self.chat_text.is_empty() {
                        show_message(mtl!("chat-empty")).error();
                    } else {
                        let client = Arc::clone(client);
                        let text = self.chat_text.clone();
                        self.chat_task = Some(Task::new(async move { client.chat(text).await }));
                    }
                    return true;
                }
                let is_host = state.is_host;
                match state.state {
                    RoomState::SelectChart(_) => {
                        if is_host {
                            if self.request_start_btn.touch(touch, t) {
                                self.request_start();
                                return true;
                            }
                            if self.lock_room_btn.touch(touch, t) {
                                let to = !state.locked;
                                let client = self.clone_client();
                                self.task = Some(Task::new(async move { client.lock_room(to).await.with_context(|| mtl!("lock-room-failed")) }));
                                return true;
                            }
                            if self.cycle_room_btn.touch(touch, t) {
                                let to = !state.cycle;
                                let client = self.clone_client();
                                self.task = Some(Task::new(async move { client.cycle_room(to).await.with_context(|| mtl!("cycle-room-failed")) }));
                                return true;
                            }
                        }
                        if self.leave_room_btn.touch(touch, t) {
                            let client = self.clone_client();
                            self.task = Some(Task::new(async move { client.leave_room().await }));
                            return true;
                        }
                    }
                    // 本地谱面分享状态：房主点开始（分享）、锁定、循环；玩家点"准备"后开始下载
                    RoomState::LocalChart => {
                        if is_host {
                            if self.host_started {
                                // 房主已点开始：按钮变为"取消准备"，点击取消分享
                                if self.cancel_ready_btn.touch(touch, t) {
                                    self.cancel_local_chart();
                                    return true;
                                }
                            } else if self.request_start_btn.touch(touch, t) {
                                self.request_start();
                                return true;
                            }
                            if self.lock_room_btn.touch(touch, t) {
                                let to = !state.locked;
                                let client = self.clone_client();
                                self.task = Some(Task::new(async move { client.lock_room(to).await.with_context(|| mtl!("lock-room-failed")) }));
                                return true;
                            }
                            if self.cycle_room_btn.touch(touch, t) {
                                let to = !state.cycle;
                                let client = self.clone_client();
                                self.task = Some(Task::new(async move { client.cycle_room(to).await.with_context(|| mtl!("cycle-room-failed")) }));
                                return true;
                            }
                        } else {
                            // 玩家：点"准备"后开始下载；已准备则按钮变为"取消准备"
                            if self.local_ready {
                                if self.cancel_ready_btn.touch(touch, t) {
                                    self.cancel_local_download();
                                    return true;
                                }
                            } else if self.syncing.is_none() && self.ready_btn.touch(touch, t) {
                                self.local_ready = true;
                                self.start_pending_download();
                                return true;
                            }
                        }
                        if self.leave_room_btn.touch(touch, t) {
                            let client = self.clone_client();
                            self.task = Some(Task::new(async move { client.leave_room().await }));
                            return true;
                        }
                    }
                    RoomState::WaitingForReady => {
                        if client.blocking_is_ready().unwrap() {
                            if self.cancel_ready_btn.touch(touch, t) {
                                let client = self.clone_client();
                                self.task = Some(Task::new(async move { client.cancel_ready().await }));
                                return true;
                            }
                        } else if self.ready_btn.touch(touch, t) {
                            self.check_download(false);
                            return true;
                        }
                    }
                    _ => {}
                }
                if self.user_list_btn.touch(touch, t) {
                    self.user_list_scroll.y_scroller.reset();
                    self.user_list_p.goto(1., t, USER_LIST_TRANSIT);
                    client.blocking_state().unwrap().users.keys().copied().for_each(UserManager::request);
                }
            } else {
                if self.create_room_btn.touch(touch, t) {
                    request_input("room_id", InputBox::new());
                    return true;
                }
                if self.join_room_btn.touch(touch, t) {
                    request_input("join_room", InputBox::new());
                    return true;
                }
                if self.disconnect_btn.touch(touch, t) {
                    self.client = None;
                    self.msgs.clear();
                    self.msgs_dirty_from = 0;
                    return true;
                }
            }
            if client.ping_fail_count() >= 2 && self.connect_task.is_none() {
                // 本地谱面传输期间（上传/下载）心跳可能因大帧传输短暂超时，
                // 此时不要自动重连，避免中断正在进行的谱面传输。
                if self.serving.is_some() || self.syncing.is_some() {
                    // 仍在传输本地谱面，跳过自动重连
                } else {
                    warn!("lost connection, reconnecting…");
                    show_message(mtl!("reconnect")).warn();
                    self.connect();
                }
            }
        }
        true
    }

    pub fn update(&mut self, tm: &mut TimeManager) -> Result<()> {
        let t = tm.now() as f32;
        if self.side_enter_time < 0. && -tm.real_time() as f32 + ENTER_TRANSIT < self.side_enter_time {
            self.side_enter_time = f32::INFINITY;
        }
        let new_size = screen_size();
        if self.last_screen_size != new_size {
            self.last_screen_size = new_size;
            self.msgs_dirty_from = 0;
        }
        self.msg_scroll.update(t);
        if self.user_list_p.now(t) > 1e-4 {
            self.user_list_scroll.update(t);
        }
        if let Some(client) = &self.client {
            self.msgs.extend(client.blocking_take_messages().into_iter().map(|msg| {
                use phira_mp_common::Message as M;
                match msg {
                    M::Chat { user, content, .. } => Message {
                        content: format!("{}：{content}", client.user_name(user)),
                        y: 0.,
                        bottom: 0.,
                        color: WHITE,
                    },
                    msg => {
                        let content = match msg {
                            M::Chat { .. } => unreachable!(),
                            M::CreateRoom { user } => {
                                mtl!("msg-create-room", "user" => client.user_name(user))
                            }
                            M::JoinRoom { name, .. } => {
                                mtl!("msg-join-room", "user" => name)
                            }
                            M::LeaveRoom { name, .. } => {
                                mtl!("msg-leave-room", "user" => name)
                            }
                            M::NewHost { user } => {
                                mtl!("msg-new-host", "user" => client.user_name(user))
                            }
                            M::SelectChart { user, name, id } => {
                                mtl!("msg-select-chart", "user" => client.user_name(user), "chart" => name, "id" => id)
                            }
                            M::GameStart { user } => {
                                mtl!("msg-game-start", "user" => client.user_name(user))
                            }
                            M::Ready { user } => {
                                mtl!("msg-ready", "user" => client.user_name(user))
                            }
                            M::CancelReady { user } => {
                                mtl!("msg-cancel-ready", "user" => client.user_name(user))
                            }
                            M::CancelGame { user } => {
                                mtl!("msg-cancel-game", "user" => client.user_name(user))
                            }
                            M::StartPlaying => mtl!("msg-start-playing").into_owned(),
                            M::Played { user, score, accuracy, full_combo } => {
                                mtl!("msg-played", "user" => client.user_name(user), "score" => format!("{score:07}"), "accuracy" => format!("{:.2}%", accuracy * 100.), "full-combo" => full_combo.to_string())
                            }
                            M::GameEnd => mtl!("msg-game-end").into_owned(),
                            M::Abort { user } => mtl!("msg-abort", "user" => client.user_name(user)),
                            M::LockRoom { lock } => mtl!("msg-room-lock", "lock" => lock.to_string()),
                            M::CycleRoom { cycle } => mtl!("msg-room-cycle", "cycle" => cycle.to_string()),
                            M::SelectLocalChart { user, name, id } => {
                                mtl!("msg-select-local-chart", "user" => client.user_name(user), "chart" => name, "id" => id)
                            }
                            M::SendChart { user } => {
                                mtl!("msg-send-chart", "user" => client.user_name(user))
                            }
                            M::DownloadReady { user } => {
                                mtl!("msg-download-ready", "user" => client.user_name(user))
                            }
                        };
                        Message {
                            content,
                            y: 0.,
                            bottom: 0.,
                            color: semi_white(0.7),
                        }
                    }
                }
            }));
            let state = client.blocking_room_state();
            if matches!(state, Some(RoomState::Playing)) {
                if !self.game_start_consumed {
                    self.game_start_consumed = true;
                    RECORD_ID.store(-1, Ordering::Relaxed);
                    mp_reset_result();
                    self.need_upload = true;
                    self.entered = false;
                    // 本地谱面分享：从本地 download/{uuid} 加载（无在线 id）
                    if let Some((uuid, _)) = self.local_chart.clone() {
                        self.scene_task = SongScene::global_launch(
                            None,
                            &format!("download/{uuid}"),
                            Mods::default(),
                            GameMode::NoRetry,
                            self.client.as_ref().map(Arc::clone),
                            None,
                            None,
                            false,
                        )?;
                    } else {
                        let id = self.chart_id.unwrap();
                        self.scene_task = SongScene::global_launch(
                            Some(id),
                            &format!("download/{id}"),
                            Mods::default(),
                            GameMode::NoRetry,
                            self.client.as_ref().map(Arc::clone),
                            None,
                            None,
                            false,
                        )?;
                    }
                }
            } else {
                self.game_start_consumed = false;
            }
            if let Some(RoomState::SelectChart(chart)) = state {
                self.chart_id = chart;
            }
            if matches!(state, Some(RoomState::LocalChart)) {
                // 本地谱面分享状态；本地谱面 id 为 UUID，用于 download/{uuid} 加载
                if self.local_chart.is_some() {
                    self.chart_id = None; // 本地谱面无在线 id，置空在线 chart_id
                }
            } else {
                // 离开本地谱面分享阶段（进入游玩/选谱等）：重置本地谱面相关状态，
                // 避免本地谱面游玩结束后残留状态导致切换到在线谱面时卡在转圈/无法开始。
                self.host_started = false;
                self.local_ready = false;
                self.local_download_cancel = None;
                self.local_chart = None;
                self.pending_download = None;
                self.syncing = None;
                self.stop_serving();
            }
        }
        // 处理服务端下发的 LocalChart 本地谱面同步事件
        self.update_local_chart();
        if let Some(task) = &mut self.connect_task {
            if let Some(res) = task.take() {
                match res {
                    Ok(client) => {
                        show_message(mtl!("connect-success")).ok();
                        self.client = Some(client.into());
                    }
                    Err(err) => {
                        show_error(err.context(mtl!("connect-failed")));
                    }
                }
                self.connect_task = None;
            }
        }
        if let Some(task) = &mut self.create_room_task {
            if let Some(res) = task.take() {
                match res {
                    Ok(_) => {
                        show_message(mtl!("create-room-success")).ok();
                    }
                    Err(err) => {
                        show_error(err.context(mtl!("create-room-failed")));
                    }
                }
                self.create_room_task = None;
            }
        }
        if let Some(task) = &mut self.download_task {
            if let Some(res) = task.take() {
                match res {
                    Ok(entity) => {
                        let path = format!("download/{}", entity.id);
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
                            let info = entity.to_info();
                            self.downloading = Some(SongScene::global_start_download(info, Chart::clone(&entity), {
                                if Path::new(&format!("{}/{path}", dir::charts()?)).exists() {
                                    Some(path)
                                } else {
                                    None
                                }
                            })?);
                        } else {
                            self.post_download();
                        }
                    }
                    Err(err) => {
                        show_error(err.context(mtl!("download-failed")));
                    }
                }
                self.download_task = None;
            }
        }
        if let Some(dl) = &mut self.downloading {
            if let Some(res) = dl.check()? {
                if res.is_some() {
                    self.post_download();
                }
                self.downloading = None;
            }
        }
        if let Some(task) = &mut self.chat_task {
            if let Some(res) = task.take() {
                match res {
                    Ok(_) => {
                        show_message(mtl!("chat-sent")).ok();
                        self.chat_text.clear();
                    }
                    Err(err) => {
                        show_error(err.context(mtl!("chat-send-failed")));
                    }
                }
                self.chat_task = None;
            }
        }
        if let Some(task) = &mut self.task {
            if let Some(res) = task.take() {
                if let Err(err) = res {
                    show_error(err);
                }
                self.task = None;
            }
        }
        if let Some(task) = &mut self.join_room_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        show_error(err.context(mtl!("join-room-failed")));
                    }
                    Ok(state) => {
                        self.chart_id = match state {
                            RoomState::SelectChart(id) => id,
                            _ => None,
                        };
                    }
                }
                self.task = None;
            }
        }
        if let Some((id, text)) = take_input() {
            match id.as_str() {
                "chat" => {
                    self.chat_text = text;
                }
                "room_id" => {
                    self.create_room(text.try_into().with_context(|| mtl!("create-invalid-id"))?);
                }
                "join_room" => {
                    let client = self.clone_client();
                    if let Ok(id) = text.try_into() {
                        self.join_room_task = Some(Task::new(async move {
                            client.join_room(id, false).await?;
                            client.room_state().await.ok_or_else(|| anyhow!("expected room state"))
                        }));
                    } else {
                        show_message(mtl!("join-room-invalid-id")).error();
                    }
                }
                _ => return_input(id, text),
            }
        }
        if let Some(task) = &mut self.scene_task {
            if let Some(res) = poll_future(task.as_mut()) {
                match res {
                    Err(err) => {
                        show_error(err);
                    }
                    Ok(scene) => self.next_scene = Some(scene),
                }
                self.scene_task = None;
            }
        }
        if self.need_upload && self.entered {
            // 本地谱面（无官方 record id）：成绩直传；在线谱面：走原始渠道（上传官方 record id）
            let is_local_chart = self.local_chart.is_some();
            // 单人房间中游玩期间一旦连接断开并重连，服务端可能已在 Playing 状态清理掉房间，
            // 此时再上报成绩会得到 "no room" 报错，因此若已不在房间则跳过成绩上报。
            let in_room = self
                .client
                .as_ref()
                .is_some_and(|it| it.blocking_state().is_some());
            if in_room {
                let client = self.clone_client();
                let id = RECORD_ID.load(Ordering::Relaxed);
                self.task = Some(Task::new(async move {
                    if is_local_chart {
                        // 本地谱面：成绩直传（客户端上报真实成绩，不依赖官方 record id）
                        if let Some(r) = mp_take_result() {
                            client
                                .played_with_score(id, r.score, r.accuracy, r.full_combo, r.max_combo, r.perfect, r.good, r.bad, r.miss)
                                .await
                        } else {
                            client.abort().await
                        }
                    } else {
                        // 在线谱面：走原始渠道（上传官方 record id，服务端回源）
                        if id != -1 {
                            client.played(id).await
                        } else {
                            client.abort().await
                        }
                    }
                }));
            }
            self.need_upload = false;
        }
        self.poll_local_chart_task();
        Ok(())
    }

    // 本地谱面同步任务完成：成功则隐藏"正在同步谱面"转圈
    fn poll_local_chart_task(&mut self) {
        if let Some(task) = &mut self.local_chart_task {
            if let Some(res) = task.take() {
                match res {
                    Ok(()) => {
                        // 房主：上传完成即停止下载服务器并隐藏转圈
                        // 玩家：下载完成即隐藏转圈（玩家已在 task 内通知服务端就绪）
                        if self.serving.is_some() {
                            self.stop_serving();
                        }
                        self.syncing = None;
                    }
                    Err(err) => {
                        // 上传/下载失败：隐藏"正在同步谱面"转圈，并重置按钮状态，方便用户重试
                        if self.serving.is_some() {
                            self.stop_serving();
                        }
                        if let Some(syncing) = &self.syncing {
                            syncing.set_error(err.to_string());
                        }
                        self.syncing = None;
                        self.host_started = false;
                        self.local_ready = false;
                        self.local_download_cancel = None;
                        show_error(err);
                    }
                }
                self.local_chart_task = None;
            }
        }
        if let Some(syncing) = &self.syncing {
            if let Some(err) = syncing.error() {
                self.syncing = None;
                show_message(mtl!("local-chart-sync-failed", "err" => err)).error();
            }
        }
    }

    /// 消费服务端下发的 LocalChart 事件，驱动本地谱面分享流程。
    fn update_local_chart(&mut self) {
        let Some(client) = self.client.clone() else { return };
        let is_host = client.blocking_is_host().unwrap_or(false);
        let events = client.blocking_take_local_chart_events();
        for ev in events {
            match ev {
                phira_mp_client::LocalChartEvent::ChangeLocalChart { local, chart_id } => {
                    if local {
                        self.local_chart = Some((chart_id, String::new()));
                    } else {
                        self.local_chart = None;
                        self.pending_download = None;
                        self.syncing = None;
                        self.stop_serving();
                    }
                    // 重置就绪/开始按钮状态
                    self.host_started = false;
                    self.local_ready = false;
                    self.local_download_cancel = None;
                }
                phira_mp_client::LocalChartEvent::StartServing { chart_id, chart_name } => {
                    if !is_host {
                        continue;
                    }
                    self.local_chart = Some((chart_id.clone(), chart_name));
                    self.start_serving(chart_id);
                }
                phira_mp_client::LocalChartEvent::StartDownload {
                    host_id: _,
                    host_name: _,
                    addr,
                    port,
                    chart_id,
                    chart_name,
                } => {
                    if is_host {
                        continue;
                    }
                    // 玩家：建立连接后不立即下载，先保存下载信息，等点击"准备"后才开始下载
                    self.local_chart = Some((chart_id.clone(), chart_name.clone()));
                    self.pending_download = Some((addr, port, chart_id, chart_name));
                }
                phira_mp_client::LocalChartEvent::HostReady => {
                    if is_host {
                        self.stop_serving();
                    }
                    self.host_started = false;
                    self.local_ready = false;
                    self.local_download_cancel = None;
                }
                phira_mp_client::LocalChartEvent::Canceled => {
                    // 房主取消了分享：重置所有就绪/开始按钮状态，仍停留在分享阶段
                    self.host_started = false;
                    self.local_ready = false;
                    self.pending_download = None;
                    self.syncing = None;
                    self.local_download_cancel = None;
                    self.stop_serving();
                }
            }
        }
    }

    fn start_serving(&mut self, chart_id: String) {
        match crate::mp::serve::ChartServer::start(chart_id.clone()) {
            Ok(server) => {
                self.serving = Some(Arc::clone(&server));
                let client = self.clone_client();
                let server = Arc::clone(&server);
                self.local_chart_task = Some(Task::new(async move {
                    // 短暂等待服务器监听就绪
                    for _ in 0..50 {
                        if server.ready() {
                            break;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    }
                    // 把本地谱面包经 game 连接上传到服务端（服务端打洞中转），玩家从服务端下载
                    crate::mp::serve::upload_chart(&client, &chart_id).await?;
                    // 通知服务端开始分享；玩家下载地址由服务端下发
                    client.send_chart(String::new(), 0).await?;
                    Ok::<_, anyhow::Error>(())
                }));
            }
            Err(err) => {
                show_error(err);
            }
        }
    }

    fn stop_serving(&mut self) {
        if let Some(server) = self.serving.take() {
            server.stop();
        }
    }

    /// 玩家点击"准备"后开始经服务端下载谱面；下载完成后发送就绪指令。
    /// 若没有待下载信息（异常情况），直接发送就绪。
    fn start_pending_download(&mut self) {
        let Some((_addr, _port, chart_id, _chart_name)) = self.pending_download.take() else {
            // 没有待下载信息：直接通知服务端就绪
            let client = self.clone_client();
            self.local_chart_task = Some(Task::new(async move {
                client.download_ready().await?;
                Ok::<_, anyhow::Error>(())
            }));
            return;
        };
        // 显示"正在同步谱面"转圈并经 game 连接从服务端下载
        let syncing = Arc::new(crate::mp::serve::ChartSyncing::new());
        syncing.mark_started();
        self.syncing = Some(Arc::clone(&syncing));
        let client = self.clone_client();
        // 取消标记：玩家取消准备后，下载完成不再自动发送就绪
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cancel_task = Arc::clone(&cancel);
        self.local_download_cancel = Some(cancel);
        self.local_chart_task = Some(Task::new(async move {
            crate::mp::serve::download_chart(&client, &chart_id, Arc::clone(&syncing)).await?;
            if !cancel_task.load(std::sync::atomic::Ordering::Relaxed) {
                client.download_ready().await?;
            }
            Ok::<_, anyhow::Error>(())
        }));
    }

    /// 房主取消本地谱面分享：删除服务端缓存、重置所有玩家就绪状态
    fn cancel_local_chart(&mut self) {
        self.host_started = false;
        self.local_ready = false;
        self.pending_download = None;
        self.syncing = None;
        if let Some(cancel) = self.local_download_cancel.take() {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.stop_serving();
        let client = self.clone_client();
        self.task = Some(Task::new(async move {
            client.cancel_local_chart().await?;
            Ok(())
        }));
    }

    /// 玩家取消已就绪（尚未开始游玩前可取消）
    fn cancel_local_download(&mut self) {
        self.local_ready = false;
        self.syncing = None;
        if let Some(cancel) = self.local_download_cancel.take() {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        let client = self.clone_client();
        self.task = Some(Task::new(async move {
            client.cancel_download_ready().await?;
            Ok(())
        }));
    }

    pub fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) {
        let rt = tm.real_time() as f32;
        let t = tm.now() as f32;
        if self.side_enter_time.is_finite() {
            let p = ((rt - self.side_enter_time.abs()) / ENTER_TRANSIT).min(1.);
            let p = 1. - (1. - p).powi(3);
            let p = if self.side_enter_time < 0. { 1. - p } else { p };
            ui.fill_rect(ui.screen_rect(), semi_black(p * 0.6));
            let w = WIDTH;
            let rt = f32::tween(&-1., &(w - 1.), p);
            ui.scope(|ui| {
                ui.dx(rt - w);
                ui.dy(-ui.top);
                let h = ui.top * 2.;
                let r = Rect::new(0., 0., w, h).feather(-0.02);
                ui.fill_path(&r.rounded(0.02), ui.background());
                if let Some(id) = self.client.as_ref().and_then(|it| it.blocking_room_id()) {
                    ui.text(mtl!("room-id", "id" => id.to_string()))
                        .pos(r.right() - 0.02, r.y + 0.02)
                        .anchor(1., 0.)
                        .size(0.44)
                        .color(semi_white(0.4))
                        .draw();
                }
                let tr = ui.text(mtl!("multiplayer")).pos(0.05, 0.05).draw();
                let r = Rect::new(r.x, tr.bottom(), r.w, r.bottom() - tr.bottom()).feather(-0.02);
                if self.client.is_none() {
                    let ct = r.center();
                    self.connect_btn
                        .render_text(ui, Rect::new(ct.x, ct.y, 0., 0.).nonuniform_feather(0.14, 0.06), t, mtl!("connect"), 0.5, true);
                } else {
                    self.render_main(tm, ui, r);
                }
            });
        }
        if let Some(dl) = &mut self.downloading {
            dl.render(ui, t);
        }
        if self.has_task() {
            ui.full_loading_simple(t);
        }
        if self.serving.is_some() || self.syncing.is_some() {
            ui.full_loading(mtl!("local-chart-syncing"), t);
        }
    }

    fn render_main(&mut self, tm: &mut TimeManager, ui: &mut Ui, r: Rect) {
        let t = tm.now() as f32;
        let client = self.client.as_ref().unwrap();
        let mr = Rect::new(r.x, r.y, r.w * 0.8, r.h - if CHAT_ENABLED { 0.11 } else { 0. });
        ui.fill_path(&mr.rounded(0.01), semi_black(0.4));
        ui.scope(|ui| {
            let mut mr = mr.feather(-0.015);
            mr.y -= 0.015;
            mr.h += 0.015;
            ui.dx(mr.x);
            ui.dy(mr.y);
            let mut y = if self.msgs_dirty_from == 0 {
                0.
            } else {
                self.msgs.get(self.msgs_dirty_from - 1).map_or(0., |it| it.bottom)
            };
            let old_dirty = self.msgs_dirty_from != self.msgs.len();
            for msg in &mut self.msgs[self.msgs_dirty_from..] {
                msg.y = y + 0.02;
                msg.bottom = msg.text(ui, mr.w).measure().bottom();
                y = msg.bottom;
            }
            if old_dirty {
                let o = y - mr.h;
                if o >= 0. {
                    self.msg_scroll.y_scroller.goto = Some(o);
                }
            }
            self.msgs_dirty_from = self.msgs.len();
            self.msg_scroll.size((mr.w, mr.h));
            let offset = self.msg_scroll.y_scroller.offset;
            self.msg_scroll.render(ui, |ui| {
                for msg in &self.msgs {
                    if msg.bottom < offset {
                        continue;
                    }
                    if msg.y > offset + mr.h {
                        break;
                    }
                    msg.text(ui, mr.w).draw();
                }
                (mr.w, self.msgs.last().map(|it| it.bottom).unwrap_or_default() + 0.03)
            });
        });

        if CHAT_ENABLED {
            let lw = 0.16;
            let h = 0.09;
            let br = Rect::new(r.x, r.bottom() - h, mr.w - lw - 0.02, h);
            self.chat_btn.render_input(ui, br, t, &self.chat_text, mtl!("chat-placeholder"), 0.5);
            let br = Rect::new(mr.right() - lw, br.y, lw, br.h);
            self.chat_send_btn.render_text(ui, br, t, mtl!("chat-send"), 0.5, true);
        }

        let mut br = Rect::new(mr.right() + 0.02, mr.y, r.right() - mr.right() - 0.02, 0.1);
        let mut btns = SmallVec::<[(&mut DRectButton, String); 5]>::new();
        if let Some(state) = client.blocking_state() {
            match state.state {
                RoomState::SelectChart(_) => {
                    if client.blocking_is_host().unwrap() {
                        btns.push((&mut self.request_start_btn, mtl!("request-start").into_owned()));
                        btns.push((&mut self.lock_room_btn, mtl!("lock-room", "current" => state.locked.to_string())));
                        btns.push((&mut self.cycle_room_btn, mtl!("cycle-room", "current" => state.cycle.to_string())));
                    }
                    btns.push((&mut self.leave_room_btn, mtl!("leave-room").into_owned()));
                }
                RoomState::LocalChart => {
                    if client.blocking_is_host().unwrap() {
                        if self.host_started {
                            // 房主已点开始：开始按钮变为"取消准备"
                            btns.push((&mut self.cancel_ready_btn, mtl!("cancel-ready").into_owned()));
                        } else {
                            btns.push((&mut self.request_start_btn, mtl!("request-start").into_owned()));
                        }
                        btns.push((&mut self.lock_room_btn, mtl!("lock-room", "current" => state.locked.to_string())));
                        btns.push((&mut self.cycle_room_btn, mtl!("cycle-room", "current" => state.cycle.to_string())));
                    } else if self.local_ready {
                        // 玩家已准备：按钮变为"取消准备"，直到进入游玩才隐藏
                        btns.push((&mut self.cancel_ready_btn, mtl!("cancel-ready").into_owned()));
                    } else if self.pending_download.is_some() {
                        // 玩家：房主上传完成后，等待点击"准备"开始下载
                        btns.push((&mut self.ready_btn, mtl!("ready").into_owned()));
                    }
                    btns.push((&mut self.leave_room_btn, mtl!("leave-room").into_owned()));
                }
                RoomState::WaitingForReady => {
                    if client.blocking_is_ready().unwrap() {
                        btns.push((&mut self.cancel_ready_btn, mtl!("cancel-ready").into_owned()));
                    } else {
                        btns.push((&mut self.ready_btn, mtl!("ready").into_owned()));
                    }
                }
                _ => {}
            }
            btns.push((&mut self.user_list_btn, mtl!("user-list").into_owned()));
        } else {
            btns.push((&mut self.create_room_btn, mtl!("create-room").into_owned()));
            btns.push((&mut self.join_room_btn, mtl!("join-room").into_owned()));
            btns.push((&mut self.disconnect_btn, mtl!("disconnect").into_owned()));
        }
        for (btn, text) in btns {
            btn.render_text(ui, br, t, text, 0.5, true);
            br.y += br.h + 0.02;
        }

        let p = self.user_list_p.now(t);
        if p > 1e-4 {
            ui.abs_scope(|ui| {
                ui.alpha(p, |ui| {
                    let users: Vec<_> = client.blocking_state().unwrap().users.values().cloned().collect();
                    let n = users.len();
                    let columns = n.clamp(2, 4);
                    let rn = n.div_ceil(columns);
                    ui.fill_rect(ui.screen_rect(), semi_black(p * 0.4));

                    let mut iter = users.into_iter();
                    let h = 0.14;
                    let w = 0.48;
                    let pad = 0.03;
                    let width = w * columns as f32 + pad * (columns - 1) as f32;
                    let viewport_height = (ui.top * 2. - 0.16).max(h);
                    ui.dx(-width / 2.);
                    ui.dy(-ui.top + 0.08);
                    self.user_list_scroll.size((width, viewport_height));
                    self.user_list_scroll.render(ui, |ui| {
                        for i in 0..rn {
                            let cn = (n - i * columns).min(columns);
                            let row_width = w * cn as f32 + pad * (cn - 1) as f32;
                            let row_offset = (width - row_width) / 2.;
                            for j in 0..cn {
                                let r = Rect::new(row_offset + j as f32 * (w + pad), i as f32 * (h + pad), w, h);
                                let Some(user) = iter.next() else { unreachable!() };
                                ui.avatar(r.x + 0.055, r.center().y, 0.04, t, UserManager::opt_avatar(user.id, &self.icon_user));
                                ui.text(user.name)
                                    .pos(r.x + 0.105, r.center().y)
                                    .anchor(0., 0.5)
                                    .no_baseline()
                                    .max_width(0.36)
                                    .size(0.55)
                                    .draw();
                            }
                        }
                        (width, (rn as f32 * (h + pad) - pad).max(0.))
                    });
                });
            });
        }
    }

    #[inline]
    pub fn next_scene(&mut self) -> Option<NextScene> {
        self.next_scene.take()
    }
}
