prpr::tl_file!("event");

use super::{render_ldb, LdbDisplayItem, ProfileScene};
use crate::{
    client::{recv_raw, Client, Event, UserManager},
    icons::Icons,
    page::{EventPage, Fader, Illustration, SFader},
    uml::{parse_uml, Uml},
};
use anyhow::Result;
use chrono::Utc;
use macroquad::prelude::*;
use prpr::{
    core::Tweenable,
    ext::{open_url, semi_black, semi_white, RectExt, SafeTexture, ScaleType},
    scene::{show_error, NextScene, Scene},
    task::Task,
    time::TimeManager,
    ui::{button_hit, DRectButton, LoadingParams, RectButton, Scroll, Ui},
};
use serde::Deserialize;
use std::{any::Any, sync::Arc, time::SystemTime};

const DEBUG_MODE: bool = cfg!(feature = "event_debug");
const LDB_WIDTH: f32 = 0.94;
const TRANSIT_TIME: f32 = 0.4;

#[derive(Deserialize)]
struct LdbItem {
    player: i32,
    rank: i32,
    score: i32,
    #[serde(skip, default)]
    btn: RectButton,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct Status {
    joined: bool,
    rank: Option<i32>,
    score: Option<i32>,
}

pub struct EventScene {
    event: Event,
    illu: Illustration,

    scroll: Scroll,

    btn_back: RectButton,

    status_task: Option<Task<Result<Status>>>,
    status: Option<Status>,

    uml_task: Option<Task<Result<String>>>,
    uml: Uml,
    last_modified: SystemTime,

    next_scene: Option<NextScene>,

    btn_join: DRectButton,
    join_task: Option<Task<Result<()>>>,

    scrolled: bool,
    start_time: f32,

    side_enter_time: f32,

    ldb_scroll: Scroll,
    ldb_fader: Fader,
    ldb_task: Option<Task<Result<Vec<LdbItem>>>>,
    ldb: Option<Vec<LdbItem>>,

    icons: Arc<Icons>,
    rank_icons: [SafeTexture; 8],

    last_scroll_handled_touch: bool,

    sf: SFader,
}

impl EventScene {
    pub fn new(event: Event, illu: Illustration, icons: Arc<Icons>, rank_icons: [SafeTexture; 8]) -> Self {
        let id = event.id;
        Self {
            event,
            illu,

            scroll: Scroll::new(),

            btn_back: RectButton::new(),

            status_task: None,
            status: None,

            uml_task: if DEBUG_MODE {
                None
            } else {
                Some(Task::new(async move {
                    Ok(recv_raw(Client::get(format!("/event/{id}/uml")).query(&[("version", env!("CARGO_PKG_VERSION"))]))
                        .await?
                        .text()
                        .await?)
                }))
            },
            uml: Uml::default(),
            last_modified: SystemTime::now(),

            next_scene: None,

            btn_join: DRectButton::new(),
            join_task: None,

            scrolled: false,
            start_time: 0.,

            side_enter_time: f32::NAN,

            ldb_scroll: Scroll::new(),
            ldb_fader: Fader::new(),
            ldb_task: None,
            ldb: None,

            icons,
            rank_icons,

            last_scroll_handled_touch: false,

            sf: SFader::new(),
        }
    }

    fn load_status(&mut self) {
        self.status = None;
        let id = self.event.id;
        self.status_task = Some(Task::new(async move { Ok(recv_raw(Client::get(format!("/event/{id}/status"))).await?.json().await?) }));
    }

    fn load_ldb(&mut self) {
        let id = self.event.id;
        self.ldb = None;
        self.ldb_task = Some(Task::new(async move { Ok(recv_raw(Client::get(format!("/event/{id}/list15"))).await?.json().await?) }));
    }

    fn loading(&self) -> bool {
        self.join_task.is_some()
    }

