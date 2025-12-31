mod chart;
pub use chart::*;

mod collection;
pub use collection::*;

mod event;
pub use event::*;

mod message;
pub use message::*;

mod record;
pub use record::*;

mod user;
pub use user::*;

use super::{basic_client_builder, Client, API_URL, CLIENT_TOKEN};
use crate::{
    dir, get_data,
    images::{THUMBNAIL_HEIGHT, THUMBNAIL_WIDTH},
    ttl,
};
use anyhow::{bail, Result};
use bytes::Bytes;
use image::DynamicImage;
use lru::LruCache;
use once_cell::sync::Lazy;
use reqwest::Response;
use serde::{de::DeserializeOwned, Deserialize, Serialize, Serializer};
use std::{
    any::Any,
    collections::HashMap,
    marker::PhantomData,
    sync::{Arc, Mutex},
};
use tracing::debug;

pub(crate) type ObjectMap<T> = LruCache<i32, Arc<T>>;
static CACHES: Lazy<Mutex<HashMap<&'static str, Arc<Mutex<Box<dyn Any + Send + Sync>>>>>> = Lazy::new(Mutex::default);

pub(crate) fn obtain_map_cache<T: Object + 'static>() -> Arc<Mutex<Box<dyn Any + Send + Sync>>> {
    let mut caches = CACHES.lock().unwrap();
    Arc::clone(
        caches
            .entry(T::QUERY_PATH)
            .or_insert_with(|| Arc::new(Mutex::new(Box::new(ObjectMap::<T>::new(64.try_into().unwrap()))))),
    )
}

pub trait Object: Clone + DeserializeOwned + Send + Sync {
    const QUERY_PATH: &'static str;

    fn id(&self) -> i32;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(try_from = "String")]
#[serde(into = "String")]
pub struct MusicPosition {
    pub seconds: u32,
}
impl TryFrom<String> for MusicPosition {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let seconds = || -> Option<u32> {
            let mut it = value.splitn(3, ':');
            let mut res = it.next()?.parse::<u32>().ok()?;
            res = res * 60 + it.next()?.parse::<u32>().ok()?;
            res = res * 60 + it.next()?.parse::<u32>().ok()?;
            Some(res)
        }()
        .ok_or("illegal position")?;
        Ok(MusicPosition { seconds })
    }
}
impl From<MusicPosition> for String {
    fn from(value: MusicPosition) -> Self {
        format!("00:00:{:02}", value.seconds)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(try_from = "u8")]
#[repr(u8)]
pub enum LevelType {
    EZ = 0,
    HD,
    IN,
    AT,
    SP,
}
impl TryFrom<u8> for LevelType {
    type Error = String;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        use LevelType::*;
        Ok(match value {
            0 => EZ,
            1 => HD,
            2 => IN,
            3 => AT,
            4 => SP,
            x => {
                return Err(format!("illegal level type: {x}"));
            }
        })
    }
}

#[derive(Debug)]
pub struct Ptr<T> {
    pub id: i32,
    _marker: PhantomData<T>,
}
impl<T: Object + 'static> Clone for Ptr<T> {
    fn clone(&self) -> Self {
        Self::new(self.id)
    }
}
impl<T: Object + 'static> From<i32> for Ptr<T> {
    fn from(value: i32) -> Self {
        Self::new(value)
    }
}

impl<T: Object + 'static> Ptr<T> {
    pub fn new(id: i32) -> Self {
        Self { id, _marker: PhantomData }
    }

    #[inline]
    pub async fn fetch(&self) -> Result<Arc<T>> {
        Client::fetch(self.id).await
    }

    #[inline]
    pub async fn fetch_opt(&self) -> Result<Option<Arc<T>>> {
        Client::fetch_opt(self.id).await
    }

    pub async fn load(&self) -> Result<Arc<T>> {
        // sync locks can not be held accross await point
        {
            let map = obtain_map_cache::<T>();
            let mut guard = map.lock().unwrap();
            let Some(actual_map) = guard.downcast_mut::<ObjectMap<T>>() else {
                unreachable!()
            };
            if let Some(value) = actual_map.get(&self.id) {
                return Ok(Arc::clone(value));
            }
            drop(guard);
            drop(map);
        }
        self.fetch().await
    }
}
impl<T: Object + 'static> Serialize for Ptr<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_i32(self.id)
    }
}
impl<'de, T: Object + 'static> Deserialize<'de> for Ptr<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        i32::deserialize(deserializer).map(Self::new)
    }
}

