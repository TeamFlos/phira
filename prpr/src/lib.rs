pub mod bin;
pub mod config;
pub mod core;
pub mod dir;
pub mod ext;
pub mod fs;
pub mod info;
pub mod judge;
pub mod parse;
pub mod particle;
pub mod replay;
pub mod scene;
pub mod task;
pub mod time;
pub mod ui;

#[cfg(feature = "log")]
pub mod log;

#[rustfmt::skip]
#[cfg(closed)]
pub mod inner;

pub use scene::Main;

use std::sync::Mutex;

static DATA_DIR: Mutex<Option<String>> = Mutex::new(None);

pub fn set_data_dir(dir: String) {
    *DATA_DIR.lock().unwrap() = Some(dir);
}

pub fn get_data_dir() -> Option<String> {
    DATA_DIR.lock().unwrap().clone()
}

pub fn build_conf() -> macroquad::window::Conf {
    macroquad::window::Conf {
        window_title: "Phira".to_string(),
        window_width: 973,
        window_height: 608,
        ..Default::default()
    }
}
