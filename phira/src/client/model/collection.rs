use super::{Chart, Object, Ptr, User};
use chrono::{DateTime, Utc};
use serde::Deserialize;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    pub id: i32,
    pub owner: Ptr<User>,
    pub name: String,
    pub description: String,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
    pub charts: Vec<Ptr<Chart>>,
}
impl Object for Collection {
    const QUERY_PATH: &'static str = "collection";

    fn id(&self) -> i32 {
        self.id
    }
}
