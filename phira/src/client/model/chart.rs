use super::{File, Object, Ptr, User};
use crate::data::BriefChartInfo;
use chrono::{DateTime, Utc};
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
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

    pub illustration: File,
    pub preview: File,
    pub file: File,

    pub uploader: Ptr<User>,

    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
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
        }
    }
}
