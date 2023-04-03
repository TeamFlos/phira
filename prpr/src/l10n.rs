pub use fluent::{fluent_args, FluentBundle, FluentResource};
use miniquad::warn;
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

static LANGS: [&str; 2] = ["zh-CN", "en-US"]; // this should be consistent with the macro below (create_bundles)
pub static LANG_NAMES: [&str; 2] = ["简体中文", "English"]; // this should be consistent with the macro below (create_bundles)
pub static LANG_IDENTS: Lazy<[LanguageIdentifier; 2]> = Lazy::new(|| LANGS.map(|lang| lang.parse().unwrap()));

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
        bundles.push($crate::create_bundle!("zh-CN", $file));
        bundles.push($crate::create_bundle!("en-US", $file));
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
        let locale_lang: LanguageIdentifier = locale_lang.parse().unwrap();
        for (id, lang) in LANG_IDENTS.iter().enumerate() {
            lang_map.insert(lang.clone(), id);
            if *lang == locale_lang {
                order.push(id);
            }
        }
        order.push(1); // zh-CN
        Self {
            lang_map,
            order: order.into(),
        }
    }
}

static GLOBAL: Lazy<L10nGlobal> = Lazy::new(L10nGlobal::new);

pub fn set_locale_order(locales: &[LanguageIdentifier]) {
    let mut ids = Vec::new();
    let map = &GLOBAL.lang_map;
    for locale in locales {
        if let Some(lang) = map.get(locale) {
            ids.push(*lang);
        }
    }
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
    cache: LruCache<&'static str, (usize, &'static Pattern<&'static str>)>,
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

    fn format_with_errors<'s>(&mut self, key: &'static str, args: Option<&'s FluentArgs<'s>>, errors: &mut Vec<FluentError>) -> Cow<'s, str> {
        let gen = GENERATION.load(Ordering::Relaxed);
        if gen > self.generation {
            self.generation = gen;
            self.cache.clear();
        }
        let (id, pattern) = self.cache.get_or_insert(key, || {
            let guard = GLOBAL.order.lock().unwrap();
            if let Some((id, message)) = guard
                .iter()
                .filter_map(|id| self.bundles.inner[*id].get_message(key).map(|msg| (*id, msg)))
                .next()
            {
                return (id, message.value().unwrap());
            }
            panic!("no translation found for {key}");
        });
        unsafe { std::mem::transmute(self.bundles.inner[*id].format_pattern(pattern, args, errors)) }
    }

    pub fn format<'s>(&mut self, key: &'static str, args: Option<&'s FluentArgs<'s>>) -> Cow<'s, str> {
        let mut errors = Vec::new();
        let res = self.format_with_errors(key, args, &mut errors);
        for error in errors {
            warn!("Message error {}: {:?}", key, error);
        }
        res
    }
}

#[macro_export]
macro_rules! tl_file {
    ($file:literal) => {
        $crate::tl_file!($file tl);
    };
    ($file:literal $macro_name:ident) => {
        static L10N_BUNDLES: $crate::l10n::Lazy<$crate::l10n::L10nBundles> = $crate::l10n::Lazy::new(|| $crate::create_bundles!($file).into());

        thread_local! {
            static L10N_LOCAL: std::cell::RefCell<$crate::l10n::L10nLocal> = $crate::l10n::L10nLocal::new(&*L10N_BUNDLES).into();
        }

        macro_rules! __tl_builder {
            ($d:tt) => {
                macro_rules! $macro_name {
                    ($d key:expr) => {
                        L10N_LOCAL.with(|it| it.borrow_mut().format($key, None))
                    };
                    ($d key:expr, $d args:expr) => {
                        L10N_LOCAL.with(|it| it.borrow_mut().format($key, Some($args)))
                    };
                    ($d key:expr, $d ($d name:expr => $d value:expr),+) => {
                        L10N_LOCAL.with(|it| it.borrow_mut().format($key, Some(&$crate::l10n::fluent_args![$d($d name => $d value), *])).to_string())
                    };
                    (err $d ($d body:tt)*) => {
                        anyhow::Error::msg(tl!($d($d body)*))
                    };
                    (bail $d ($d body:tt)*) => {
                        anyhow::Result::Err(anyhow::Error::msg(tl!($d($d body)*)))?
                    };
                }
            }
        }

        __tl_builder!($);
    };
}
