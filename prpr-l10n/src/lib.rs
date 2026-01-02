//! Localization utilities.

pub use fluent::{fluent_args, FluentBundle, FluentResource};
pub use once_cell::sync::Lazy;
pub use unic_langid::LanguageIdentifier;

use std::sync::atomic::{AtomicU8, Ordering};

mod global;
pub use global::*;

mod local;
pub use local::*;

mod macros;

pub mod tools;

langs! {
    "de-DE": "Deutsch",
    "en-US": "English",
    "fr-FR": "Français",
    "id-ID": "Bahasa Indonesia",
    "ja-JP": "日本語",
    "ko-KR": "한국어",
    "pl-PL": "Polski",
    "pt-BR": "Português",
    "ru-RU": "Русский",
    "th-TH": "แบบไทย",
    "vi-VN": "Tiếng Việt",
    "zh-CN": "简体中文",
    "zh-TW": "繁體中文"
}

#[macro_export]
macro_rules! fallback_langid {
    () => {
        unic_langid::langid!("en-US")
    };
}

pub const FALLBACK_LANG: &str = "en-US";

pub static GLOBAL: Lazy<L10nGlobal> = Lazy::new(L10nGlobal::new);

pub fn set_prefered_locale(locale: Option<LanguageIdentifier>) {
    let mut ids = Vec::new();
    let map = &GLOBAL.lang_map;
    if let Some(lang) = locale.and_then(|it| map.get(&it)) {
        ids.push(*lang);
    }
    if let Some(lang) = sys_locale::get_locale()
        .and_then(|it| it.parse::<LanguageIdentifier>().ok())
        .and_then(|it| map.get(&it))
    {
        ids.push(*lang);
    }
    ids.push(*map.get(&fallback_langid!()).unwrap());
    *GLOBAL.order.lock().unwrap() = ids;
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

pub fn locale_order() -> Vec<usize> {
    GLOBAL.order.lock().unwrap().clone()
}

pub struct L10nBundles {
    inner: Vec<FluentBundle<FluentResource>>,
}

impl From<Vec<FluentBundle<FluentResource>>> for L10nBundles {
    fn from(inner: Vec<FluentBundle<FluentResource>>) -> Self {
        Self { inner }
    }
}

unsafe impl Send for L10nBundles {}
unsafe impl Sync for L10nBundles {}

pub static GENERATION: AtomicU8 = AtomicU8::new(0);