    fn join_or(&mut self, rt: f32) {
        if let Some(status) = &self.status {
            if status.joined {
                if (self.event.time_start..self.event.time_end).contains(&Utc::now()) {
                    if self.ldb_task.is_none() && self.ldb.is_none() {
                        self.load_ldb();
                    }
                    self.side_enter_time = rt;
                }
            } else {
                let id = self.event.id;
                self.join_task = Some(Task::new(async move {
                    recv_raw(Client::post(format!("/event/{id}/join"), &())).await?;
                    Ok(())
                }));
            }
        }
    }
}

impl Scene for EventScene {
    fn on_result(&mut self, tm: &mut TimeManager, res: Box<dyn Any>) -> Result<()> {
        let _res = match res.downcast::<bool>() {
            Err(res) => res,
            Ok(delete) => {
                self.uml.on_result(tm.now() as _, *delete);
                return Ok(());
            }
        };
        Ok(())
    }

    fn enter(&mut self, tm: &mut TimeManager, _target: Option<RenderTarget>) -> Result<()> {
        self.start_time = tm.now() as _;
        self.load_status();
        Ok(())
    }

    fn touch(&mut self, tm: &mut TimeManager, touch: &Touch) -> Result<bool> {
        let t = tm.now() as f32;
        let rt = tm.real_time() as f32;

        if self.loading() {
            return Ok(true);
        }

        if !self.side_enter_time.is_nan() {
            if self.side_enter_time > 0.
                && tm.real_time() as f32 > self.side_enter_time + TRANSIT_TIME
                && touch.position.x < 1. - LDB_WIDTH
                && touch.phase == TouchPhase::Started
            {
                self.side_enter_time = -rt;
                return Ok(true);
            }
            if self.ldb_scroll.touch(touch, t) {
                return Ok(true);
            }
            return Ok(false);
        }

        if self.scroll.y_scroller.offset < 0.3 {
            if self.btn_back.touch(touch) {
                button_hit();
                self.next_scene = Some(NextScene::Pop);
                return Ok(true);
            }
            if self.btn_join.touch(touch, t) {
                self.join_or(rt);
                return Ok(true);
            }
        }

        if self.last_scroll_handled_touch && self.scroll.touch(touch, t) {
            self.scrolled = true;
            return Ok(true);
        } else {
            self.last_scroll_handled_touch = false;
        }

        let mut action = None;
        if self.uml.touch(touch, t, rt, &mut action)? {
            if let Some(action) = action {
                match action.as_str() {
                    "exit" => {
                        self.next_scene = Some(NextScene::Pop);
                    }
                    "join" => {
                        self.join_or(rt);
                    }
                    x => {
                        if let Some(url) = x.strip_prefix("open:") {
                            open_url(url)?;
                        }
                    }
                }
            }
            return Ok(true);
        }

        if !self.last_scroll_handled_touch && self.scroll.touch(touch, t) {
            self.scrolled = true;
            self.last_scroll_handled_touch = true;
            return Ok(true);
        }

        if let Some(ldb) = &mut self.ldb {
            for item in ldb {
                if item.btn.touch(touch) {
                    button_hit();
                    self.sf
                        .goto(t, ProfileScene::new(item.player, self.icons.user.clone(), self.rank_icons.clone()));
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }

    fn update(&mut self, tm: &mut TimeManager) -> Result<()> {
        let t = tm.now() as f32;

        self.scroll.update(t);

        if self.ldb_scroll.y_scroller.pulled {
            self.load_ldb();
        }
        self.ldb_scroll.update(t);

        if DEBUG_MODE {
            let path = std::path::Path::new("test.uml");
            if let Ok(meta) = path.metadata() {
                let new_modified = meta.modified()?;
                if new_modified != self.last_modified {
                    self.last_modified = new_modified;
                    self.uml = parse_uml(&std::fs::read_to_string(path)?, &self.icons, &self.rank_icons).unwrap_or_else(|e| {
                        eprintln!("{e:?}");
                        Uml::default()
                    });
                }
            }
        } else if let Some(task) = &mut self.uml_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        show_error(err.context(tl!("load-failed")));
                    }
                    Ok(res) => {
                        self.uml = parse_uml(&res, &self.icons, &self.rank_icons).map_err(anyhow::Error::msg)?;
                    }
                }
                self.uml_task = None;
            }
        }

