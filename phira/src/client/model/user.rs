use crate::client::Client;

use super::{PZFile, PZObject};
use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use image::DynamicImage;
use macroquad::prelude::warn;
use once_cell::sync::Lazy;
use prpr::{ext::SafeTexture, task::Task};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

#[derive(Debug, Serialize, Deserialize, Copy, Clone)]
#[repr(u8)]
#[serde(rename_all = "lowercase")]
pub enum PZUserRole {
    Banned = 0,
    Member,
    Qualified,
    Volunteer,
    Admin,
}

impl PZUserRole {
    pub fn priority(&self) -> u8 {
        *self as u8
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PZUser {
    pub id: i32,
    pub name: String,
    pub avatar: Option<PZFile>,
    pub badge: Option<String>,
    pub language: String,
    pub bio: Option<String>,
    pub exp: i64,
    pub rks: f32,

    pub joined: DateTime<Utc>,
    pub last_login: DateTime<Utc>,
}
impl PZObject for PZUser {
    const QUERY_PATH: &'static str = "user";

    fn id(&self) -> i32 {
        self.id
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PZUserExtra {}

static TASKS: Lazy<Mutex<HashMap<i32, Task<Result<DynamicImage>>>>> = Lazy::new(Mutex::default);
static RESULTS: Lazy<Mutex<HashMap<i32, (String, Option<SafeTexture>)>>> = Lazy::new(Mutex::default);

pub struct UserManager;

impl UserManager {
    pub fn clear_cache(id: i32) {
        RESULTS.blocking_lock().remove(&id);
    }

    pub fn request(id: i32) {
        let mut tasks = TASKS.blocking_lock();
        if tasks.contains_key(&id) {
            return;
        }
        tasks.insert(
            id,
            Task::new(async move {
                let user: Arc<PZUser> = Client::load(id).await?;
                RESULTS.lock().await.insert(id, (user.name.clone(), None));
                let image = user.avatar.clone().ok_or_else(|| anyhow!("no avatar"))?.fetch().await?;
                let image = image::load_from_memory(&image)?;
                Ok(image)
            }),
        );
    }

    pub fn get_name(id: i32) -> Option<String> {
        let names = RESULTS.blocking_lock();
        if let Some((name, _)) = names.get(&id) {
            return Some(name.clone());
        }
        None
    }

    pub fn get_avatar(id: i32) -> Option<SafeTexture> {
        let mut guard = TASKS.blocking_lock();
        if let Some(task) = guard.get_mut(&id) {
            if let Some(result) = task.take() {
                match result {
                    Err(err) => {
                        warn!("Failed to fetch user info: {:?}", err);
                        guard.remove(&id);
                    }
                    Ok(image) => {
                        RESULTS.blocking_lock().get_mut(&id).unwrap().1 = Some(SafeTexture::from(image).with_mipmap());
                    }
                }
            }
        } else {
            drop(guard);
        }
        RESULTS.blocking_lock().get(&id).and_then(|it| it.1.clone())
    }
}
