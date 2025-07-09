prpr::tl_file!("respack");

use super::{Page, SharedState};
use crate::{
    dir, get_data, get_data_mut,
    icons::Icons,
    save_data,
    scene::{confirm_delete, MainScene},
};
use anyhow::Result;
use macroquad::prelude::*;
use prpr::{
    core::{NoteStyle, ParticleEmitter, ResPackInfo, ResourcePack},
    ext::{create_audio_manger, poll_future, semi_black, semi_white, LocalTask, RectExt, SafeTexture, ScaleType},
    scene::{request_file, show_error, show_message},
    ui::{DRectButton, Dialog, Scroll, Ui},
};
use sasa::{AudioManager, PlaySfxParams, Sfx};
use serde_yaml::Error;
use std::{
    borrow::Cow,
    fs::File,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

fn build_emitter(pack: &ResourcePack) -> Result<ParticleEmitter> {
    ParticleEmitter::new(pack, get_data().config.note_scale * 0.6, pack.info.hide_particles)
}

pub struct ResPackItem {
    path: Option<PathBuf>,
    name: String,
    btn: DRectButton,

    loaded: Option<ResourcePack>,
    load_task: LocalTask<Result<ResourcePack>>,
}

impl ResPackItem {
    pub fn new(path: Option<PathBuf>, name: String) -> Self {
        Self {
            path,
            name,
            btn: DRectButton::new(),

            loaded: None,
            load_task: None,
        }
    }

    fn load(&mut self) {
        if self.load_task.is_some() {}
        if let Some(loaded) = self.loaded.take() {
            self.load_task = Some(Box::pin(async move { Ok(loaded) }));
        } else {
            self.load_task = Some(Box::pin(ResourcePack::from_path(self.path.clone())));
        }
    }
}

pub struct ResPackPage {
    audio: AudioManager,
    items: Vec<ResPackItem>,
    import_btn: DRectButton,
    btns_scroll: Scroll,
    index: usize,

    icons: Arc<Icons>,

    info_btn: DRectButton,
    delete_btn: DRectButton,

    should_delete: Arc<AtomicBool>,

    emitter: Option<ParticleEmitter>,
    sfxs: Option<[Sfx; 3]>,
    last_round: u32,
}

impl ResPackPage {
    pub fn new(icons: Arc<Icons>) -> Result<Self> {
        MainScene::take_imported_respack();
        let dir = dir::respacks()?;
        let mut items = vec![ResPackItem::new(None, tl!("default").into_owned())];
        let data = get_data_mut();
        data.respacks = data
            .respacks
            .clone()
            .into_iter()
            .filter(|path| -> bool {
                let p = format!("{dir}/{path}");
                let p = Path::new(&p);
                if !p.is_dir() {
                    return false;
                }
                let cfg = File::open(p.join("info.yml"));
                match cfg {
                    Err(_) => {
                        let _ = std::fs::remove_dir_all(p);
                        false
                    }
                    Ok(cfg) => {
                        let info: Result<ResPackInfo, Error> = serde_yaml::from_reader(cfg);
                        match info {
                            Err(_) => {
                                let _ = std::fs::remove_dir_all(p);
                                false
                            }
                            Ok(info) => {
                                items.push(ResPackItem::new(Some(p.to_owned()), info.name));
                                true
                            }
                        }
                    }
                }
            })
            .collect();
        save_data()?;

        let index = get_data().respack_id;
        items[index].load();
        let delete_btn = DRectButton::new().with_delta(-0.004).with_elevation(0.);
        Ok(Self {
            audio: create_audio_manger(&get_data().config)?,
            items,
            import_btn: DRectButton::new(),
            btns_scroll: Scroll::new(),
            index,

            icons,

            info_btn: delete_btn.clone(),
            delete_btn,

            should_delete: Arc::new(AtomicBool::default()),

            emitter: None,
            sfxs: None,
            last_round: u32::MAX,
        })
    }
}

impl Page for ResPackPage {
    fn label(&self) -> Cow<'static, str> {
        tl!("label")
    }

    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        let t = s.t;
        if self.btns_scroll.touch(touch, t) {
            return Ok(true);
        }
        if self.import_btn.touch(touch, t) {
            request_file("_import_respack");
            return Ok(true);
        }
        if self.items[self.index].load_task.is_none() {
            for (index, item) in self.items.iter_mut().enumerate() {
                if item.btn.touch(touch, t) {
                    self.index = index;
                    get_data_mut().respack_id = index;
                    save_data()?;
                    item.load();
                    return Ok(true);
                }
            }
        }
        if self.info_btn.touch(touch, t) {
            let item = &self.items[self.index];
            let info = &item.loaded.as_ref().unwrap().info;
            Dialog::plain(
                tl!("info"),
                tl!("info-content", "name" => item.name.clone(), "author" => info.author.clone(), "desc" => info.description.clone()),
            )
            .listener(|_dialog, pos| pos == -2)
            .show();
            return Ok(true);
        }
        if self.delete_btn.touch(touch, t) {
            if self.index == 0 {
                show_message(tl!("cant-delete-builtin")).error();
                return Ok(true);
            }
            confirm_delete(self.should_delete.clone());
            return Ok(true);
        }
        Ok(false)
    }

    fn update(&mut self, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        self.btns_scroll.update(t);
        let item = &mut self.items[self.index];
        if let Some(task) = &mut item.load_task {
            if let Some(res) = poll_future(task.as_mut()) {
                match res {
                    Err(err) => {
                        show_error(err.context(tl!("load-failed")));
                    }
                    Ok(val) => {
                        self.emitter = Some(build_emitter(&val)?);
                        self.sfxs = Some([
                            self.audio.create_sfx(val.sfx_click.clone(), None)?,
                            self.audio.create_sfx(val.sfx_drag.clone(), None)?,
                            self.audio.create_sfx(val.sfx_flick.clone(), None)?,
                        ]);
                        item.loaded = Some(val);
                    }
                }
                item.load_task = None;
            }
        }
        if self.should_delete.fetch_and(false, Ordering::Relaxed) {
            std::fs::remove_dir_all(self.items[self.index].path.as_ref().unwrap())?;
            self.items.remove(self.index);
            get_data_mut().respacks.remove(self.index - 1);
            self.index -= 1;
            get_data_mut().respack_id = self.index;
            save_data()?;
            self.items[self.index].load();
            show_message(tl!("deleted")).ok();
        }
        if let Some(item) = MainScene::take_imported_respack() {
            self.items.push(item);
        }
        Ok(())
    }

    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        let t = s.t;

        let mut cr = ui.content_rect();
        let d = 0.29;
        cr.x += d;
        cr.w -= d;
        let r = Rect::new(-0.92, cr.y, 0.47, cr.h);

        s.render_fader(ui, |ui| {
            ui.fill_path(&r.rounded(0.005), semi_black(0.4));
            let pad = 0.02;
            self.btns_scroll.size((r.w, r.h - pad));
            ui.dx(r.x);
            ui.dy(r.y + pad);
            self.btns_scroll.render(ui, |ui| {
                let w = r.w - pad * 2.;
                let mut h = 0.;
                let r = Rect::new(pad, 0., r.w - pad * 2., 0.1);
                for (index, item) in self.items.iter_mut().enumerate() {
                    item.btn.render_text(ui, r, t, &item.name, 0.7, index == self.index);
                    ui.dy(r.h + pad);
                    h += r.h + pad;
                }
                self.import_btn.render_text(ui, r, t, "+", 0.8, false);
                ui.dy(r.h + pad);
                h += r.h + pad;
                (w, h)
            });
        });

        s.render_fader(ui, |ui| {
            ui.fill_path(&cr.rounded(0.005), semi_black(0.4));
            let item = &self.items[self.index];
            if let Some(pack) = &item.loaded {
                let width = 0.16;
                let mut r = Rect::new(cr.x + 0.07, cr.y + 0.1, width, 0.);
                let mut draw = |mut r: Rect, tex: Texture2D, mh: Texture2D| {
                    let y = r.y;
                    r.h = tex.height() / tex.width() * r.w;
                    r.y = y - r.h / 2.;
                    ui.fill_rect(r, (tex, r, ScaleType::Fit));
                    r.x += r.w * 1.8;
                    r.w *= mh.width() / tex.width();
                    r.x -= r.w / 2.;
                    r.h = mh.height() / mh.width() * r.w;
                    r.y = y - r.h / 2.;
                    ui.fill_rect(r, (mh, r, ScaleType::Fit));
                };
                let sp = (cr.h - 0.4) / 2.;
                draw(r, *pack.note_style.click, *pack.note_style_mh.click);
                r.y += sp;
                draw(r, *pack.note_style.drag, *pack.note_style_mh.drag);
                r.y += sp;
                draw(r, *pack.note_style.flick, *pack.note_style_mh.flick);
                r.y += sp;
                let mut r = Rect::new(0.1, cr.y + 0.1, width, cr.h - 0.38);
                let draw = |mut r: Rect, style: &NoteStyle, width: f32| {
                    let conv = |r: Rect, tex: &SafeTexture| Rect::new(r.x * tex.width(), r.y * tex.height(), r.w * tex.width(), r.h * tex.height());
                    let tr = conv(style.hold_tail_rect(), &style.hold);
                    let factor = if pack.info.hold_compact { 0.5 } else { 1. };
                    let h = tr.h / tr.w * width;
                    let r2 = Rect::new(r.x, r.y - h * factor, width, h);
                    let r2 = ui.rect_to_global(r2);
                    draw_texture_ex(
                        *style.hold,
                        r2.x,
                        r2.y,
                        semi_white(ui.alpha),
                        DrawTextureParams {
                            source: Some(tr),
                            dest_size: Some(vec2(r2.w, r2.h)),
                            ..Default::default()
                        },
                    );
                    let tr = conv(style.hold_head_rect(), &style.hold);
                    let h = tr.h / tr.w * width;
                    let r2 = Rect::new(r.x, r.bottom() - h * (1. - factor), width, h);
                    let r2 = ui.rect_to_global(r2);
                    draw_texture_ex(
                        *style.hold,
                        r2.x,
                        r2.y,
                        semi_white(ui.alpha),
                        DrawTextureParams {
                            source: Some(tr),
                            dest_size: Some(vec2(r2.w, r2.h)),
                            ..Default::default()
                        },
                    );
                    r.w = width;
                    let r2 = ui.rect_to_global(r);
                    draw_texture_ex(
                        if pack.info.hold_repeat {
                            **style.hold_body.as_ref().unwrap()
                        } else {
                            *style.hold
                        },
                        r2.x,
                        r2.y,
                        semi_white(ui.alpha),
                        DrawTextureParams {
                            source: Some({
                                if pack.info.hold_repeat {
                                    let hold_body = style.hold_body.as_ref().unwrap();
                                    let w = hold_body.width();
                                    Rect::new(0., 0., w, r2.h / width / 2. * w)
                                } else {
                                    conv(style.hold_body_rect(), &style.hold)
                                }
                            }),
                            dest_size: Some(vec2(r2.w, r2.h)),
                            ..Default::default()
                        },
                    )
                };
                draw(r, &pack.note_style, width);
                r.x += width + 0.04;
                draw(r, &pack.note_style_mh, width * pack.note_style_mh.hold.width() / pack.note_style.hold.width());
                let x = cr.x + 0.05;
                if let Some(emitter) = &mut self.emitter {
                    emitter.draw(get_frame_time());
                };

                let inter = 1.5;
                let rnd = t.div_euclid(inter);
                let irnd = rnd as u32;
                let tex = match irnd % 3 {
                    0 => *pack.note_style.click,
                    1 => *pack.note_style.drag,
                    2 => *pack.note_style.flick,
                    _ => unreachable!(),
                };
                let st = r.y + 0.06;
                let cx = r.x + 0.43;
                let line = 0.12;
                ui.fill_rect(Rect::new(cx - 0.2, line - 0.004, 0.4, 0.008), WHITE);
                let p = (t - inter * rnd) / 0.9;
                if p <= 1. {
                    let y = st + (line - st) * p;
                    let h = tex.height() / tex.width() * width;
                    let r = Rect::new(cx - width / 2., y - h / 2., width, h);
                    ui.fill_rect(r, (tex, r, ScaleType::Fit));
                } else if irnd != self.last_round {
                    if let Some(emitter) = &mut self.emitter {
                        emitter.emit_at(vec2(cx, line), 0., pack.info.fx_perfect());
                    }
                    if let Some(sfxs) = &mut self.sfxs {
                        let _ = sfxs[(irnd % 3) as usize].play(PlaySfxParams::default());
                    }
                    self.last_round = irnd;
                }
                ui.text(&item.name)
                    .pos(x, cr.bottom() - 0.05)
                    .anchor(0., 1.)
                    .max_width(cr.right() - x - 0.05)
                    .size(1.2)
                    .draw();
            } else {
                let ct = cr.center();
                ui.loading(ct.x, ct.y, t, WHITE, ());
            }
            let s = 0.12;
            let mut tr = Rect::new(cr.right() - 0.04 - s, cr.bottom() - 0.04 - s, s, s);
            self.delete_btn.render_shadow(ui, tr, t, |ui, path| {
                ui.fill_path(&path, semi_black(0.2));
                let r = tr.feather(-0.02);
                ui.fill_rect(r, (*self.icons.delete, r, ScaleType::Fit));
            });
            if item.loaded.is_some() {
                tr.x -= tr.w + 0.02;
                self.info_btn.render_shadow(ui, tr, t, |ui, path| {
                    ui.fill_path(&path, semi_black(0.2));
                    let r = tr.feather(-0.02);
                    ui.fill_rect(r, (*self.icons.info, r, ScaleType::Fit));
                });
            }
        });
        Ok(())
    }
}
