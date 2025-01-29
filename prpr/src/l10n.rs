//! Localization utilities.

pub use fluent::{fluent_args, FluentBundle, FluentResource};
pub use once_cell::sync::Lazy;
pub use unic_langid::{langid, LanguageIdentifier};

use fluent::{FluentArgs, FluentError};
use fluent_syntax::ast::Pattern;
use lru::LruCache;
use std::{
    borrow::Cow,
    collections::HashMap,
    sync::{
        atomic::{AtomicU8, Ordering},
        Mutex,
    },
};
use sys_locale::get_locale;
use tracing::warn;

pub static LANGS: [&str; 12] = [
    "en-US", "fr-FR", "id-ID", "ja-JP", "ko-KR", "pl-PL", "pt-BR", "ru-RU", "th-TH", "vi-VN", "zh-CN", "zh-TW",
]; // this should be consistent with the macro below (create_bundles)
pub static LANG_NAMES: [&str; 12] = [
    "English",
    "Français",
    "Bahasa Indonesia",
    "日本語",
    "한국어",
    "Polski",
    "Português",
    "Русский",
    "แบบไทย",
    "Tiếng Việt",
    "简体中文",
    "繁體中文",
]; // this should be consistent with the macro below (create_bundles)
pub static LANG_IDENTS: Lazy<[LanguageIdentifier; 12]> = Lazy::new(|| LANGS.map(|lang| lang.parse().unwrap()));

#[macro_export]
macro_rules! create_bundle {
    ($locale:literal, $file:literal) => {{
        let mut bundle = $crate::l10n::FluentBundle::new($crate::l10n::LANG_IDENTS.iter().cloned().collect());
        bundle
            .add_resource(
                $crate::l10n::FluentResource::try_new(
                    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/locales/", $locale, "/", $file, ".ftl")).to_owned(),
                )
                .unwrap(),
            )
            .unwrap();
        bundle.set_use_isolating(false);
        bundle
    }};
}

#[macro_export]
macro_rules! create_bundles {
    ($file:literal) => {{
        let mut bundles = Vec::new();
        bundles.push($crate::create_bundle!("en-US", $file));
        bundles.push($crate::create_bundle!("fr-FR", $file));
        bundles.push($crate::create_bundle!("id-ID", $file));
        bundles.push($crate::create_bundle!("ja-JP", $file));
        bundles.push($crate::create_bundle!("ko-KR", $file));
        bundles.push($crate::create_bundle!("pl-PL", $file));
        bundles.push($crate::create_bundle!("pt-BR", $file));
        bundles.push($crate::create_bundle!("ru-RU", $file));
        bundles.push($crate::create_bundle!("th-TH", $file));
        bundles.push($crate::create_bundle!("vi-VN", $file));
        bundles.push($crate::create_bundle!("zh-CN", $file));
        bundles.push($crate::create_bundle!("zh-TW", $file));
        bundles
    }};
}

pub struct L10nGlobal {
    pub lang_map: HashMap<LanguageIdentifier, usize>,
    pub order: Mutex<Vec<usize>>,
}

impl Default for L10nGlobal {
    fn default() -> Self {
        Self::new()
    }
}

impl L10nGlobal {
    pub fn new() -> Self {
        let mut lang_map = HashMap::new();
        let mut order = Vec::new();
        let locale_lang = get_locale().unwrap_or_else(|| String::from("en-US"));
        let locale_lang: LanguageIdentifier = locale_lang.parse().unwrap_or_else(|_| {
            warn!("Invalid locale detected, defaulting to en-US");
            // Debug log: send lang tag to log
            warn!("Locale detected: {:?}", locale_lang);
            langid!("en-US")
        });
        for (id, lang) in LANG_IDENTS.iter().enumerate() {
            lang_map.insert(lang.clone(), id);
            if *lang == locale_lang {
                order.push(id);
            }
        }
        order.push(*lang_map.get(&langid!("en-US")).unwrap());
        Self {
            lang_map,
            order: order.into(),
        }
    }
}

pub static GLOBAL: Lazy<L10nGlobal> = Lazy::new(L10nGlobal::new);

