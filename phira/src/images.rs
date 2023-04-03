use anyhow::{Context, Result};
use image::imageops::thumbnail;
use image::DynamicImage;
use prpr::ext::SafeTexture;
use std::future::Future;
use std::path::Path;

pub const THUMBNAIL_WIDTH: u32 = 347;
pub const THUMBNAIL_HEIGHT: u32 = 200;

pub struct Images;
impl Images {
    pub fn into_texture(tex: (DynamicImage, Option<DynamicImage>)) -> (SafeTexture, SafeTexture) {
        match tex {
            (thumb, Some(full)) => (thumb.into(), full.into()),
            (thumb, None) => {
                let tex: SafeTexture = thumb.into();
                (tex.clone(), tex)
            }
        }
    }

    pub fn thumbnail(image: &DynamicImage) -> DynamicImage {
        let width = (image.width() as f32 / image.height() as f32 * THUMBNAIL_HEIGHT as f32).ceil() as u32;
        DynamicImage::ImageRgba8(thumbnail(image, width, THUMBNAIL_HEIGHT))
    }

    pub async fn local_or_else(path: impl AsRef<Path>, task: impl Future<Output = Result<DynamicImage>>) -> Result<DynamicImage> {
        let path = path.as_ref();
        Ok(if path.exists() {
            image::load_from_memory(&tokio::fs::read(path).await.context("Failed to read image")?)?
        } else {
            let image = task.await?;
            image.save_with_format(path, image::ImageFormat::Jpeg).context("Failed to save image")?;
            image
        })
    }
}
