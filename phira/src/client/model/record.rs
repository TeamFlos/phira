use super::{Chart, Object, Ptr, User};
use chrono::{DateTime, Utc};
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub struct Record {
    pub id: i32,
    pub player: Ptr<User>,
    pub chart: Ptr<Chart>,
    pub score: i32,
    pub accuracy: f32,
    pub perfect: i32,
    pub good: i32,
    pub bad: i32,
    pub miss: i32,
    pub speed: f32,
    pub max_combo: i32,
    pub full_combo: bool,
    pub best: bool,
    pub mods: i32,
    pub time: DateTime<Utc>,
    pub std: Option<f32>,
    pub std_score: Option<f32>,
}
impl Object for Record {
    const QUERY_PATH: &'static str = "records";

    fn id(&self) -> i32 {
        self.id
    }
}
