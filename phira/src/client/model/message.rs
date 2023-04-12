use chrono::{DateTime, Utc};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Message {
    pub id: i32,
    pub title: String,
    pub content: String,
    pub author: String,
    pub time: DateTime<Utc>,
}
