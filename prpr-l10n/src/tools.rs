use std::{
    collections::{BTreeSet, HashMap, HashSet},
    error::Error,
    fmt::Display,
    fs,
    path::{Path, PathBuf},
};

use fluent_syntax::{ast, parser};
use walkdir::WalkDir;

use crate::{FALLBACK_LANG, LANGS};

#[derive(Debug)]
struct L10nCheckErrors {
    messages: Vec<String>,
}

impl Display for L10nCheckErrors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (idx, message) in self.messages.iter().enumerate() {
            if idx > 0 {
                writeln!(f)?;
            }
            write!(f, "{message}")?;
        }
        Ok(())
    }
}

impl Error for L10nCheckErrors {}

#[derive(Debug)]
struct FileReport {
    keys: BTreeSet<String>,
    has_crlf: bool,
}

const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];
const SUMMARY_LIMIT: usize = 8;

fn summarize_list(items: &[String]) -> String {
    if items.len() <= SUMMARY_LIMIT {
        return items.join(", ");
    }
    let shown = items[..SUMMARY_LIMIT].join(", ");
    format!("{shown}, ... (+{} more)", items.len() - SUMMARY_LIMIT)
}

fn collect_ftl_files(locale_dir: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut files = Vec::new();
    for entry in WalkDir::new(locale_dir) {
        let entry = entry?;
        if entry.file_type().is_file() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "ftl") {
                files.push(path.to_path_buf());
            }
        }
    }
    Ok(files)
}

fn extract_keys(resource: &ast::Resource<&str>) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    for entry in &resource.body {
        match entry {
            ast::Entry::Message(message) => {
                let id = message.id.name.to_string();
                keys.insert(id.clone());
                for attr in &message.attributes {
                    keys.insert(format!("{id}.{}", attr.id.name));
                }
            }
            ast::Entry::Term(term) => {
                let id = format!("-{}", term.id.name);
                keys.insert(id.clone());
                for attr in &term.attributes {
                    keys.insert(format!("{id}.{}", attr.id.name));
                }
            }
            _ => {}
        }
    }
    keys
}

fn read_and_parse_ftl(path: &Path) -> Result<FileReport, String> {
    let bytes = fs::read(path).map_err(|err| format!("{}: failed to read ({err})", path.display()))?;
    if bytes.starts_with(UTF8_BOM) {
        return Err(format!("{}: UTF-8 BOM is not allowed", path.display()));
    }
    let text = std::str::from_utf8(&bytes).map_err(|err| format!("{}: not valid UTF-8 ({err})", path.display()))?;
    if text.contains('\u{FEFF}') {
        return Err(format!("{}: contains BOM character (U+FEFF)", path.display()));
    }
    let has_crlf = text.contains("\r\n");
    match parser::parse(text) {
        Ok(resource) => Ok(FileReport {
            keys: extract_keys(&resource),
            has_crlf,
        }),
        Err((_, errors)) => {
            let mut details: Vec<String> = errors.iter().map(|err| format!("{}-{}: {err}", err.pos.start, err.pos.end)).collect();
            details.sort();
            let summary = summarize_list(&details);
            Err(format!("{}: invalid FTL ({summary})", path.display()))
        }
    }
}

pub fn check_langfile(path: &str) -> Result<(), Box<dyn Error>> {
    let locales_dir = Path::new(path);
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    let mut locale_files: HashMap<&'static str, HashSet<PathBuf>> = HashMap::new();
    let mut locale_keys: HashMap<&'static str, BTreeSet<String>> = HashMap::new();
    let mut crlf_files: Vec<String> = Vec::new();

    for &lang in LANGS.iter() {
        let locale_dir = locales_dir.join(lang);
        if !locale_dir.is_dir() {
            errors.push(format!("missing locale directory: {lang} ({})", locale_dir.display()));
            continue;
        }

        let mut files = HashSet::new();
        let mut keys = BTreeSet::new();

        let ftl_files = match collect_ftl_files(&locale_dir) {
            Ok(files) => files,
            Err(err) => {
                errors.push(format!("{}: failed to list .ftl files ({err})", locale_dir.display()));
                continue;
            }
        };

        for file_path in ftl_files {
            let rel = file_path.strip_prefix(&locale_dir).unwrap_or(&file_path).to_path_buf();
            files.insert(rel);

            match read_and_parse_ftl(&file_path) {
                Ok(report) => {
                    keys.extend(report.keys);
                    if report.has_crlf {
                        let rel_to_locales = file_path.strip_prefix(locales_dir).unwrap_or(&file_path);
                        crlf_files.push(rel_to_locales.display().to_string());
                    }
                }
                Err(message) => errors.push(message),
            }
        }

        locale_files.insert(lang, files);
        locale_keys.insert(lang, keys);
    }

    let base_lang = FALLBACK_LANG;
    let base_files = match locale_files.get(base_lang) {
        Some(files) => Some(files),
        None => {
            errors.push(format!("missing base locale directory: {base_lang}"));
            None
        }
    };

    if let Some(base_files) = base_files {
        for &lang in LANGS.iter() {
            if lang == base_lang {
                continue;
            }
            if let Some(lang_files) = locale_files.get(lang) {
                let mut missing: Vec<String> = base_files.difference(lang_files).map(|path| path.display().to_string()).collect();
                missing.sort();
                if !missing.is_empty() {
                    errors.push(format!("missing files in {lang}: {}", summarize_list(&missing)));
                }
            }
        }
    }

    if let Some(base_keys) = locale_keys.get(base_lang) {
        for &lang in LANGS.iter() {
            if lang == base_lang {
                continue;
            }
            if let Some(lang_keys) = locale_keys.get(lang) {
                let mut missing: Vec<String> = base_keys.difference(lang_keys).cloned().collect();
                if !missing.is_empty() {
                    missing.sort();
                    warnings.push(format!("missing keys in {lang}: {}", summarize_list(&missing)));
                }
            }
        }
    }

    if !crlf_files.is_empty() {
        crlf_files.sort();
        warnings.push(format!("CRLF line endings detected: {}", summarize_list(&crlf_files)));
    }

    for warning in &warnings {
        eprintln!("[l10n][warning] {warning}");
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(Box::new(L10nCheckErrors { messages: errors }))
    }
}
