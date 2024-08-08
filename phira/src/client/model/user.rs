use super::{File, Object};
use crate::client::Client;
use anyhow::Result;
use bitflags::bitflags;
use chrono::{DateTime, Utc};
use image::DynamicImage;
use macroquad::prelude::Color;
use once_cell::sync::Lazy;
use prpr::{ext::SafeTexture, task::Task};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;
use tracing::warn;

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
        const DELETE_STABLE     = 0x00000100;
        const SEE_ALL_EVENTS    = 0x00000200;
        const BAN_USER          = 0x00000400;
        const SET_RANKED        = 0x00000800;
        const SET_ALL_ROLE      = 0x00001000;
        const SET_REVIEWER      = 0x00002000;
        const SET_SUPERVISOR    = 0x00004000;
        const BAN_AVATAR        = 0x00008000;
        const REVIEW_PECJAM     = 0x00010000;
    }
}

bitflags! {
    #[derive(Default, Debug, Clone, Copy)]
    pub struct Roles: i32 {
        const ADMIN             = 0x0001;
        const REVIEWER          = 0x0002;
        const SUPERVISOR        = 0x0004;
        const HEAD_SUPERVISOR   = 0x0008;
        const HEAD_REVIEWER     = 0x0010;
        const PECJAM_REVIEWER   = 0x0020;
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
        if self.contains(Self::HEAD_REVIEWER) {
            perm |= Permissions::BAN_USER;
            perm |= Permissions::SET_REVIEWER;
        }
        if self.contains(Self::SUPERVISOR) {
            perm |= Permissions::SEE_UNREVIEWED;
            perm |= Permissions::SEE_STABLE_REQ;
            perm |= Permissions::STABILIZE_CHART;
            perm |= Permissions::EDIT_TAGS;
        }
        if self.contains(Self::HEAD_SUPERVISOR) {
            perm |= Permissions::STABILIZE_JUDGE;
            perm |= Permissions::DELETE_STABLE;
            perm |= Permissions::SET_RANKED;
            perm |= Permissions::SET_SUPERVISOR;
        }
        if self.contains(Self::PECJAM_REVIEWER) {
            perm |= Permissions::REVIEW_PECJAM;
        }
        perm
    }
}

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct User {
    pub id: i32,
    pub name: String,
    pub avatar: Option<File>,
    pub badge: Option<String>,
    pub badges: Vec<String>,
    pub language: String,
    pub bio: Option<String>,
    pub exp: i64,
    pub rks: f32,
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
    pub fn perms(&self) -> Permissions {
        Roles::from_bits(self.roles).map(|it| it.perms(false)).unwrap_or_default()
    }

    pub fn has_perm(&self, perm: Permissions) -> bool {
        Roles::from_bits(self.roles).map_or(false, |it| it.perms(false).contains(perm))
    }

    pub fn name_color(&self) -> Color {
        Color::from_hex(if self.badges.iter().any(|it| it == "admin") {
            0xff673ab7
        } else if self.badges.iter().any(|it| it == "sponsor") {
            0xffff7043
        } else {
            0xffffffff
        })
    }
}

static TASKS: Lazy<Mutex<HashMap<i32, Task<Result<Option<DynamicImage>>>>>> = Lazy::new(Mutex::default);
static RESULTS: Lazy<Mutex<HashMap<i32, (String, Color, Option<Option<SafeTexture>>)>>> = Lazy::new(Mutex::default);

pub struct UserManager;

impl UserManager {
    pub fn clear_cache(id: i32) -> Result<()> {
        TASKS.blocking_lock().remove(&id);
        RESULTS.blocking_lock().remove(&id);
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
                RESULTS.lock().await.insert(id, (user.name.clone(), user.name_color(), None));
                if let Some(avatar) = &user.avatar {
                    Ok(Some(image::load_from_memory(&avatar.fetch().await?)?))
                } else {
                    Ok(None)
                }
            }),
        );
    }

    pub fn name_and_color(id: i32) -> Option<(String, Color)> {
        let names = RESULTS.blocking_lock();
        if let Some((name, color, ..)) = names.get(&id) {
            Some((name.to_owned(), *color))
        } else {
            None
        }
    }

    pub fn get_avatar(id: i32) -> Option<Option<SafeTexture>> {
        let mut guard = TASKS.blocking_lock();
        if let Some(task) = guard.get_mut(&id) {
            if let Some(result) = task.take() {
                match result {
                    Err(err) => {
                        warn!("Failed to fetch user info: {err:?}");
                        guard.remove(&id);
                    }
                    Ok(image) => {
                        RESULTS.blocking_lock().get_mut(&id).unwrap().2 = Some(image.map(|it| SafeTexture::from(it).with_mipmap()));
                    }
                }
            }
        } else {
            drop(guard);
        }
        RESULTS.blocking_lock().get(&id).and_then(|it| it.2.clone())
    }

    pub fn opt_avatar(id: i32, tex: &SafeTexture) -> Result<Option<SafeTexture>, SafeTexture> {
        Self::get_avatar(id).map(|it| it.ok_or_else(|| tex.clone())).transpose()
    }
}
