use super::{File, Object, Ptr, User};
use crate::data::BriefChartInfo;
use chrono::{DateTime, Utc};
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chart {
    pub id: i32,
    pub name: String,
    pub level: String,
    pub difficulty: f32,
    pub charter: String,
    pub composer: String,
    pub illustrator: String,
    pub description: Option<String>,
    pub ranked: bool,
    pub reviewed: bool,
    pub stable: bool,
    pub stable_request: bool,

    pub illustration: File,
    pub preview: File,
    pub file: File,

    pub uploader: Ptr<User>,

    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
    pub chart_updated: DateTime<Utc>,
    #[serde(default)]
    pub tags: Vec<String>,

    pub rating: Option<f32>,
}
impl Object for Chart {
    const QUERY_PATH: &'static str = "chart";

    fn id(&self) -> i32 {
        self.id
    }
}

impl Chart {
    pub fn to_info(&self) -> BriefChartInfo {
        BriefChartInfo {
            id: Some(self.id),
            uploader: Some(self.uploader.clone()),
            name: self.name.clone(),
            level: self.level.clone(),
            difficulty: self.difficulty,
            intro: self.description.clone().unwrap_or_default(),
            charter: self.charter.clone(),
            composer: self.composer.clone(),
            illustrator: self.illustrator.clone(),
            created: Some(self.created),
            updated: Some(self.updated),
            chart_updated: Some(self.chart_updated),
            has_unlock: false,
        }
    }
}