pub static CACHE_DIR: Lazy<String> = Lazy::new(|| format!("{}/http-cache", dir::cache().unwrap_or_else(|_| ".".to_owned())));

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct File {
    pub url: String,
}
impl File {
    fn request(&self) -> reqwest::RequestBuilder {
        let mut req = basic_client_builder().build().unwrap().get(&self.url);
        // TODO: thread safety?
        if get_data().enable_anys {
            if let Some(path) = self.url.strip_prefix(API_URL) {
                if let Some(rest_path) = path.strip_prefix("/files/") {
                    let url = format!("{API_URL}/anys/{rest_path}");
                    req = basic_client_builder().build().unwrap().get(url);
                }
            }
        }
        if let Some(token) = CLIENT_TOKEN.load().as_ref() {
            req.header("Authorization", format!("Bearer {token}"))
        } else {
            req
        }
    }

    pub async fn fetch(&self) -> Result<Bytes> {
        async fn fetch_raw(f: &File) -> Result<Response> {
            Ok(f.request().send().await?)
        }
        match cacache::read(&*CACHE_DIR, &self.url).await {
            Ok(data) => Ok(data.into()),
            Err(cacache::Error::EntryNotFound(..)) => {
                let mut resp = fetch_raw(self).await?;
                if resp.status().is_redirection() {
                    let p2p_url = resp.headers().get("location").unwrap().to_str().unwrap().to_owned();
                    if let Some(cid) = p2p_url.strip_prefix("anys://") {
                        let cid = cid.to_owned();
                        let data = get_data();
                        let new_url = format!("{}/{}", data.anys_gateway, cid);
                        debug!("p2p redirection: {} -> {}", p2p_url, new_url);
                        resp = fetch_raw(&File { url: new_url }).await?
                    } else {
                        bail!("illegal p2p redirection: {}", p2p_url);
                    }
                }
                if !resp.status().is_success() {
                    bail!("{}", resp.text().await?);
                } else {
                    let bytes = resp.error_for_status()?.bytes().await?;
                    cacache::write(&*CACHE_DIR, &self.url, &bytes).await?;
                    Ok(bytes)
                }
            }
            Err(err) => Err(err.into()),
        }
    }

    pub async fn load_image(&self) -> Result<DynamicImage> {
        Ok(image::load_from_memory(&self.fetch().await?)?)
    }

    pub async fn load_thumbnail(&self) -> Result<DynamicImage> {
        if self.url.starts_with("https://phira.mivik.cn/") {
            File {
                url: format!("{}?imageView/0/w/{THUMBNAIL_WIDTH}/h/{THUMBNAIL_HEIGHT}", self.url),
            }
            .load_image()
            .await
        } else if self.url.starts_with("https://files.phira.cn/") || self.url.starts_with("https://phira.5wyxi.com/files/") {
            File {
                url: format!("{}.thumbnail", self.url),
            }
            .load_image()
            .await
        } else {
            self.load_image().await
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Character {
    pub id: String,
    pub name: String,
    pub intro: String,
    pub illust: String,
    pub artist: String,
    pub designer: String,

    #[serde(default)]
    pub name_size: Option<f32>,

    #[serde(default)]
    pub baseline: bool,

    #[serde(default)]
    pub illu_adjust: (f32, f32, f32, f32),

    #[serde(skip)]
    name_en: Option<String>,
}
impl Default for Character {
    fn default() -> Self {
        Self {
            id: "shee".to_owned(),
            name: ttl!("main-character-name").into_owned(),
            intro: ttl!("main-character-intro").into_owned(),
            illust: "@".to_owned(),
            artist: "清水QR".to_owned(),
            designer: "清水QR".to_owned(),

            name_size: None,

            baseline: false,

            illu_adjust: (0., 0., 0., 0.),

            name_en: None,
        }
    }
}
impl Character {
    pub fn name_en(&mut self) -> &str {
        if self.name_en.is_none() {
            let words = self.id.split('_');
            let mut name_en = String::new();
            for word in words {
                let (first, rest) = word.split_at(1);
                name_en.push_str(&first.to_uppercase());
                name_en.push_str(rest);
                name_en.push(' ');
            }
            if !name_en.is_empty() {
                name_en.pop();
            }
            self.name_en = Some(name_en);
        }
        self.name_en.as_ref().unwrap()
    }
}
