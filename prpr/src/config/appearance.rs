use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, Serialize)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct AppearanceConfig {
    pub ap_fc_indicator: bool,
    pub disable_effect: bool,
    pub double_hint: bool,
    pub note_scale: f32,
    pub particle: bool,
    pub res_pack_path: Option<String>,
    pub show_acc: bool,
    pub show_avg_fps: bool,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            ap_fc_indicator: true,
            disable_effect: false,
            double_hint: true,
            note_scale: 1.0,
            particle: true,
            res_pack_path: None,
            show_acc: false,
            show_avg_fps: false,
        }
    }
}
