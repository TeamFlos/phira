use super::{File, Object};
use crate::{client::Client, dir, images::Images};
use anyhow::Result;
use bitflags::bitflags;
use chrono::{DateTime, Utc};
use image::DynamicImage;
use macroquad::prelude::warn;
use once_cell::sync::Lazy;
use prpr::{ext::SafeTexture, task::Task};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tokio::sync::Mutex;

bitflags! {
    #[derive(Default, Debug, Clone, Copy)]
    pub struct Permissions: i64 {
        const UPLOAD_CHART      = 0x00000001;
        const SEE_UNREVIEWED    = 0x00000002;
        const DELETE_UNSTABLE   = 0x00000004;
        const REVIEW            = 0x00000008;
        const SEE_STABLE_REQ    = 0x00000010;
        const STABILIZE_CHART   = 0x00000020;
        const EDIT_TAGS         = 0x00000040;
        const STABILIZE_JUDGE   = 0x00000080;
    }
}

bitflags! {
    #[derive(Default, Debug, Clone, Copy)]
    pub struct Roles: i32 {
        const ADMIN             = 0x0001;
        const REVIEWER          = 0x0002;
        const SUPERVISOR        = 0x0004;
        const HEAD_SUPERVISOR   = 0x0008;
    }
}

impl Roles {
    pub fn perms(&self, banned: bool) -> Permissions {
        let mut perm = Permissions::empty();
        if !banned {
            perm |= Permissions::UPLOAD_CHART;
        }
        if self.contains(Self::ADMIN) {
            perm = Permissions::all();
        }
        if self.contains(Self::REVIEWER) {
            perm |= Permissions::SEE_UNREVIEWED;
            perm |= Permissions::DELETE_UNSTABLE;
            perm |= Permissions::REVIEW;
            perm |= Permissions::EDIT_TAGS;
        }
        if self.contains(Self::SUPERVISOR) {
            perm |= Permissions::SEE_UNREVIEWED;
            perm |= Permissions::SEE_STABLE_REQ;
            perm |= Permissions::STABILIZE_CHART;
            perm |= Permissions::EDIT_TAGS;
        }
        if self.contains(Self::HEAD_SUPERVISOR) {
            perm |= Permissions::STABILIZE_JUDGE;
        }
        perm
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct User {
    pub id: i32,
    pub name: String,
    pub avatar: Option<File>,
    pub badge: Option<String>,
    pub language: String,
    pub bio: Option<String>,
    pub exp: i64,
    pub rks: f32,
    #[serde(default)]
    pub roles: i32,

    pub joined: DateTime<Utc>,
    pub last_login: DateTime<Utc>,
}
impl Object for User {
    const QUERY_PATH: &'static str = "user";

    fn id(&self) -> i32 {
        self.id
    }
}
impl User {
    pub fn has_perm(&self, perm: Permissions) -> bool {
        Roles::from_bits(self.roles).map_or(false, |it| it.perms(false).contains(perm))
    }
}

static TASKS: Lazy<Mutex<HashMap<i32, Task<Result<Option<DynamicImage>>>>>> = Lazy::new(Mutex::default);
static RESULTS: Lazy<Mutex<HashMap<i32, (String, Option<Option<SafeTexture>>)>>> = Lazy::new(Mutex::default);

pub struct UserManager;

impl UserManager {
    fn cache_path(id: i32) -> Result<PathBuf> {
        Ok(format!("{}/{id}", dir::cache_avatar()?).into())
    }

    pub fn clear_cache(id: i32) -> Result<()> {
        TASKS.blocking_lock().remove(&id);
        RESULTS.blocking_lock().remove(&id);
        let path = Self::cache_path(id)?;
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }

    pub fn request(id: i32) {
        let mut tasks = TASKS.blocking_lock();
        if tasks.contains_key(&id) {
            return;
        }
        tasks.insert(
            id,
            Task::new(async move {
                let user: Arc<User> = Client::load(id).await?;
                RESULTS.lock().await.insert(id, (user.name.clone(), None));
                if let Some(avatar) = &user.avatar {
                    let image =
                        Images::local_or_else(Self::cache_path(id)?, async move { Ok(image::load_from_memory(&avatar.fetch().await?)?) }).await?;
                    Ok(Some(image))
                } else {
                    Ok(None)
                }
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

    pub fn get_avatar(id: i32) -> Option<Option<SafeTexture>> {
        let mut guard = TASKS.blocking_lock();
        if let Some(task) = guard.get_mut(&id) {
            if let Some(result) = task.take() {
                match result {
                    Err(err) => {
                        warn!("Failed to fetch user info: {:?}", err);
                        guard.remove(&id);
                    }
                    Ok(image) => {
                        RESULTS.blocking_lock().get_mut(&id).unwrap().1 = Some(image.map(|it| SafeTexture::from(it).with_mipmap()));
                    }
                }
            }
        } else {
            drop(guard);
        }
        RESULTS.blocking_lock().get(&id).and_then(|it| it.1.clone())
    }

    pub fn opt_avatar(id: i32, tex: &SafeTexture) -> Result<Option<SafeTexture>, SafeTexture> {
        Self::get_avatar(id).map(|it| it.ok_or_else(|| tex.clone())).transpose()
    }
}
