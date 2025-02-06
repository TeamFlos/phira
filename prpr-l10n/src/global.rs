use std::{collections::HashMap, sync::Mutex};

use tracing::warn;
use unic_langid::LanguageIdentifier;

use crate::{fallback_langid, FALLBACK_LANG, LANG_IDENTS};

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
        let locale_lang = sys_locale::get_locale().unwrap_or_else(|| String::from(FALLBACK_LANG));
        let locale_lang: LanguageIdentifier = locale_lang.parse().unwrap_or_else(|_| {
            warn!("Invalid locale detected, defaulting to `{}`", FALLBACK_LANG);
            // Debug log: send lang tag to log
            warn!("Locale detected: {:?}", locale_lang);
            fallback_langid!()
        });
        for (id, lang) in LANG_IDENTS.iter().enumerate() {
            lang_map.insert(lang.clone(), id);
            if *lang == locale_lang {
                order.push(id);
            }
        }
        order.push(*lang_map.get(&fallback_langid!()).unwrap());
        Self {
            lang_map,
            order: order.into(),
        }
    }
}
