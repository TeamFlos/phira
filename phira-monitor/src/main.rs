mod cloud;
mod launch;
mod scene;

use anyhow::{Context, Result};
use macroquad::prelude::*;
use prpr::{
    core::init_assets,
    scene::show_error,
    time::TimeManager,
    ui::{FontArc, TextPainter},
    Main,
};
use scene::MainScene;
use serde::Deserialize;
use std::fs::File;

mod dir {
    use anyhow::Result;

    fn ensure(s: &str) -> Result<String> {
        let path = std::path::Path::new(s);
        if !path.exists() {
            std::fs::create_dir_all(path)?;
        }
        Ok(s.to_owned())
    }

    pub fn charts() -> Result<String> {
        ensure("data/charts")
    }

    pub fn downloaded_charts() -> Result<String> {
        ensure("data/charts/download")
    }
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    server: String,

    email: String,
    password: String,

    room_id: String,
}

pub fn build_conf() -> macroquad::window::Conf {
    macroquad::window::Conf {
        window_title: "prpr-monitor".to_string(),
        window_width: 1080,
        window_height: 608,
        ..Default::default()
    }
}

#[macroquad::main(build_conf)]
async fn main() {
    if let Err(err) = the_main().await {
        eprintln!("{err:?}");
    }
}

async fn the_main() -> Result<()> {
    pretty_env_logger::init();

    init_assets();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap();
    let _guard = rt.enter();

    let font = FontArc::try_from_vec(load_file("font.ttf").await?)?;
    let mut painter = TextPainter::new(font, None);

    let config: Config = (|| -> Result<Config> { Ok(serde_yaml::from_reader(File::open("monitor-config.yml")?)?) })().context("读取配置失败")?;

    let mut main = Main::new(Box::new(MainScene::new(config.clone()).await?), TimeManager::default(), None).await?;
    // main.viewport = Some((0, 100, 500, 500));

    let tm = TimeManager::default();
    let mut fps_time = -1;
    'app: loop {
        let frame_start = tm.real_time();
        let res = || -> Result<()> {
            main.update()?;
            main.render(&mut painter)?;
            Ok(())
        }();
        if let Err(err) = res {
            warn!(?err, "uncaught error");
            show_error(err);
        }
        if main.should_exit() {
            break 'app;
        }

        let t = tm.real_time();
        let fps_now = t as i32;
        if fps_now != fps_time {
            fps_time = fps_now;
            info!("| {}", (1. / (t - frame_start)) as u32);
        }

        next_frame().await;
    }

    Ok(())
}
