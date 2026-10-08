use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, Serialize)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct AudioConfig {
    pub audio_buffer_size: Option<u32>,
    pub preferred_sample_rate: Option<u32>,
    pub volume_music: f32,
    pub volume_sfx: f32,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            audio_buffer_size: None,
            preferred_sample_rate: None,
            volume_music: 1.,
            volume_sfx: 1.,
        }
    }
}
