use fluent::{FluentArgs, FluentError};
use fluent_syntax::ast::Pattern;
use lru::LruCache;
use std::{borrow::Cow, sync::atomic::Ordering};
use tracing::warn;

use crate::{L10nBundles, GENERATION, GLOBAL};

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
