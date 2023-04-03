pub mod config;
pub mod core;
pub mod ext;
pub mod fs;
pub mod info;
pub mod judge;
pub mod l10n;
pub mod parse;
pub mod particle;
pub mod scene;
pub mod task;
pub mod time;
pub mod ui;

#[cfg(feature = "closed")]
pub mod inner;

#[cfg(target_os = "ios")]
pub mod objc;

pub use scene::Main;

pub fn build_conf() -> macroquad::window::Conf {
    macroquad::window::Conf {
        window_title: "prpr".to_string(),
        window_width: 1080,
        window_height: 608,
        ..Default::default()
    }
}
