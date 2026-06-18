use crate::{
    client::{Character, Chart, LocalCollection, Ptr, User},
    dir,
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use prpr::{
    config::{Config, Mods},
    info::ChartInfo,
    scene::SimpleRecord,
    ui::PREFER_REDUCED_MOTION,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    ops::DerefMut,
    path::Path,
    sync::{atomic::Ordering, Arc},
};
use tracing::{debug, warn};
use uuid::Uuid;

const MAX_IMPORT_RETRIES: u8 = 2;

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

impl BriefChartInfo {
    pub fn from_chart(chart: &Chart) -> Self {
        Self {
            id: Some(chart.id),
            uploader: Some(chart.uploader.clone()),
            name: chart.name.clone(),
            level: chart.level.clone(),
            difficulty: chart.difficulty,
            intro: chart.description.clone().unwrap_or_default(),
            charter: chart.charter.clone(),
            composer: chart.composer.clone(),
            illustrator: chart.illustrator.clone(),
            created: Some(chart.created),
            updated: Some(chart.updated),
            chart_updated: Some(chart.chart_updated),
            has_unlock: false,
        }
    }
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

    pub prefer_reduced_motion: bool,

    #[serde(default, rename = "collections")]
    collections_legacy: Vec<LocalCollection>,
    #[serde(default)]
    collection_uuids: Vec<Uuid>,

    /// Need to know what path caused the problem when restarting the program next time
    /// see: https://github.com/TeamFlos/phira/pull/689/#discussion_r2899026506
    #[serde(default)]
    pub import_scan_retry: HashMap<String, u8>,

    #[serde(skip)]
    collection_cache: DashMap<Uuid, Arc<LocalCollection>>,
}

impl Data {
    pub async fn init(&mut self) -> Result<()> {
        fn persist_retry_state(data: &Data) {
            let res = (|| -> Result<()> {
                let root = dir::root().with_context(|| "failed to get root directory")?;
                let path = format!("{}/data.json", root);
                std::fs::write(&path, serde_json::to_string(data)?).with_context(|| format!("failed to write to {}", path))?;
                Ok(())
            })();
            if let Err(err) = res {
                warn!(?err, "failed to persist import scan retry state");
            }
        }

        fn remove_failed_entry(path: &Path, key: &str, retry_map: &mut HashMap<String, u8>) {
            let remove_res = if path.is_dir() {
                std::fs::remove_dir_all(path)
            } else if path.exists() {
                std::fs::remove_file(path)
            } else {
                Ok(())
            };
            if let Err(err) = remove_res {
                warn!(?err, "failed to remove exhausted import entry: {}", key);
            }
            retry_map.remove(key);
        }

        fn bump_retry(map: &mut HashMap<String, u8>, key: &str) {
            let entry = map.entry(key.to_owned()).or_default();
            *entry = (*entry + 1).min(MAX_IMPORT_RETRIES);
        }

        let collections = dir::collections()?;
        for col in self.collections_legacy.drain(..) {
            let uuid = Uuid::new_v4();
            self.collection_uuids.push(uuid);
            std::fs::write(format!("{collections}/{uuid}.json"), serde_json::to_string(&col)?)?;
        }
        self.collection_uuids.retain(|uuid| match Self::load_collection_info(uuid) {
            Ok(info) => {
                self.collection_cache.insert(*uuid, Arc::new(info));
                true
            }
            Err(err) => {
                warn!(?err, "failed to load collection info during migration, skipping: {uuid}");
                false
            }
        });

        let charts = dir::charts()?;
        self.charts.retain(|it| Path::new(&format!("{}/{}", charts, it.local_path)).exists());
        let occurred: HashSet<_> = self.charts.iter().map(|it| it.local_path.clone()).collect();
        for entry in std::fs::read_dir(dir::custom_charts()?)? {
            let entry = entry?;
            let filename = entry.file_name();
            let filename = filename.to_str().unwrap();
            let filename = format!("custom/{filename}");
            let path = entry.path();
            if occurred.contains(&filename) {
                self.import_scan_retry.remove(&filename);
                continue;
            }
            if self.import_scan_retry.get(&filename).copied().unwrap_or_default() >= MAX_IMPORT_RETRIES {
                remove_failed_entry(&path, &filename, &mut self.import_scan_retry);
                persist_retry_state(self);
                warn!("skip startup import scan after retry limit reached: {filename}");
                continue;
            }
            // Persist retry count before parsing so crashes during parsing still consume one retry.
            bump_retry(&mut self.import_scan_retry, &filename);
            persist_retry_state(self);
            let Ok(mut fs) = prpr::fs::fs_from_file(&path) else {
                continue;
            };
            let result = prpr::fs::load_info(fs.deref_mut()).await;
            match result {
                Ok(info) => {
                    self.import_scan_retry.remove(&filename);
                    self.charts.push(LocalChart {
                        info: BriefChartInfo { id: None, ..info.into() },
                        local_path: filename,
                        record: None,
                        mods: Mods::default(),
                        played_unlock: false,
                    });
                }
                Err(err) => {
                    warn!(?err, "failed to parse startup custom import candidate: {}", filename);
                }
            }
        }
        for entry in std::fs::read_dir(dir::downloaded_charts()?)? {
            let entry = entry?;
            let filename = entry.file_name();
            let filename = filename.to_str().unwrap();
            let Ok(id): Result<i32, _> = filename.parse() else { continue };
            let filename = format!("download/{filename}");
            let path = entry.path();
            if occurred.contains(&filename) {
                self.import_scan_retry.remove(&filename);
                continue;
            }
            if self.import_scan_retry.get(&filename).copied().unwrap_or_default() >= MAX_IMPORT_RETRIES {
                remove_failed_entry(&path, &filename, &mut self.import_scan_retry);
                persist_retry_state(self);
                warn!("skip startup import scan after retry limit reached: {filename}");
                continue;
            }
            // Persist retry count before parsing so crashes during parsing still consume one retry.
            bump_retry(&mut self.import_scan_retry, &filename);
            persist_retry_state(self);
            let Ok(mut fs) = prpr::fs::fs_from_file(&path) else {
                warn!("failed to open file system for downloaded chart: {}", filename);
                continue;
            };
            let result = prpr::fs::load_info(fs.deref_mut()).await;
            match result {
                Ok(info) => {
                    self.import_scan_retry.remove(&filename);
                    self.charts.push(LocalChart {
                        info: BriefChartInfo { id: Some(id), ..info.into() },
                        local_path: filename,
                        record: None,
                        mods: Mods::default(),
                        played_unlock: false,
                    });
                }
                Err(err) => {
                    warn!(?err, "failed to parse startup downloaded import candidate: {}", filename);
                }
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
        self.respack_id = self.respack_id.min(self.respacks.len());
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
        if !self.collection_cache.iter().any(|it| it.value().is_default) {
            let uuid = Uuid::new_v4();
            self.set_collection_info(
                &uuid,
                LocalCollection {
                    is_default: true,
                    ..LocalCollection::new(crate::ttl!("default-fav-folder").into_owned())
                },
            )?;
            self.collection_uuids.insert(0, uuid);
        }
        let charts = dir::charts()?;
        self.local_records
            .retain(|local_path, _| Path::new(&format!("{charts}/{local_path}")).exists());

        self.config.init();
        PREFER_REDUCED_MOTION.store(self.prefer_reduced_motion, Ordering::Relaxed);
        Ok(())
    }

    pub fn find_chart_by_path(&self, local_path: &str) -> Option<usize> {
        self.charts.iter().position(|local| local.local_path == local_path)
    }

    pub fn collection_uuids(&self) -> &[Uuid] {
        &self.collection_uuids
    }
    pub fn collections(&self) -> impl Iterator<Item = Arc<LocalCollection>> + '_ {
        self.collection_uuids.iter().map(|uuid| self.collection_info(uuid))
    }
    pub fn set_collection_uuids(&mut self, uuids: Vec<Uuid>) {
        self.collection_uuids = uuids;
        self.collection_cache.clear();
    }
    pub fn collection_by_index(&self, index: usize) -> Arc<LocalCollection> {
        let uuid = &self.collection_uuids[index];
        self.collection_info(uuid)
    }

    pub fn set_collection_info(&self, uuid: &Uuid, info: LocalCollection) -> Result<()> {
        let path = Self::collection_info_path(uuid)?;
        std::fs::write(path, serde_json::to_string(&info)?)?;
        self.collection_cache.insert(*uuid, Arc::new(info));
        Ok(())
    }
    pub fn push_collection(&mut self, info: LocalCollection) -> Result<Uuid> {
        let uuid = Uuid::new_v4();
        self.set_collection_info(&uuid, info)?;
        self.collection_uuids.push(uuid);
        Ok(uuid)
    }
    pub fn remove_collection(&mut self, index: usize) -> Result<Uuid> {
        let uuid = self.collection_uuids.remove(index);
        let path = Self::collection_info_path(&uuid)?;
        std::fs::remove_file(path)?;
        self.collection_cache.remove(&uuid);
        Ok(uuid)
    }
    pub fn move_collection(&mut self, from: usize, to: usize) {
        let uuid = self.collection_uuids.remove(from);
        self.collection_uuids.insert(to, uuid);
    }

    fn collection_info_path(uuid: &Uuid) -> Result<String> {
        Ok(format!("{}/{}.json", dir::collections()?, uuid))
    }
    fn load_collection_info(uuid: &Uuid) -> Result<LocalCollection> {
        let path = Self::collection_info_path(uuid)?;
        let info: LocalCollection = serde_json::from_str(&std::fs::read_to_string(path)?)?;
        Ok(info)
    }
    pub fn collection_info(&self, uuid: &Uuid) -> Arc<LocalCollection> {
        self.collection_cache
            .entry(*uuid)
            .or_insert_with(|| match Self::load_collection_info(uuid) {
                Ok(info) => Arc::new(info),
                Err(err) => {
                    panic!("failed to load collection info of {uuid}: {err:?}");
                }
            })
            .value()
            .clone()
    }
}
