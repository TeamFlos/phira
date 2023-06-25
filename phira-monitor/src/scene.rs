use std::sync::Arc;

use crate::Config;
use anyhow::{Context, Result};
use log::info;
use macroquad::prelude::*;
use phira_mp_client::Client;
use phira_mp_common::RoomId;
use prpr::{
    ext::screen_aspect,
    scene::{show_error, Scene},
    task::Task,
    time::TimeManager,
    ui::Ui,
};
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;

#[derive(Serialize)]
struct LoginP<'a> {
    email: &'a str,
    password: &'a str,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoginR {
    token: String,
}

pub struct MainScene {
    config: Config,
    client: Option<Arc<Client>>,

    init_task: Option<Task<Result<Client>>>,
}

impl MainScene {
    pub async fn new(config: Config) -> Result<Self> {
        Ok(Self {
            config: config.clone(),
            client: None,

            init_task: Some(Task::new(async move {
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

                info!("连接 & 鉴权中…");
                let client = Client::new(TcpStream::connect(&config.server).await.context("连接到服务器失败")?)
                    .await
                    .context("连接失败")?;
                client.authenticate(resp.token).await?;

                info!("加入房间…");
                let room_id: RoomId = config.room_id.clone().try_into().context("房间 ID 不合法")?;
                client.join_room(room_id, true).await?;

                info!("初始化完成");

                Ok(client)
            })),
        })
    }
}

impl Scene for MainScene {
    fn touch(&mut self, _tm: &mut TimeManager, _touch: &Touch) -> Result<bool> {
        if self.client.is_none() {
            return Ok(true);
        }
        Ok(false)
    }

    fn update(&mut self, _tm: &mut TimeManager) -> Result<()> {
        if let Some(task) = &mut self.init_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        show_error(err.context("初始化失败"));
                    }
                    Ok(client) => {
                        self.client = Some(Arc::new(client));
                    }
                }
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

        ui.fill_rect(ui.screen_rect(), ui.background());

        let Some(client) = &self.client else {
            ui.full_loading_simple(t);
            return Ok(());
        };

        Ok(())
    }
}
