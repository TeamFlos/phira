use super::{PZChart, PZObject, PZUser, Ptr};
use chrono::{DateTime, Utc};
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub struct PZRecord {
    pub id: i32,
    pub player: Ptr<PZUser>,
    pub chart: Ptr<PZChart>,
    pub score: i32,
    pub accuracy: f32,
    pub perfect: i32,
    pub good: i32,
    pub bad: i32,
    pub miss: i32,
    pub speed: f32,
    pub max_combo: i32,
    pub best: bool,
    pub mods: i32,
    pub time: DateTime<Utc>,
}
impl PZObject for PZRecord {
    const QUERY_PATH: &'static str = "records";

    fn id(&self) -> i32 {
        self.id
    }
}
