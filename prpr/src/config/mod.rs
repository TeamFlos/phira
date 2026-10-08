//! Configuration module of the playing environment.\
//! e.g. player name, volume, speed, autoplay, etc.

mod appearance;
mod audio;
mod graphics;

pub use appearance::AppearanceConfig;
pub use audio::AudioConfig;
pub use graphics::GraphicsConfig;

use bitflags::bitflags;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

pub static TIPS: Lazy<Vec<String>> = Lazy::new(|| include_str!("tips.txt").split('\n').map(str::to_owned).collect());

bitflags! {
    #[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq, Debug)]
    #[serde(transparent)]
    pub struct Mods: i32 {
        const AUTOPLAY = 0x0001;
        const FLIP_X = 0x0002;
        const FADE_OUT = 0x0004;
        const FADE_IN = 0x0008;
        const NIGHTCORE = 0x0010;
        const RAINBOW = 0x0020;
        const NO_SHADER = 0x0040;
        const INSTANT_DEATH_AP = 0x0080;
        const INSTANT_DEATH_FC = 0x0100;

        const UNRATED = Self::AUTOPLAY.bits() | Self::NO_SHADER.bits();
    }
}

impl Mods {
    pub fn toggle_mod(&mut self, flag: Mods) {
        if self.contains(flag) {
            self.remove(flag);
        } else {
            for &conflict in Mods::conflicts(flag) {
                self.remove(conflict);
            }
            self.insert(flag);
        }
    }
    fn conflicts(flag: Mods) -> &'static [Mods] {
        match flag {
            Mods::FADE_IN => &[Mods::FADE_OUT],
            Mods::FADE_OUT => &[Mods::FADE_IN],
            Mods::INSTANT_DEATH_AP => &[Mods::INSTANT_DEATH_FC],
            Mods::INSTANT_DEATH_FC => &[Mods::INSTANT_DEATH_AP],
            _ => &[],
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub appearance: AppearanceConfig,
    pub audio: AudioConfig,
    pub graphics: GraphicsConfig,

    #[serde(rename = "adjust_time_new")]
    pub adjust_time: bool,

    pub aggressive: bool,

    pub chart_debug: bool,
    pub touch_debug: bool,

    pub aspect_ratio: Option<f32>,
    pub double_click_to_pause: bool,

    pub interactive: bool,

    pub mods: Mods,
    pub speed: f32,

    pub offline_mode: bool,
    pub offset: f32,

    pub player_name: String,
    pub player_rks: f32,

    pub use_keyboard: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            appearance: Default::default(),
            audio: Default::default(),
            graphics: Default::default(),

            player_name: "Mivik".to_string(),
            player_rks: 15.,

            adjust_time: false,
            aggressive: true,

            aspect_ratio: None,

            double_click_to_pause: true,

            interactive: true,
            mods: Mods::default(),
            offline_mode: false,
            offset: 0.,
            speed: 1.,
            use_keyboard: false,

            chart_debug: false,
            touch_debug: false,
        }
    }
}

impl Config {
    pub fn init(&mut self) {
        #[cfg(target_env = "ohos")]
        {
            // Due to the fucking poor performance of the Maloon GPU, the sample count must be set to 1.
            self.sample_count = 1;
        }
    }

    #[inline]
    pub fn has_mod(&self, m: Mods) -> bool {
        self.mods.contains(m)
    }

    #[inline]
    pub fn autoplay(&self) -> bool {
        self.has_mod(Mods::AUTOPLAY)
    }

    #[inline]
    pub fn flip_x(&self) -> bool {
        self.has_mod(Mods::FLIP_X)
    }
}
