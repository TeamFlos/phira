use crate::scene::TEX_ICON_BACK;
use anyhow::Result;
use macroquad::texture::load_texture;
use prpr::ext::SafeTexture;

pub struct Icons {
    pub icon: SafeTexture,
    pub play: SafeTexture,
    pub medal: SafeTexture,
    pub respack: SafeTexture,
    pub msg: SafeTexture,
    pub settings: SafeTexture,
    pub back: SafeTexture,
    pub lang: SafeTexture,
    pub download: SafeTexture,
    pub user: SafeTexture,
    pub info: SafeTexture,
    pub delete: SafeTexture,
    pub menu: SafeTexture,
    pub edit: SafeTexture,
    pub ldb: SafeTexture,
    pub close: SafeTexture,
    pub search: SafeTexture,
    pub order: SafeTexture,
    pub filter: SafeTexture,
    pub r#mod: SafeTexture,
    pub star: SafeTexture,

    pub r#abstract: SafeTexture,
}

impl Icons {
    pub async fn new() -> Result<Self> {
        Ok(Self {
            icon: load_texture("icon.png").await?.into(),
            play: load_texture("resume.png").await?.into(),
            medal: load_texture("medal.png").await?.into(),
            respack: load_texture("respack.png").await?.into(),
            msg: load_texture("message.png").await?.into(),
            settings: load_texture("settings.png").await?.into(),
            lang: load_texture("language.png").await?.into(),
            back: TEX_ICON_BACK.with(|it| it.borrow().clone().unwrap()),
            download: load_texture("download.png").await?.into(),
            user: load_texture("user.png").await?.into(),
            info: load_texture("info.png").await?.into(),
            delete: load_texture("delete.png").await?.into(),
            menu: load_texture("menu.png").await?.into(),
            edit: load_texture("edit.png").await?.into(),
            ldb: load_texture("leaderboard.png").await?.into(),
            close: load_texture("close.png").await?.into(),
            search: SafeTexture::from(load_texture("search.png").await?).with_mipmap(),
            order: SafeTexture::from(load_texture("order.png").await?).with_mipmap(),
            filter: SafeTexture::from(load_texture("filter.png").await?).with_mipmap(),
            r#mod: SafeTexture::from(load_texture("mod.png").await?).with_mipmap(),
            star: SafeTexture::from(load_texture("star.png").await?).with_mipmap(),

            r#abstract: load_texture("abstract.jpg").await?.into(),
        })
    }
}
