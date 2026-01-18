//! File system abstraction

use crate::{ext::spawn_task, info::ChartInfo};
use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use chardetng::EncodingDetector;
use concat_string::concat_string;
use macroquad::prelude::load_file;
use serde::Deserialize;
use serde_json::Value;
use std::{
    any::Any,
    collections::HashMap,
    fs,
    io::{Cursor, Read, Seek, Write},
    path::Path,
    sync::{Arc, Mutex},
};
use tracing::warn;
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};

pub fn update_zip<R: Read + Seek>(zip: &mut ZipArchive<R>, patches: HashMap<String, Vec<u8>>) -> Result<Vec<u8>> {
    let mut buffer = Vec::new();
    let mut w = ZipWriter::new(Cursor::new(&mut buffer));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o755);
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).unwrap();
        let path = match entry.enclosed_name() {
            Some(path) => path.to_owned(),
            None => continue,
        };
        let path = path.display().to_string();
        if entry.is_dir() {
            w.add_directory(path, options)?;
        } else if !patches.contains_key(&path) {
            w.start_file(&path, options)?;
            std::io::copy(&mut entry, &mut w)?;
        }
    }
    for (path, data) in patches.into_iter() {
        w.start_file(path, options)?;
        w.write_all(&data)?;
    }
    w.finish()?;
    Ok(buffer)
}

#[async_trait]
pub trait FileSystem: Send {
    async fn load_file(&mut self, path: &str) -> Result<Vec<u8>>;
    async fn exists(&mut self, path: &str) -> Result<bool>;
    fn list_root(&self) -> Result<Vec<String>>;
    fn clone_box(&self) -> Box<dyn FileSystem>;
    fn as_any(&mut self) -> &mut dyn Any;
}

#[derive(Clone)]
pub struct AssetsFileSystem(String);

#[async_trait]
impl FileSystem for AssetsFileSystem {
    async fn load_file(&mut self, path: &str) -> Result<Vec<u8>> {
        Ok(load_file(&concat_string!(self.0, path)).await?)
    }

    async fn exists(&mut self, path: &str) -> Result<bool> {
        // unlikely to be called
        Ok(load_file(&concat_string!(self.0, path)).await.is_ok())
    }