pub fn set_prefered_locale(locale: Option<LanguageIdentifier>) {
    let mut ids = Vec::new();
    let map = &GLOBAL.lang_map;
    if let Some(lang) = locale.and_then(|it| map.get(&it)) {
        ids.push(*lang);
    }
    if let Some(lang) = get_locale()
        .and_then(|it| it.parse::<LanguageIdentifier>().ok())
        .and_then(|it| map.get(&it))
    {
        ids.push(*lang);
    }
    ids.push(*map.get(&langid!("en-US")).unwrap());
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

pub struct L10nLocal {
    bundles: &'static L10nBundles,
    cache: LruCache<Cow<'static, str>, (usize, &'static Pattern<&'static str>)>,
    generation: u8,
}

impl L10nLocal {
    pub fn new(bundles: &'static L10nBundles) -> Self {
        Self {
            bundles,
            cache: LruCache::new(16.try_into().unwrap()),
            generation: 0,
        }
    }

    fn format_with_errors<'s>(&mut self, key: Cow<'static, str>, args: Option<&'s FluentArgs<'s>>, errors: &mut Vec<FluentError>) -> Cow<'s, str> {
        let gen = GENERATION.load(Ordering::Relaxed);
        if gen > self.generation {
            self.generation = gen;
            self.cache.clear();
        }
        if let Some((id, pattern)) = {
            let get_result = self.cache.get(&key);
            if get_result.is_none() {
                let guard = GLOBAL.order.lock().unwrap();
                guard
                    .iter()
                    .filter_map(|id| self.bundles.inner[*id].get_message(&key).map(|msg| (*id, msg)))
                    .next()
                    .map(|(id, message)| (id, message.value().unwrap()))
                    .map(|val| self.cache.get_or_insert(key.clone(), || val))
            } else {
                get_result
            }
        } {
            unsafe { std::mem::transmute(self.bundles.inner[*id].format_pattern(pattern, args, errors)) }
        } else {
            warn!("no translation found for {key}, returning key");
            key
        }
    }

    pub fn format<'s>(&mut self, key: impl Into<Cow<'static, str>>, args: Option<&'s FluentArgs<'s>>) -> Cow<'s, str> {
        let mut errors = Vec::new();
        let key: Cow<'static, str> = key.into();
        let res = self.format_with_errors(key.clone(), args, &mut errors);
        for error in errors {
            warn!("l10n error {key}: {error:?}");
        }
        res
    }
}

#[macro_export]
macro_rules! tl_file {
    ($file:literal) => {
        $crate::tl_file!($file tl);
    };
    ($file:literal $macro_name:ident $($p:tt)*) => {
        static L10N_BUNDLES: $crate::l10n::Lazy<$crate::l10n::L10nBundles> = $crate::l10n::Lazy::new(|| $crate::create_bundles!($file).into());

        thread_local! {
            pub static L10N_LOCAL: std::cell::RefCell<$crate::l10n::L10nLocal> = $crate::l10n::L10nLocal::new(&*L10N_BUNDLES).into();
        }

        macro_rules! __tl_builder {
            ($d:tt) => {
                macro_rules! $macro_name {
                    ($d key:expr) => {
                        $($p)* L10N_LOCAL.with(|it| it.borrow_mut().format($key, None))
                    };
                    ($d key:expr, $d args:expr) => {
                        $($p)* L10N_LOCAL.with(|it| it.borrow_mut().format($key, Some($args)))
                    };
                    ($d key:expr, $d ($d name:expr => $d value:expr),+) => {
                        $($p)* L10N_LOCAL.with(|it| it.borrow_mut().format($key, Some(&$crate::l10n::fluent_args![$d($d name => $d value), *])).to_string())
                    };
                    (err $d ($d body:tt)*) => {
                        anyhow::Error::msg($macro_name!($d($d body)*))
                    };
                    (bail $d ($d body:tt)*) => {
                        return anyhow::Result::Err(anyhow::Error::msg($macro_name!($d($d body)*)))
                    };
                }

                pub(crate) use $macro_name;
            }
        }

        __tl_builder!($);
    };
}
