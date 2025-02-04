use once_cell::sync::Lazy;
use prpr::l10n::{LANGS, LANG_IDENTS};
use std::collections::HashSet;
use std::error::Error;
use std::fmt::Display;
use std::path::Path;
use walkdir::WalkDir;

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
            if path.extension().map_or(false, |ext| ext == "ftl") {
                let relative_path = path.strip_prefix(path.parent().unwrap())?;
                let normalized = relative_path.to_string_lossy().replace('\\', "/");
                files.insert(normalized);
            }
        }
    }
    Ok(files)
}
#[test]
fn check_langid() -> anyhow::Result<()> {
    // Lang ID is illegal if panicked
    Lazy::force(&LANG_IDENTS);
    Ok(())
}

#[test]
fn check_langfile() -> Result<(), Box<dyn Error>> {
    let locales_dir = Path::new("locales");
    let zh_cn_dir = locales_dir.join("zh-CN");
    let all_locales: [std::path::PathBuf; 12] = LANGS.map(|x| locales_dir.join(x));
    // 获取中文基准文件结构
    let zh_cn_files = get_ftl_files(&zh_cn_dir)?;
    let mut inconsistent_languages = Vec::new();
    let mut i = 0;
    // 遍历所有语言目录
    while i < LANGS.len() {
        let path = all_locales[i].to_owned();
        if path.is_dir() {
            let lang_code = LANGS[i];
            i += 1;
            if lang_code == "zh-CN" {
                continue;
            }

            // 获取当前语言的文件结构
            match get_ftl_files(&path) {
                Ok(files) => {
                    if files != zh_cn_files {
                        inconsistent_languages.push(lang_code);
                    }
                }
                Err(_) => inconsistent_languages.push(lang_code), // 无法读取的目录视为不一致
            }
        }
    }

    // 输出结果
    if !inconsistent_languages.is_empty() {
        return Err(Box::new(IllegalLanguages {
            languages: inconsistent_languages.iter().map(|x| x.to_string()).collect(),
        }));
    } else {
        println!("所有语言文件结构与 zh-CN 一致");
    }

    Ok(())
}
