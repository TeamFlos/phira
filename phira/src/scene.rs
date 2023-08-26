prpr::tl_file!("import" itl);

mod chart_order;
pub use chart_order::{ChartOrder, ORDERS};

pub(crate) mod event;
pub use event::EventScene;

mod main;
pub use main::{MainScene, BGM_VOLUME_UPDATED, MP_PANEL};

mod song;
pub use song::{Downloading, SongScene, RECORD_ID};

mod profile;
pub use profile::ProfileScene;

use crate::{client::UserManager, data::LocalChart, dir, get_data, page::Fader};
use anyhow::{bail, Context, Result};
use prpr::{
    config::Mods,
    ext::{semi_white, unzip_into, RectExt, SafeTexture},
    fs::{self, FileSystem},
    ui::{Dialog, RectButton, Scroll, Ui},
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
use uuid::Uuid;

thread_local! {
    pub static TEX_BACKGROUND: RefCell<Option<SafeTexture>> = RefCell::new(None);
    pub static TEX_ICON_BACK: RefCell<Option<SafeTexture>> = RefCell::new(None);
}

pub fn fs_from_path(path: &str) -> Result<Box<dyn FileSystem + Send + Sync + 'static>> {
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
    let mut id = Uuid::new_v4();
    while dir.join(&id.to_string()).exists() {
        id = Uuid::new_v4();
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

pub struct LdbDisplayItem<'a> {
    pub player_id: i32,
    pub rank: u32,
    pub score: String,
    pub alt: Option<String>,
    pub btn: &'a mut RectButton,
}

pub fn render_ldb<'a>(
    ui: &mut Ui,
    title: &str,
    w: f32,
    rt: f32,
    scroll: &mut Scroll,
    fader: &mut Fader,
    icon_user: &SafeTexture,
    iter: Option<impl Iterator<Item = LdbDisplayItem<'a>>>,
) {
    use macroquad::prelude::*;

    let pad = 0.03;
    let width = w - pad;
    ui.dy(0.03);
    let r = ui.text(title).size(0.8).draw();
    ui.dy(r.h + 0.03);
    let sh = ui.top * 2. - r.h - 0.08;
    let Some(iter) = iter else {
        ui.loading(width / 2., sh / 2., rt, WHITE, ());
        return;
    };
    scroll.size((width, sh));
    scroll.render(ui, |ui| {
        ui.text(ttl!("release-to-refresh"))
            .pos(width / 2., -0.13)
            .anchor(0.5, 0.)
            .size(0.8)
            .draw();
        let s = 0.14;
        let mut h = 0.;
        ui.dx(0.02);
        fader.reset();
        let me = get_data().me.as_ref().map(|it| it.id);
        fader.for_sub(|f| {
            for item in iter {
                f.render(ui, rt, |ui, c| {
                    if me == Some(item.player_id) {
                        ui.fill_path(&Rect::new(-0.02, 0., width, s).feather(-0.01).rounded(0.02), Color { a: c.a, ..ui.background() });
                    }
                    let r = s / 2. - 0.02;
                    ui.text(format!("#{}", item.rank))
                        .pos((0.18 - r) / 2., s / 2.)
                        .anchor(0.5, 0.5)
                        .no_baseline()
                        .size(0.52)
                        .color(c)
                        .draw();
                    let ct = (0.18, s / 2.);
                    ui.avatar(ct.0, ct.1, r, c, rt, UserManager::opt_avatar(item.player_id, icon_user));
                    item.btn.set(ui, Rect::new(ct.0 - r, ct.1 - r, r * 2., r * 2.));
                    let mut rt = width - 0.04;
                    if let Some(alt) = item.alt {
                        let r = ui
                            .text(alt)
                            .pos(rt, s / 2.)
                            .anchor(1., 0.5)
                            .no_baseline()
                            .size(0.4)
                            .color(semi_white(c.a * 0.6))
                            .draw();
                        rt -= r.w + 0.01;
                    } else {
                        rt -= 0.01;
                    }
                    let r = ui
                        .text(item.score)
                        .pos(rt, s / 2.)
                        .anchor(1., 0.5)
                        .no_baseline()
                        .size(0.6)
                        .color(c)
                        .draw();
                    rt -= r.w + 0.03;
                    let lt = 0.24;
                    if let Some((name, color)) = UserManager::name_and_color(item.player_id) {
                        ui.text(name)
                            .pos(lt, s / 2.)
                            .anchor(0., 0.5)
                            .no_baseline()
                            .max_width(rt - lt - 0.01)
                            .size(0.5)
                            .color(Color { a: c.a, ..color })
                            .draw();
                    }
                });
                ui.dy(s);
                h += s;
            }
        });
        (width, h)
    });
}
