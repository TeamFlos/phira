use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

pub static TIPS: Lazy<Vec<String>> = Lazy::new(|| include_str!("tips.txt").split('\n').map(str::to_owned).collect());

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChallengeModeColor {
    White,
    Green,
    Blue,
    Red,
    Golden,
    Rainbow,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(rename = "adjust_time_new")]
    pub adjust_time: bool,
    pub aggressive: bool,
    pub aspect_ratio: Option<f32>,
    pub audio_buffer_size: Option<u32>,
    pub autoplay: bool,
    pub challenge_color: ChallengeModeColor,
    pub challenge_rank: u32,
    pub debug: bool,
    pub disable_effect: bool,
    pub double_click_to_pause: bool,
    pub fix_aspect_ratio: bool,
    pub fxaa: bool,
    pub interactive: bool,
    pub double_hint: bool,
    pub note_scale: f32,
    pub offline_mode: bool,
    pub offset: f32,
    pub particle: bool,
    pub player_name: String,
    pub player_rks: f32,
    pub sample_count: u32,
    pub res_pack_path: Option<String>,
    pub speed: f32,
    pub volume_music: f32,
    pub volume_sfx: f32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            adjust_time: false,
            aggressive: true,
            aspect_ratio: None,
            audio_buffer_size: None,
            autoplay: false,
            challenge_color: ChallengeModeColor::Golden,
            challenge_rank: 45,
            debug: false,
            disable_effect: false,
            double_click_to_pause: true,
            fix_aspect_ratio: false,
            fxaa: false,
            interactive: true,
            double_hint: true,
            note_scale: 1.0,
            offline_mode: false,
            offset: 0.,
            res_pack_path: None,
            particle: true,
            player_name: "Mivik".to_string(),
            player_rks: 15.,
            sample_count: 1,
            speed: 1.,
            volume_music: 1.,
            volume_sfx: 1.,
        }
    }
}
