use crate::{
    client::{Character, Ptr, User},
    dir,
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use prpr::{
    config::{Config, Mods},
    info::ChartInfo,
    scene::SimpleRecord,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    ops::DerefMut,
    path::Path,
};
use tracing::debug;

pub const DEFAULT_FAVORITES_KEY: &str = "default";

// 收藏夹数据结构 || Favorites data structure
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Favorites {
    // 收藏夹列表：key 为收藏夹名称，value 为 local_path 列表 || Folder map: key = folder name, value = local_path list
    #[serde(default)]
    pub folders: HashMap<String, Vec<String>>,
    // 收藏夹自定义封面路径 || Custom cover image paths
    #[serde(default)]
    pub covers: HashMap<String, String>,
}

impl Favorites {
    pub fn ensure_default(&mut self) {
        if !self.folders.contains_key(DEFAULT_FAVORITES_KEY) {
            self.folders.insert(DEFAULT_FAVORITES_KEY.to_string(), Vec::new());
        }
    }

    pub fn is_in_default(&self, local_path: &str) -> bool {
        self.folders
            .get(DEFAULT_FAVORITES_KEY)
            .map_or(false, |v| v.contains(&local_path.to_string()))
    }

    pub fn is_favorited(&self, local_path: &str) -> bool {
        self.folders.values().any(|v| v.contains(&local_path.to_string()))
    }

    pub fn add_to(&mut self, folder: &str, local_path: &str) {
        let list = self.folders.entry(folder.to_string()).or_default();
        if !list.contains(&local_path.to_string()) {
            list.push(local_path.to_string());
        }
    }

    pub fn remove_from(&mut self, folder: &str, local_path: &str) {
        if let Some(list) = self.folders.get_mut(folder) {
            list.retain(|p| p != local_path);
        }
    }

    // 切换默认收藏夹状态，返回是否添加 || Toggle default folder, returns whether added
    pub fn toggle_default(&mut self, local_path: &str) -> bool {
        self.ensure_default();
        let list = self.folders.get_mut(DEFAULT_FAVORITES_KEY).unwrap();
        if let Some(pos) = list.iter().position(|p| p == local_path) {
            list.remove(pos);
            false
        } else {
            list.push(local_path.to_string());
            true
        }
    }

    pub fn custom_folder_names(&self) -> Vec<String> {
        self.folders
            .keys()
            .filter(|k| k.as_str() != DEFAULT_FAVORITES_KEY)
            .cloned()
            .collect()
    }

    // 所有收藏夹名称（默认在前） || All folder names (default first)
    pub fn all_folder_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        if self.folders.contains_key(DEFAULT_FAVORITES_KEY) {
            names.push(DEFAULT_FAVORITES_KEY.to_string());
        }
        for k in self.folders.keys() {
            if k != DEFAULT_FAVORITES_KEY {
                names.push(k.clone());
            }
        }
        names
    }

    pub fn get_paths(&self, folder: &str) -> Vec<String> {
        self.folders.get(folder).cloned().unwrap_or_default()
    }

    pub fn delete_folder(&mut self, folder: &str) {
        if folder != DEFAULT_FAVORITES_KEY {
            self.folders.remove(folder);
        }
    }

    pub fn rename_folder(&mut self, old_name: &str, new_name: &str) -> bool {
        if old_name == DEFAULT_FAVORITES_KEY || self.folders.contains_key(new_name) {
            return false;
        }
        if let Some(paths) = self.folders.remove(old_name) {
            self.folders.insert(new_name.to_string(), paths);
            true
        } else {
            false
        }
    }

    pub fn create_folder(&mut self, name: &str) -> bool {
        if self.folders.contains_key(name) {
            return false;
        }
        self.folders.insert(name.to_string(), Vec::new());
        true
    }

    pub fn folders_containing(&self, local_path: &str) -> Vec<String> {
        self.folders
            .iter()
            .filter(|(_, v)| v.contains(&local_path.to_string()))
            .map(|(k, _)| k.clone())
            .collect()
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BriefChartInfo {
    pub id: Option<i32>,
    pub uploader: Option<Ptr<User>>,
    pub name: String,
    pub level: String,
    pub difficulty: f32,
    #[serde(alias = "description")]
    pub intro: String,
    pub charter: String,
    pub composer: String,
    pub illustrator: String,
    pub created: Option<DateTime<Utc>>,
    pub updated: Option<DateTime<Utc>>,
    pub chart_updated: Option<DateTime<Utc>>,
    #[serde(default)]
    pub has_unlock: bool,
}

impl From<ChartInfo> for BriefChartInfo {
    fn from(info: ChartInfo) -> Self {
        Self {
            id: info.id,
            uploader: info.uploader.map(Ptr::new),
            name: info.name,
            level: info.level,
            difficulty: info.difficulty,
            intro: info.intro,
            charter: info.charter,
            composer: info.composer,
            illustrator: info.illustrator,
            created: info.created,
            updated: info.updated,
            chart_updated: info.chart_updated,
            has_unlock: info.unlock_video.is_some(),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct LocalChart {
    #[serde(flatten)]
    pub info: BriefChartInfo,
    pub local_path: String,
    pub record: Option<SimpleRecord>,
    #[serde(default)]
    pub mods: Mods,
    #[serde(default)]
    pub played_unlock: bool,
}

fn default_anys_gateway() -> String {
    "https://anys.mivik.moe".to_string()
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Data {
    pub me: Option<User>,
    pub charts: Vec<LocalChart>,
    pub local_records: HashMap<String, Option<SimpleRecord>>,
    pub config: Config,
    pub message_check_time: Option<DateTime<Utc>>,
    pub language: Option<String>,
    pub theme: usize,
    pub tokens: Option<(String, String)>,
    pub respacks: Vec<String>,
    pub respack_id: usize,
    pub accept_invalid_cert: bool,
    // for compatibility
    pub read_tos_and_policy: bool,
    pub terms_modified: Option<String>,
    pub ignored_version: Option<semver::Version>,
    pub character: Option<Character>,

    pub enable_anys: bool,
    #[serde(default = "default_anys_gateway")]
    pub anys_gateway: String,

    #[serde(default)]
    pub favorites: Favorites,
}

impl Data {
    pub async fn init(&mut self) -> Result<()> {
        let charts = dir::charts()?;
        self.charts.retain(|it| Path::new(&format!("{}/{}", charts, it.local_path)).exists());
        let occurred: HashSet<_> = self.charts.iter().map(|it| it.local_path.clone()).collect();
        for entry in std::fs::read_dir(dir::custom_charts()?)? {
            let entry = entry?;
            let filename = entry.file_name();
            let filename = filename.to_str().unwrap();
            let filename = format!("custom/{filename}");
            if occurred.contains(&filename) {
                continue;
            }
            let path = entry.path();
            let Ok(mut fs) = prpr::fs::fs_from_file(&path) else {
                continue;
            };
            let result = prpr::fs::load_info(fs.deref_mut()).await;
            if let Ok(info) = result {
                self.charts.push(LocalChart {
                    info: BriefChartInfo { id: None, ..info.into() },
                    local_path: filename,
                    record: None,
                    mods: Mods::default(),
                    played_unlock: false,
                });
            }
        }
        for entry in std::fs::read_dir(dir::downloaded_charts()?)? {
            let entry = entry?;
            let filename = entry.file_name();
            let filename = filename.to_str().unwrap();
            let Ok(id): Result<i32, _> = filename.parse() else { continue };
            let filename = format!("download/{filename}");
            if occurred.contains(&filename) {
                continue;
            }
            let path = entry.path();
            let Ok(mut fs) = prpr::fs::fs_from_file(&path) else {
                continue;
            };
            let result = prpr::fs::load_info(fs.deref_mut()).await;
            if let Ok(info) = result {
                self.charts.push(LocalChart {
                    info: BriefChartInfo { id: Some(id), ..info.into() },
                    local_path: filename,
                    record: None,
                    mods: Mods::default(),
                    played_unlock: false,
                });
            }
        }
        let respacks: HashSet<_> = self.respacks.iter().cloned().collect();
        for entry in std::fs::read_dir(dir::respacks()?)? {
            let entry = entry?;
            let filename = entry.file_name();
            let filename = filename.to_str().unwrap().to_string();
            if respacks.contains(&filename) {
                continue;
            }
            self.respacks.push(filename);
        }
        if let Some(res_pack_path) = &mut self.config.res_pack_path {
            if res_pack_path.starts_with('/') {
                // for compatibility
                *res_pack_path = "chart.zip".to_owned();
            }
        }
        if self.read_tos_and_policy {
            debug!("migrating from old version");
            self.terms_modified = Some("Mon, 05 Aug 2024 17:32:41 GMT".to_owned());
            self.read_tos_and_policy = false;
        }
        self.config.init();
        self.favorites.ensure_default();
        Ok(())
    }

    pub fn find_chart_by_path(&self, local_path: &str) -> Option<usize> {
        self.charts.iter().position(|local| local.local_path == local_path)
    }
}
