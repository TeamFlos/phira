use crate::{dir, scene::ChartEntity};
use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use log::info;
use prpr::{ext::unzip_into, info::ChartInfo};
use reqwest::{RequestBuilder, Response};
use std::{
    io::{Cursor, Write},
    path::Path,
};
use uuid::Uuid;

pub async fn recv_raw(request: RequestBuilder) -> Result<Response> {
    let response = request.send().await?;
    if !response.status().is_success() {
        let status = response.status().as_str().to_owned();
        let text = response.text().await.context("failed to receive text")?;
        if let Ok(what) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(detail) = what["error"].as_str() {
                bail!("request failed ({status}): {detail}");
            }
        }
        bail!("request failed ({status}): {text}");
    }
    Ok(response)
}

pub async fn download(entity: ChartEntity, token: String) -> Result<String> {
    let path = format!("{}/{}", dir::downloaded_charts()?, Uuid::new_v4());
    let path = std::path::Path::new(&path);
    tokio::fs::create_dir(path).await?;
    let dir = prpr::dir::Dir::new(path)?;

    async fn download(mut file: impl Write, url: &str, token: &str) -> Result<()> {
        let res = recv_raw(reqwest::Client::new().get(url).header("Authorization", format!("Bearer {token}")))
            .await
            .context("请求失败")?;
        info!("已连接");
        let size = res.content_length();
        let mut stream = res.bytes_stream();
        let mut count = 0;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            file.write_all(&chunk)?;
            count += chunk.len() as u64;
            if let Some(size) = size {
                info!("下载：{:.2}%", count.min(size) as f32 / size as f32 * 100.);
            }
        }
        Ok(())
    }

    info!("下载中…");
    let mut bytes = Vec::new();
    download(Cursor::new(&mut bytes), &entity.file, &token).await?;

    info!("提取中…");
    unzip_into(Cursor::new(bytes), &dir, false)?;

    info!("保存中…");
    let mut info: ChartInfo = serde_yaml::from_reader(dir.open("info.yml")?)?;
    info.id = Some(entity.id);
    info.created = Some(entity.created);
    info.updated = Some(entity.updated);
    info.chart_updated = Some(entity.chart_updated);
    info.uploader = Some(entity.uploader);
    serde_yaml::to_writer(dir.create("info.yml")?, &info)?;

    let local_path = format!("download/{}", entity.id);
    let to_path = format!("{}/{local_path}", dir::charts()?);
    let to_path = Path::new(&to_path);
    if to_path.exists() {
        if to_path.is_file() {
            tokio::fs::remove_file(to_path).await?;
        } else {
            tokio::fs::remove_dir_all(to_path).await?;
        }
    }
    tokio::fs::rename(path, to_path).await?;

    Ok(local_path)
}
