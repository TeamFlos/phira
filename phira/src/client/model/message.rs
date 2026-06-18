use chrono::{DateTime, Utc};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct MessageAction {
    pub name: String,
    pub action: String,
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct Message {
    pub id: i32,
    pub title: String,
    pub content: String,
    pub author: String,
    pub time: DateTime<Utc>,
    #[serde(default)]
    pub actions: Vec<MessageAction>,
}
