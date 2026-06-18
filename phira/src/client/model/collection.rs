use std::{
    borrow::Cow,
    collections::HashSet,
    fmt::Debug,
    hash::{Hash, Hasher},
};

use crate::{
    client::{recv_raw, Client, File},
    data::BriefChartInfo,
    dir, get_data,
    page::{local_illustration, Illustration},
};

use super::{Chart, Object, Ptr, User};
use anyhow::Result;
use chrono::{DateTime, Utc};
use prpr::{ext::BLACK_TEXTURE, info::ChartInfo, task::Task, ui::Dialog};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct Collection {
    pub id: i32,
    pub cover: Option<File>,
    pub owner: Ptr<User>,
    pub name: String,
    pub description: String,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
    pub charts: Vec<Chart>,
    pub public: bool,
}
impl Object for Collection {
    const QUERY_PATH: &'static str = "collection";

    fn id(&self) -> i32 {
        self.id
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ChartRefChartInfo {
    #[serde(flatten)]
    pub info: BriefChartInfo,
    pub illustration: File,
}

impl ChartRefChartInfo {
    pub fn from_chart(chart: &Chart) -> Self {
        Self {
            info: BriefChartInfo::from_chart(chart),
            illustration: chart.illustration.clone(),
        }
    }
}

#[derive(Clone, Serialize)]
pub struct ChartRef {
    pub path: String,
    pub info: Option<Box<ChartRefChartInfo>>,
}

impl Debug for ChartRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChartRef").field("path", &self.path).finish()
    }
}

impl<'de> Deserialize<'de> for ChartRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum FuseChartRef {
            New { path: String, info: Option<Box<ChartRefChartInfo>> },
            Local(String),
            Online(i32, Option<Box<ChartRefChartInfo>>),
        }

        let fuse = FuseChartRef::deserialize(deserializer)?;
        Ok(match fuse {
            FuseChartRef::New { path, info } => Self { path, info },
            FuseChartRef::Local(local_path) => Self {
                path: local_path,
                info: None,
            },
            FuseChartRef::Online(id, info) => Self {
                path: format!("download/{id}"),
                info,
            },
        })
    }
}

impl ChartRef {
    pub fn new_bare(id: Option<i32>, local_path: Option<&str>) -> Self {
        let path = if let Some(id) = id {
            format!("download/{id}")
        } else if let Some(local) = local_path {
            local.to_string()
        } else {
            panic!("chart ref must have either id or local path");
        };
        Self { path, info: None }
    }

    pub fn exists(&self) -> bool {
        std::fs::exists(format!("{}/{}", dir::charts().unwrap(), self.path)).unwrap()
    }

    pub fn find_local_path<'a>(&'a self) -> Result<Option<Cow<'a, str>>> {
        let charts = dir::charts()?;
        if std::fs::exists(format!("{charts}/{}", self.path))? {
            return Ok(Some(Cow::Borrowed(&self.path)));
        }
        let Some(id) = self.id() else {
            return Ok(None);
        };
        // TODO: optimize
        Ok(get_data().charts.iter().find_map(|it| {
            if it.info.id == Some(id) {
                Some(Cow::Owned(it.local_path.clone()))
            } else {
                None
            }
        }))
    }

    pub fn id(&self) -> Option<i32> {
        self.path.strip_prefix("download/").and_then(|s| s.parse().ok())
    }
    pub fn is_online(&self) -> bool {
        self.path.starts_with("download/")
    }
}

impl From<Chart> for ChartRef {
    fn from(chart: Chart) -> Self {
        ChartRef {
            path: format!("download/{}", chart.id),
            info: Some(Box::new(ChartRefChartInfo::from_chart(&chart))),
        }
    }
}

impl PartialEq for ChartRef {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}
impl Eq for ChartRef {}

impl Hash for ChartRef {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.path.hash(state);
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub enum CollectionCover {
    Unset,
    Online(File),
    LocalChart(String),
}

pub enum CollectionUpdate {
    Unchanged,
    Updated {
        sync_task: Option<Task<Result<(Collection, bool)>>>,
        add: bool,
    },
}

#[derive(Clone, Serialize, Deserialize)]
pub struct LocalCollection {
    pub id: Option<i32>,
    pub owner: Option<Ptr<User>>,
    pub cover: CollectionCover,
    pub name: String,
    pub description: String,
    pub remote_updated: Option<DateTime<Utc>>,
    pub charts: Vec<ChartRef>,
    #[serde(default)]
    pub public: bool,
    pub is_default: bool,
}
impl LocalCollection {
    pub fn new(name: String) -> Self {
        Self {
            id: None,
            owner: None,
            cover: CollectionCover::Unset,
            name,
            description: String::new(),
            remote_updated: None,
            charts: Vec::new(),
            public: false,
            is_default: false,
        }
    }

