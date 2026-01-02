use std::{collections::HashSet, error::Error, fmt::Display, path::Path};

use walkdir::WalkDir;

use crate::LANGS;

#[derive(Debug)]
struct IllegalLanguages {
    pub languages: Vec<String>,
}

impl Display for IllegalLanguages {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.languages)?;
        Ok(())
    }
}

impl Error for IllegalLanguages {}

fn get_ftl_files(path: &Path) -> Result<HashSet<String>, Box<dyn Error>> {
    let mut files = HashSet::new();
    for entry in WalkDir::new(path) {
        let entry = entry?;
        if entry.file_type().is_file() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "ftl") {
                let relative_path = path.strip_prefix(path.parent().unwrap())?;
                let normalized = relative_path.to_string_lossy().replace('\\', "/");
                files.insert(normalized);
            }
        }
    }
    Ok(files)
}

pub fn check_langfile(path: &str) -> Result<(), Box<dyn Error>> {
    let locales_dir = Path::new(path);
    let zh_cn_dir = locales_dir.join("zh-CN");
    let all_locales: [std::path::PathBuf; _] = LANGS.map(|x| locales_dir.join(x));
    let zh_cn_files = get_ftl_files(&zh_cn_dir)?;
    let mut inconsistent_languages = Vec::new();
    let mut i = 0;
    while i < LANGS.len() {
        let path = all_locales[i].to_owned();
        if path.is_dir() {
            let lang_code = LANGS[i];
            i += 1;
            if lang_code == "zh-CN" {
                continue;
            }

            match get_ftl_files(&path) {
                Ok(files) => {
                    if files != zh_cn_files {
                        inconsistent_languages.push(lang_code);
                    }
                }
                Err(_) => inconsistent_languages.push(lang_code),
            }
        }
    }

    if !inconsistent_languages.is_empty() {
        return Err(Box::new(IllegalLanguages {
            languages: inconsistent_languages.iter().map(|x| x.to_string()).collect(),
        }));
    }

    Ok(())
}
