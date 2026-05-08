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
pub mod scene;
pub mod task;
pub mod time;
pub mod ui;

#[cfg(feature = "log")]
pub mod log;

#[rustfmt::skip]
#[cfg(closed)]
pub mod inner;

use miniquad::conf::{LinuxBackend, LinuxX11Gl, Platform};
pub use scene::Main;

pub fn build_conf() -> macroquad::window::Conf {
    macroquad::window::Conf {
        window_title: "Phira".to_string(),
        window_width: 973,
        window_height: 608,
        platform: Platform {
            linux_x11_gl: LinuxX11Gl::GLXWithEGLFallback,
            swap_interval: None,
            linux_backend: LinuxBackend::WaylandWithX11Fallback,
            framebuffer_alpha: false,
        },
        ..Default::default()
    }
}