    pub fn cover(&self) -> Illustration {
        let mut cover = self.cover.clone();
        if matches!(cover, CollectionCover::Unset) {
            if let Some(chart) = self.charts.first() {
                if let Ok(Some(local_path)) = chart.find_local_path() {
                    cover = CollectionCover::LocalChart(local_path.into_owned());
                } else if let Some(info) = &chart.info {
                    cover = CollectionCover::Online(info.illustration.clone());
                }
            }
        }
        match cover {
            CollectionCover::Unset => Illustration::from_done(BLACK_TEXTURE.clone()),
            CollectionCover::Online(file) => Illustration::from_file_thumbnail(file),
            CollectionCover::LocalChart(path) => local_illustration(path, BLACK_TEXTURE.clone(), false),
        }
    }

    pub fn is_owned(&self) -> bool {
        self.id.is_none()
            || self
                .owner
                .as_ref()
                .is_some_and(|it| get_data().me.as_ref().is_some_and(|me| me.id == it.id))
    }

    pub fn merge(&self, col: &Collection) -> Self {
        assert_eq!(self.id, Some(col.id));
        Self {
            id: Some(col.id),
            owner: Some(col.owner.clone()),
            cover: match &col.cover {
                None => CollectionCover::Unset,
                Some(file) => CollectionCover::Online(file.clone()),
            },
            name: col.name.clone(),
            description: col.description.clone(),
            remote_updated: Some(col.updated),
            charts: col.charts.iter().cloned().map(Into::into).collect(),
            public: col.public,
            is_default: self.is_default,
        }
    }

    #[must_use]
    pub fn update(mut self, uuid: Uuid, charts: &[ChartRef], add: bool) -> CollectionUpdate {
        let data = get_data();
        if self.id.is_some() && charts.iter().any(|it| !it.is_online()) {
            let dir = dir::charts().unwrap();
            let charts: Vec<_> = charts
                .iter()
                .filter(|it| !it.is_online())
                .filter_map(|it| {
                    let path = format!("{dir}/{}/info.yml", it.path);
                    let info = std::fs::read_to_string(path).ok()?;
                    serde_yaml::from_str::<ChartInfo>(&info).ok().map(|info| info.name)
                })
                .collect();
            Dialog::simple(ttl!("favorites-online-only", "charts" => charts.join(", "))).show();
            return CollectionUpdate::Unchanged;
        }

        let should_upload = self.id.is_some() && !get_data().config.offline_mode;
        let mut updated = false;
        if add {
            let local_paths: HashSet<String> = self.charts.iter().map(|it| it.path.clone()).collect();
            for chart in charts {
                if !local_paths.contains(&chart.path) {
                    self.charts.push(chart.clone());
                    updated = true;
                }
            }
        } else {
            let to_remove: HashSet<ChartRef> = charts.iter().cloned().collect();
            self.charts.retain(|it| {
                if to_remove.contains(it) {
                    updated = true;
                    false
                } else {
                    true
                }
            });
        }
        if !updated {
            return CollectionUpdate::Unchanged;
        }

        let id = self.id;
        let col_ids = self.charts.iter().filter_map(|it| it.id()).collect::<Vec<_>>();
        data.set_collection_info(&uuid, self).unwrap();
        if !should_upload {
            return CollectionUpdate::Updated { sync_task: None, add };
        }

        CollectionUpdate::Updated {
            sync_task: Some(Task::new(async move {
                let resp: Collection =
                    recv_raw(Client::request(Method::PATCH, format!("/collection/{}", id.unwrap())).json(&CollectionPatch::Set(col_ids)))
                        .await?
                        .json()
                        .await?;
                Ok((resp, add))
            })),
            add,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CollectionPatch {
    Set(Vec<i32>),
    Public(bool),
    Cover(i32),
}

#[derive(Serialize)]
pub struct CollectionContent {
    pub name: String,
    pub description: String,
    pub charts: Vec<i32>,
    pub public: bool,
}
