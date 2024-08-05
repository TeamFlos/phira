use super::{File, Object, Ptr, User};
use chrono::{DateTime, Utc};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub id: i32,
    pub creator: Ptr<User>,
    pub name: String,
    pub illustration: File,
    pub time_start: DateTime<Utc>,
    pub time_end: DateTime<Utc>,
}
impl Object for Event {
    const QUERY_PATH: &'static str = "event";

    fn id(&self) -> i32 {
        self.id
    }
}
