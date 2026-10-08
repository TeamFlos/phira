use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, Serialize)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct GraphicsConfig {
    pub fullscreen_mode: bool,
    pub fxaa: bool,
    pub sample_count: u32,
}

impl Default for GraphicsConfig {
    fn default() -> Self {
        Self {
            fxaa: false,
            fullscreen_mode: false,
            sample_count: 1,
        }
    }
}