    fn list_root(&self) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    fn clone_box(&self) -> Box<dyn FileSystem> {
        Box::new(self.clone())
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

#[derive(Clone)]
pub struct ExternalFileSystem(pub Arc<crate::dir::Dir>);

#[async_trait]
impl FileSystem for ExternalFileSystem {
    async fn load_file(&mut self, path: &str) -> Result<Vec<u8>> {
        #[cfg(target_arch = "wasm32")]
        {
            unimplemented!("cannot use external file system on wasm32")
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut file = self.0.open(path)?;
            tokio::task::spawn_blocking(move || {
                let mut res = Vec::new();
                file.read_to_end(&mut res)?;
                Ok(res)
            })
            .await?
        }
    }

    async fn exists(&mut self, path: &str) -> Result<bool> {
        self.0.exists(path)
    }

    fn list_root(&self) -> Result<Vec<String>> {
        Ok(self.0.read_dir(".")?.filter_map(|res| res.ok()?.file_name().into_string().ok()).collect())
    }

    fn clone_box(&self) -> Box<dyn FileSystem> {
        Box::new(self.clone())
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

#[derive(Clone)]
pub struct ZipFileSystem(pub Arc<Mutex<ZipArchive<Cursor<Vec<u8>>>>>, String);

impl ZipFileSystem {
    pub fn new(bytes: Vec<u8>) -> Result<Self> {
        let zip = ZipArchive::new(Cursor::new(bytes))?;
        let root_dirs = zip
            .file_names()
            .filter(|it| it.ends_with('/') && it.find('/') == Some(it.len() - 1))
            .collect::<Vec<_>>();
        let root = if root_dirs.len() == 1 { root_dirs[0].to_owned() } else { String::new() };
        Ok(Self(Arc::new(Mutex::new(zip)), root))
    }
}

#[async_trait]
impl FileSystem for ZipFileSystem {
    async fn load_file(&mut self, path: &str) -> Result<Vec<u8>> {
        let arc = Arc::clone(&self.0);
        let path = concat_string!(self.1, path);
        spawn_task(move || {
            let mut zip = arc.lock().unwrap();
            let mut entry = zip.by_name(&path)?;
            let mut res = Vec::new();
            entry.read_to_end(&mut res)?;
            Ok(res)
        })
        .await
    }

    async fn exists(&mut self, path: &str) -> Result<bool> {
        Ok(self.0.lock().unwrap().by_name(&concat_string!(self.1, path)).is_ok())
    }

    fn list_root(&self) -> Result<Vec<String>> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .file_names()
            .filter(|it| it.strip_prefix(&self.1).is_some_and(|it| !it.contains('/')))
            .map(str::to_owned)
            .collect())
    }

    fn clone_box(&self) -> Box<dyn FileSystem> {
        Box::new(self.clone())
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

pub struct PatchedFileSystem(pub Box<dyn FileSystem>, pub HashMap<String, Vec<u8>>);

#[async_trait]
impl FileSystem for PatchedFileSystem {
    async fn load_file(&mut self, path: &str) -> Result<Vec<u8>> {
        if let Some(data) = self.1.get(path) {
            Ok(data.clone())
        } else {
            self.0.load_file(path).await
        }
    }

    async fn exists(&mut self, path: &str) -> Result<bool> {
        Ok(self.0.exists(path).await? || self.1.contains_key(path))
    }

    fn list_root(&self) -> Result<Vec<String>> {
        let mut res = self.0.list_root()?;
        res.extend(self.1.keys().cloned());
        res.dedup();
        Ok(res)
    }

    fn clone_box(&self) -> Box<dyn FileSystem> {
        unimplemented!()
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

fn infer_diff(info: &mut ChartInfo, level: &str) {
    if let Ok(val) = level
        .chars()
        .rev()
        .take_while(|it| it.is_ascii_digit())
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>()
        .parse::<u32>()
    {
        info.difficulty = val as f32;
    }
}

fn info_from_kv<'a>(it: impl Iterator<Item = (&'a str, String)>, csv: bool) -> Result<ChartInfo> {
    let mut info = ChartInfo::default();
    for (key, value) in it {
        if csv && value.trim().is_empty() {
            continue;
        }
        let key = key.trim();
        if key == "Path" {
            continue;
        }
        if key == "Level" {
            infer_diff(&mut info, &value);
        }
        if key == "AspectRatio" {
            info.aspect_ratio = value.parse().context("invalid aspect ratio")?;
            continue;
        }
        if key == "BackgroundDim" {
            info.background_dim = value.parse().context("invalid background dim")?;
            continue;
        }
        if key == "NoteScale" || key == "ScaleRatio" {
            warn!("note scale is ignored");
            continue;
        }
        if key == "GlobalAlpha" {
            warn!("global alpha is ignored");
            continue;
        }
        let mut deprecate = String::new();
        *match key {
            "Name" => &mut info.name,
            "Music" | "Song" => &mut info.music,
            "Chart" => &mut info.chart,
            "Image" | "Picture" => &mut info.illustration,
            "Level" => &mut info.level,
            "Illustrator" => &mut info.illustrator,
            "Artist" | "Composer" | "Musician" => &mut info.composer,
            "Charter" | "Designer" => &mut info.charter,
            "LastEditTime" => &mut deprecate,
            "Length" => &mut deprecate,
            "EditTime" => &mut deprecate,
            _ => &mut deprecate,
        } = value;
    }
    Ok(info)
}

fn info_from_txt(text: &str) -> Result<ChartInfo> {
    let mut it = text.lines().filter(|it| !it.is_empty()).peekable();
    let first = it.next();
    if first != Some("#") && first != Some("\u{feff}#") {
        bail!("expected the first line to be #");
    }
    let kvs = it
        .map(|line| -> Result<(&str, String)> {
            let Some((key, value)) = line.split_once(": ") else {
                bail!("expected \"Key: Value\"");
            };
            Ok((key, value.to_string()))
        })
        .collect::<Result<Vec<_>>>()?;
    info_from_kv(kvs.into_iter(), false)
}

fn info_from_csv(text: &str) -> Result<ChartInfo> {
    let mut reader = csv::ReaderBuilder::new().flexible(true).from_reader(Cursor::new(text));
    // shitty design
    let headers = reader.headers()?.iter().map(str::to_owned).collect::<Vec<_>>();
    let record = reader.into_records().last().ok_or_else(|| anyhow!("expected csv records"))??; // ??
    info_from_kv(headers.iter().zip(&record).map(|(key, value)| (key.as_str(), value.to_owned())), true)
}

pub async fn fix_info(fs: &mut dyn FileSystem, info: &mut ChartInfo) -> Result<()> {
    async fn get(fs: &mut dyn FileSystem, path: &mut String) -> Result<Option<String>> {
        Ok(if fs.exists(path).await? { Some(std::mem::take(path)) } else { None })
    }
    let mut chart = get(fs, &mut info.chart).await?;
    let mut music = get(fs, &mut info.music).await?;
    let mut illustration = get(fs, &mut info.illustration).await?;
    fn put(desc: &str, status: &mut Option<String>, value: String) {
        if status.as_ref() == Some(&value) {
            return;
        }
        if status.is_some() {
            warn!("found multiple {}, using the first one", desc);
        } else {
            *status = Some(value);
        }
    }
    for file in fs.list_root().context("cannot list files")? {
        if let Some((_, ext)) = file.rsplit_once('.') {
            match ext.to_ascii_lowercase().as_str() {
                "json" | "pec" => {
                    put("charts", &mut chart, file);
                }
                _ => {}
            }
        }
    }
    if let Some(chart) = &chart {
        info.chart = chart.to_owned();
        if let Ok(s) = String::from_utf8(fs.load_file(&info.chart).await?) {
            if let Ok(mut value) = serde_json::from_str::<Value>(&s) {
                #[derive(Deserialize)]
                struct RPEMeta {
                    name: String,
                    level: String,
                    background: String,
                    charter: String,
                    composer: Option<String>,
                    illustration: Option<String>,
                    song: String,
                }
                if let Ok(mut meta) = serde_json::from_value::<RPEMeta>(value["META"].take()) {
                    info.name = meta.name;
                    infer_diff(info, &meta.level);
                    info.level = meta.level;
                    info.charter = meta.charter;
                    if let Some(val) = meta.composer {
                        info.composer = val;
                    }
                    if let Some(val) = meta.illustration {
                        info.illustrator = val;
                    }
                    if illustration.is_none() {
                        illustration = get(fs, &mut meta.background).await?;
                    }
                    if music.is_none() {
                        music = get(fs, &mut meta.song).await?;
                    }
                }
            }
        }
    } else {
        bail!("cannot find chart");
    }
    for file in fs.list_root().context("cannot list files")? {
        if let Some((_, ext)) = file.rsplit_once('.') {
            match ext.to_ascii_lowercase().as_str() {
                "mp3" | "ogg" | "wav" | "flac" | "aac" => {
                    put("music files", &mut music, file);
                }
                "png" | "jpg" | "jpeg" | "bmp" | "gif" | "webp" | "avif" | "ppm" => {
                    put("illustrations", &mut illustration, file);
                }
                _ => {}
            }
        }
    }
    if let Some(music) = music {
        info.music = music;
    }
    if let Some(illustration) = illustration {
        info.illustration = illustration;
    }
    Ok(())
}

fn bytes_to_text_auto(data: &[u8]) -> String {
    let mut det = EncodingDetector::new();
    det.feed(data, true);
    let encoding = det.guess(None, true);
    let (s, _, _) = encoding.decode(data);
    s.into_owned()
}

pub async fn load_info(fs: &mut dyn FileSystem) -> Result<ChartInfo> {
    let info = if let Ok(bytes) = fs.load_file(":info").await {
        serde_yaml::from_str(&bytes_to_text_auto(&bytes))?
    } else if let Ok(bytes) = fs.load_file("info.yml").await {
        serde_yaml::from_str(&bytes_to_text_auto(&bytes))?
    } else if let Ok(bytes) = fs.load_file("info.txt").await {
        info_from_txt(&bytes_to_text_auto(&bytes))?
    } else if let Ok(bytes) = fs.load_file("info.csv").await {
        info_from_csv(&bytes_to_text_auto(&bytes))?
    } else {
        warn!("none of info.yml, info.txt and info.csv is found, inferring");
        let mut info = ChartInfo::default();
        fix_info(fs, &mut info).await?;
        info
    };
    Ok(info)
}

pub fn fs_from_file(path: &Path) -> Result<Box<dyn FileSystem + Send + Sync + 'static>> {
    let meta = fs::metadata(path)?;
    Ok(if meta.is_file() {
        let bytes = fs::read(path).with_context(|| format!("failed to read from {}", path.display()))?;
        Box::new(ZipFileSystem::new(bytes).with_context(|| format!("cannot open {} as zip archive", path.display()))?)
    } else {
        Box::new(ExternalFileSystem(Arc::new(crate::dir::Dir::new(path)?)))
    })
}

pub fn fs_from_assets(name: impl Into<String>) -> Result<Box<dyn FileSystem + Send + Sync + 'static>> {
    Ok(Box::new(AssetsFileSystem(name.into())))
}