        if let Some(task) = &mut self.status_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        show_error(err.context(tl!("load-status-failed")));
                    }
                    Ok(val) => {
                        self.status = Some(val);
                    }
                }
                self.status_task = None;
            }
        }

        if let Some(task) = &mut self.join_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        show_error(err.context(tl!("join-failed")));
                    }
                    Ok(_) => {
                        self.load_status();
                    }
                }
                self.join_task = None;
            }
        }

        if let Some(task) = &mut self.ldb_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        show_error(err.context(tl!("load-ldb-failed")));
                    }
                    Ok(ldb) => {
                        for item in ldb.iter() {
                            UserManager::request(item.player);
                        }
                        self.ldb = Some(ldb);
                    }
                }
                self.ldb_task = None;
            }
        }

        Ok(())
    }

    fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        set_camera(&ui.camera());
        let t = tm.now() as f32;
        let rt = tm.real_time() as f32;

        let r = ui.screen_rect();
        ui.fill_rect(r, self.illu.shading(r, t));
        ui.fill_rect(r, semi_black(0.4));

        let p = 1. - (self.scroll.y_scroller.offset / 0.4).clamp(0., 1.);

        let r = ui.back_rect();
        ui.fill_rect(r, (*self.icons.back, r, ScaleType::Fit, semi_white(p)));
        self.btn_back.set(ui, r);

        ui.fill_rect(ui.screen_rect(), semi_black((self.scroll.y_scroller.offset / 0.3).min(1.) * 0.7));
        ui.scope(|ui| {
            ui.dx(-1.);
            ui.dy(-ui.top);
            let o = self.scroll.y_scroller.offset;
            self.scroll.size((2., ui.top * 2.));
            self.scroll.render(ui, |ui| {
                let top = ui.top;
                ui.text(&self.event.name)
                    .pos(EventPage::LB_PAD, top * 2. - EventPage::LB_PAD)
                    .anchor(0., 1.)
                    .size(1.5)
                    .draw();
                ui.dy(ui.top * 2.);
                if self.uml_task.is_some() {
                    let pad = 0.06;
                    ui.loading(1., pad + 0.05, t, WHITE, ());
                    (2., ui.top * 2. + (pad + 0.05) * 2.)
                } else {
                    let h = match self.uml.render(
                        ui,
                        t,
                        rt,
                        &[
                            ("t", t),
                            ("o", o),
                            ("top", ui.top),
                            ("joined", self.status.as_ref().map_or(-1., |it| it.joined as u32 as f32)),
                        ],
                    ) {
                        Ok((_, h)) => h,
                        Err(e) => {
                            eprintln!("{e:?}");
                            0.
                        }
                    };
                    (2., ui.top * 2. + h + 0.02)
                }
            });
        });

        let elapsed = t - self.start_time;
        if !self.scrolled && elapsed > 2. {
            let top = ui.top;
            ui.text(tl!("scroll-down-for-more"))
                .pos(0., top - 0.03)
                .anchor(0.5, 1.)
                .size(0.4)
                .color(semi_white((((elapsed - 2.) * 1.5 - std::f32::consts::FRAC_PI_2).sin() + 1.) / 2.))
                .draw();
        }

        ui.alpha(p * (elapsed / 0.3).min(1.), |ui| {
            let r = Rect::new(1. - 0.24, ui.top - 0.12, 0., 0.).nonuniform_feather(0.19, 0.07);
            let ct = r.center();
            if let Some(status) = &self.status {
                let bc = ui.background();
                let mut draw = |text, bc| {
                    let oh = r.h;
                    self.btn_join.render_shadow(ui, r, t, |ui, path| {
                        ui.fill_path(&path, Color { a: p, ..bc });
                        ui.text(text)
                            .pos(ct.x, ct.y)
                            .anchor(0.5, 0.5)
                            .no_baseline()
                            .size(0.8 * (1. - (1. - r.h / oh).powf(1.3)))
                            .max_width(r.w)
                            .draw();
                    });
                };
                if status.joined {
                    if Utc::now() > self.event.time_end {
                        draw(tl!("btn-ended"), semi_black(0.4));
                    } else if Utc::now() < self.event.time_start {
                        draw(tl!("btn-not-started"), Color::from_hex(0xffe3f2fd));
                    } else {
                        self.btn_join
                            .render_shadow(ui, r, t, |ui, path| ui.fill_path(&path, Color::from_hex(0xfff57c00)));
                        let mut text = ui.text(format!("#{}", status.rank.unwrap())).anchor(0., 0.5).no_baseline().size(0.7);
                        let w = text.measure().w;
                        let mut ir = Rect::new(ct.x, ct.y, 0., 0.).feather(r.h / 2. - 0.02);
                        let w = w + 0.01 + ir.w;
                        ir.x += (ir.w - w) / 2.;
                        text.pos(ir.right() + 0.01, ct.y).draw();
                        ui.fill_rect(ir, (*self.icons.ldb, ir, ScaleType::Fit));
                    }
                } else {
                    draw(tl!("btn-join"), bc);
                }
            } else {
                self.btn_join.render_shadow(ui, r, t, |ui, path| {
                    ui.fill_path(&path, semi_black(0.4));
                });
                ui.loading(
                    ct.x,
                    ct.y,
                    t,
                    WHITE,
                    LoadingParams {
                        radius: 0.03,
                        width: 0.008,
                        ..Default::default()
                    },
                );
            }
        });

        if !self.side_enter_time.is_nan() {
            let p = ((rt - self.side_enter_time.abs()) / TRANSIT_TIME).min(1.);
            let p = 1. - (1. - p).powi(3);
            let p = if self.side_enter_time < 0. {
                if p >= 1. {
                    self.side_enter_time = f32::NAN;
                }
                1. - p
            } else {
                p
            };
            ui.fill_rect(ui.screen_rect(), semi_black(p * 0.6));
            let w = LDB_WIDTH;
            let lf = f32::tween(&1.04, &(1. - w), p);
            ui.scope(|ui| {
                ui.dx(lf);
                ui.dy(-ui.top);
                let r = Rect::new(-0.2, 0., 0.2 + w, ui.top * 2.);
                ui.fill_rect(r, (Color::default(), (r.x, r.y), Color::new(0., 0., 0., p * 0.7), (r.right(), r.y)));
                render_ldb(
                    ui,
                    &tl!("ldb"),
                    LDB_WIDTH,
                    rt,
                    &mut self.ldb_scroll,
                    &mut self.ldb_fader,
                    &self.icons.user,
                    self.ldb.as_mut().map(|it| {
                        it.iter_mut().map(|it| LdbDisplayItem {
                            player_id: it.player,
                            rank: it.rank as _,
                            score: it.score.to_string(),
                            alt: None,
                            btn: &mut it.btn,
                        })
                    }),
                );
            });
        }

        self.uml.render_top(ui, t, rt)?;

        self.sf.render(ui, t);

        if self.loading() {
            ui.full_loading_simple(t);
        }

        Ok(())
    }

    fn next_scene(&mut self, tm: &mut TimeManager) -> NextScene {
        if let Some(scene) = self.sf.next_scene(tm.now() as _) {
            return scene;
        }
        self.next_scene.take().or_else(|| self.uml.next_scene()).unwrap_or_default()
    }
}
