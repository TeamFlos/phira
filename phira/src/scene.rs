prpr::tl_file!("import" itl);

mod chart_order;
pub use chart_order::{ChartOrder, ORDERS};

mod main;
pub use main::{MainScene, BGM_VOLUME_UPDATED};

mod song;
pub use song::SongScene;

mod profile;
pub use profile::ProfileScene;

use crate::{data::LocalChart, dir};
use anyhow::{bail, Context, Result};
use prpr::{
    config::Mods,
    ext::{unzip_into, SafeTexture},
    fs::{self, FileSystem},
    ui::Dialog,
};
use std::{
    cell::RefCell,
    fs::File,
    io::{BufReader, Write},
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use uuid7::{uuid7, Uuid};

thread_local! {
    pub static TEX_BACKGROUND: RefCell<Option<SafeTexture>> = RefCell::new(None);
    pub static TEX_ICON_BACK: RefCell<Option<SafeTexture>> = RefCell::new(None);
}

pub fn fs_from_path(path: &str) -> Result<Box<dyn FileSystem>> {
    if let Some(name) = path.strip_prefix(':') {
        fs::fs_from_assets(format!("charts/{name}/"))
    } else {
        fs::fs_from_file(Path::new(&format!("{}/{path}", dir::charts()?)))
    }
}

pub fn confirm_dialog(title: impl Into<String>, content: impl Into<String>, res: Arc<AtomicBool>) {
    Dialog::plain(title.into(), content.into())
        .buttons(vec![ttl!("cancel").into_owned(), ttl!("confirm").into_owned()])
        .listener(move |id| {
            if id == 1 {
                res.store(true, Ordering::SeqCst);
            }
        })
        .show();
}

#[inline]
pub fn confirm_delete(res: Arc<AtomicBool>) {
    confirm_dialog(ttl!("del-confirm"), ttl!("del-confirm-content"), res)
}

pub async fn import_chart(path: String) -> Result<LocalChart> {
    async fn inner(dir: &Path, id: Uuid, path: String) -> Result<LocalChart> {
        let path = Path::new(&path);
        if !path.exists() || !path.is_file() {
            bail!("not a file");
        }
        let dir = prpr::dir::Dir::new(dir)?;
        unzip_into(BufReader::new(File::open(path)?), &dir, true)?;
        let local_path = format!("custom/{id}");
        let mut fs = fs_from_path(&local_path)?;
        let mut info = fs::load_info(fs.as_mut()).await.with_context(|| itl!("info-fail"))?;
        fs::fix_info(fs.as_mut(), &mut info).await.with_context(|| itl!("invalid-chart"))?;
        dir.create("info.yml")?.write_all(serde_yaml::to_string(&info)?.as_bytes())?;
        Ok(LocalChart {
            info: info.into(),
            local_path,
            record: None,
            mods: Mods::default(),
        })
    }
    let dir = dir::custom_charts()?;
    let dir = Path::new(&dir);
    let mut id = uuid7();
    while dir.join(&id.to_string()).exists() {
        id = uuid7();
    }
    let dir = dir.join(id.to_string());
    std::fs::create_dir(&dir)?;
    match inner(&dir, id, path).await {
        Err(err) => {
            std::fs::remove_dir_all(dir)?;
            Err(err)
        }
        Ok(val) => Ok(val),
    }
}
