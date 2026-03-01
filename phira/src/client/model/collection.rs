use crate::{
    client::File,
    get_data,
    page::{local_illustration, Illustration},
};

use super::{Chart, Object, Ptr, User};
use chrono::{DateTime, Utc};
use prpr::ext::BLACK_TEXTURE;
use serde::{Deserialize, Serialize};

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
#[serde(untagged)]
pub enum ChartRef {
    Online(Box<Chart>),
    Local(String),
}
impl ChartRef {
    pub fn matches(&self, id: Option<i32>, local_path: Option<&str>) -> bool {
        match self {
            ChartRef::Online(chart) => id.is_some_and(|id| chart.id == id),
            ChartRef::Local(path) => local_path.is_some_and(|local_path| path == local_path),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub enum CollectionCover {
    Unset,
    Online(File),
    LocalChart(String),
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
            cover = match self.charts.first() {
                None => CollectionCover::Unset,
                Some(ChartRef::Online(chart)) => CollectionCover::Online(chart.illustration.clone()),
                Some(ChartRef::Local(path)) => CollectionCover::LocalChart(path.clone()),
            };
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

    pub fn assign_from(&mut self, col: &Collection) {
        assert_eq!(self.id, Some(col.id));
        self.owner = Some(col.owner.clone());
        self.name = col.name.clone();
        self.description = col.description.clone();
        self.cover = match &col.cover {
            None => CollectionCover::Unset,
            Some(file) => CollectionCover::Online(file.clone()),
        };
        self.remote_updated = Some(col.updated);
        self.charts = col.charts.iter().map(|chart| ChartRef::Online(Box::new(chart.clone()))).collect();
        self.public = col.public;
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CollectionPatch {
    Toggle(i32),
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
