use std::{
    borrow::Cow,
    hash::{Hash, Hasher},
};

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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ChartRef {
    Online(i32, Option<Box<Chart>>),
    Local(String),
}
impl ChartRef {
    pub fn local_path(&self) -> Cow<'_, str> {
        match self {
            Self::Online(id, _) => Cow::Owned(format!("download/{id}")),
            Self::Local(path) => Cow::Borrowed(path),
        }
    }

    pub fn matches(&self, path_or_id: (Option<&str>, Option<i32>)) -> bool {
        match self {
            ChartRef::Online(id, _) => path_or_id.1 == Some(*id),
            ChartRef::Local(path) => path_or_id.0 == Some(path.as_str()),
        }
    }
}

impl From<Chart> for ChartRef {
    fn from(chart: Chart) -> Self {
        ChartRef::Online(chart.id, Some(Box::new(chart)))
    }
}

impl PartialEq for ChartRef {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (ChartRef::Online(id1, _), ChartRef::Online(id2, _)) => id1 == id2,
            (ChartRef::Local(path1), ChartRef::Local(path2)) => path1 == path2,
            _ => false,
        }
    }
}
impl Eq for ChartRef {}

impl Hash for ChartRef {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            ChartRef::Online(id, _) => id.hash(state),
            ChartRef::Local(path) => path.hash(state),
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
                Some(ChartRef::Online(_, chart)) => CollectionCover::Online(chart.as_ref().unwrap().illustration.clone()),
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
            description: self.description.clone(),
            remote_updated: Some(col.updated),
            charts: col.charts.iter().cloned().map(Into::into).collect(),
            public: col.public,
            is_default: self.is_default,
        }
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
