prpr::tl_file!("collection");

use super::{Illustration, NextPage, Page, SharedState};
use crate::{icons::Icons, load_res_tex, resource::rtl, scene::ChapterScene};
use anyhow::Result;
use macroquad::prelude::*;
use prpr::{
    core::Tweenable,
    ext::{poll_future, semi_black, semi_white, LocalTask, RectExt, SafeTexture},
    scene::NextScene,
    ui::{RectButton, Scroll, Ui},
};
use std::{borrow::Cow, sync::Arc};
use tap::Tap;

struct CollectionItem {
    id: String,
    illu: Illustration,
    title: String,
    btn: RectButton,
}

struct Transit {
    r: Rect,
    ir: Rect,
    t: f32,
    illu: SafeTexture,
}

pub struct CollectionPage {
    icons: Arc<Icons>,

    colls: Vec<CollectionItem>,
    scroll: Scroll,

    next_page: Option<NextPage>,
    next_scene: Option<NextScene>,

    transit_id: Option<String>,
    transit: Option<Transit>,
    scene_task: LocalTask<Result<NextScene>>,
    ok_to_transit: bool,

    first_in: bool,
}

impl CollectionPage {
    const WIDTH: f32 = 0.5;
    const HEIGHT: f32 = 0.63;
    const PAD: f32 = 0.06;

    pub async fn new(icons: Arc<Icons>) -> Result<Self> {
        Ok(Self {
            icons,

            colls: {
                let mut res = {
                    use crate::resource::L10N_LOCAL;
                    vec![CollectionItem {
                        id: "c1".to_owned(),
                        illu: Illustration::from_done(load_res_tex("res/chap/c1/cover").await),
                        title: rtl!("chap-c1").into_owned(),
                        btn: RectButton::new(),
                    }]
                };
                res.push(CollectionItem {
                    id: "@".to_owned(),
                    illu: Illustration::from_done(Texture2D::from_rgba8(1, 1, &[211, 211, 211, 255]).into()),
                    title: tl!("wait-for-more").into_owned(),
                    btn: RectButton::new(),
                });
                res
            },
            scroll: Scroll::new().horizontal().tap_mut(|it| it.x_scroller.step = Self::WIDTH + Self::PAD),

            next_page: None,
            next_scene: None,

            transit_id: None,
            transit: None,
            scene_task: None,
            ok_to_transit: false,

            first_in: true,
        })
    }
}

impl Page for CollectionPage {
    fn enter(&mut self, s: &mut SharedState) -> Result<()> {
        if self.first_in {
            self.first_in = false;
        } else {
            self.transit.as_mut().unwrap().t = -s.rt;
        }

        Ok(())
    }

    fn label(&self) -> Cow<'static, str> {
        tl!("label")
    }

    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        let rt = s.rt;

        if self.scroll.touch(touch, rt) {
            return Ok(true);
        }
        if self.transit.is_none() {
            for coll in &mut self.colls {
                if coll.btn.touch(touch) {
                    if coll.id != "@" {
                        self.transit_id = Some(coll.id.clone());
                        let id = coll.id.clone();
                        let icons = Arc::clone(&self.icons);
                        let rank_icons = s.icons.clone();
                        let illu = coll.illu.texture.1.clone();
                        self.scene_task = Some(Box::pin(async move {
                            let scene = ChapterScene::new(id, icons, rank_icons, illu).await?;
                            Ok(NextScene::Overlay(Box::new(scene)))
                        }));
                    }
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn update(&mut self, s: &mut SharedState) -> Result<()> {
        let t = s.t;

        self.scroll.update(s.rt);
        for coll in &mut self.colls {
            coll.illu.settle(t);
        }
        if let Some(task) = &mut self.scene_task {
            if let Some(res) = poll_future(task.as_mut()) {
                self.next_scene = Some(res?);
                self.scene_task = None;
            }
        }
        Ok(())
    }

    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        let rt = s.rt;

        s.render_fader(ui, |ui| {
            self.scroll.size((2., ui.top * 2.));
            ui.scope(|ui| {
                ui.dx(-1.);
                ui.dy(-ui.top);
                let cur = self.scroll.x_scroller.offset;
                self.scroll.render(ui, |ui| {
                    ui.dx(1.);
                    ui.dy(ui.top);
                    let mut x = 0.;
                    let step = Self::WIDTH + Self::PAD;
                    for coll in &mut self.colls {
                        let off = (x - cur) / step;
                        let r = Rect::new(x - Self::WIDTH / 2., -Self::HEIGHT / 2., Self::WIDTH, Self::HEIGHT);
                        coll.btn.set(ui, r);
                        ui.fill_rect(r, BLACK);
                        let mut ir = r.nonuniform_feather(0.4, 0.2);
                        ir.x += off / 4.;
                        ui.fill_rect(r, coll.illu.shading(ir, t));
                        ui.fill_rect(r, semi_black(0.2));
                        if self.transit_id.as_ref() == Some(&coll.id) {
                            self.transit = Some(Transit {
                                r,
                                ir,
                                t: rt,
                                illu: coll.illu.texture.1.clone(),
                            });
                            self.transit_id = None;
                        }
                        let p = 1. - off.abs() * step;
                        ui.text(&coll.title)
                            .pos(r.x + 0.02, r.y + 0.02)
                            .max_width(r.w - 0.04)
                            .size(0.5 + p * 0.34)
                            .color(semi_white(1. - (1. - p) * 0.8))
                            .draw();
                        x += step;
                    }

                    (step * (self.colls.len() - 1) as f32 + 2., Self::HEIGHT)
                });
            })
        });
        Ok(())
    }

    fn render_top(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        if let Some(tr) = &self.transit {
            let p = ((s.rt - tr.t.abs()) / (if tr.t < 0. { 0.3 } else { 0.5 })).min(1.);
            let p = if tr.t < 0. { (1. - p).powi(3) } else { 1. - (1. - p).powi(3) };

            let xp = p;
            let yp = (p / 0.45).min(1.);
            let sr = ui.screen_rect();
            let r = tr.r;
            let r = Rect::new(f32::tween(&r.x, &sr.x, xp), f32::tween(&r.y, &sr.y, yp), f32::tween(&r.w, &sr.w, xp), f32::tween(&r.h, &sr.h, yp));
            let ir = Rect::tween(&tr.ir, &sr, xp);
            ui.fill_rect(r, (*tr.illu, ir));
            ui.fill_rect(r, semi_black(0.2 + 0.1 * p));
            if p >= 1. && tr.t > 0. {
                self.ok_to_transit = true;
            }
            if p <= 0. && tr.t < 0. {
                self.transit = None;
            }
        }
        Ok(())
    }

    fn next_page(&mut self) -> NextPage {
        self.next_page.take().unwrap_or_default()
    }

    fn next_scene(&mut self, _s: &mut SharedState) -> NextScene {
        if self.ok_to_transit {
            self.ok_to_transit = false;
            return self.next_scene.take().unwrap_or_default();
        }
        NextScene::None
    }
}
