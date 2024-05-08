prpr::tl_file!("profile");

use super::{confirm_delete, TEX_BACKGROUND, TEX_ICON_BACK};
use crate::{
    anti_addiction_action,
    client::{recv_raw, Client, Record, User, UserManager},
    get_data, get_data_mut,
    page::{Fader, Illustration, SFader},
    save_data, sync_data,
};
use anyhow::Result;
use chrono::Local;
use macroquad::prelude::*;
use prpr::{
    ext::{open_url, semi_black, semi_white, RectExt, SafeTexture, ScaleType, BLACK_TEXTURE},
    judge::icon_index,
    scene::{request_file, return_file, show_error, show_message, take_file, NextScene, Scene},
    task::Task,
    time::TimeManager,
    ui::{button_hit, rounded_rect_shadow, DRectButton, RectButton, Scroll, ShadowConfig, Ui},
};
use serde_json::json;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::Notify;

struct RecordItem {
    record: Record,
    name: Task<Result<String>>,
    btn: DRectButton,
    illu: Illustration,
}

pub struct ProfileScene {
    id: i32,
    user: Option<Arc<User>>,
    user_badges: Vec<String>,

    pf_scroll: Scroll,

    background: SafeTexture,

    icon_back: SafeTexture,
    icon_user: SafeTexture,

    btn_back: RectButton,
    btn_open_web: DRectButton,
    btn_logout: DRectButton,
    btn_delete: DRectButton,

    load_task: Option<Task<Result<Arc<User>>>>,

    avatar_btn: RectButton,
    avatar_task: Option<Task<Result<()>>>,

    should_delete: Arc<AtomicBool>,
    delete_task: Option<Task<Result<()>>>,

    scroll: Scroll,
    record_task: Option<Task<Result<Vec<RecordItem>>>>,
    record_items: Option<Vec<RecordItem>>,

    sf: SFader,
    fader: Fader,

    rank_icons: [SafeTexture; 8],
}

impl ProfileScene {
    pub fn new(id: i32, icon_user: SafeTexture, rank_icons: [SafeTexture; 8]) -> Self {
        let _ = UserManager::clear_cache(id);
        UserManager::request(id);
        let load_task = Some(Task::new(Client::load(id)));
        Self {
            id,
            user: None,
            user_badges: Vec::new(),

            pf_scroll: Scroll::new(),

            background: TEX_BACKGROUND.with(|it| it.borrow().clone().unwrap()),

            icon_back: TEX_ICON_BACK.with(|it| it.borrow().clone().unwrap()),
            icon_user,

            btn_back: RectButton::new(),
            btn_open_web: DRectButton::new(),
            btn_logout: DRectButton::new(),
            btn_delete: DRectButton::new(),

            load_task,

            avatar_btn: RectButton::new(),
            avatar_task: None,

            should_delete: Arc::default(),
            delete_task: None,

            scroll: Scroll::new(),
            record_task: Some(Task::new(async move {
                let records: Vec<Record> = recv_raw(Client::get(format!("/record?player={id}"))).await?.json().await?;
                Ok(records
                    .into_iter()
                    .map(|it| {
                        let illu = {
                            let chart = it.chart.clone();
                            let notify = Arc::new(Notify::new());
                            Illustration {
                                texture: (BLACK_TEXTURE.clone(), BLACK_TEXTURE.clone()),
                                notify: Arc::clone(&notify),
                                task: Some(Task::new({
                                    async move {
                                        notify.notified().await;
                                        let illu = &chart.fetch().await?.illustration;
                                        Ok((illu.load_thumbnail().await?, None))
                                    }
                                })),
                                loaded: Arc::default(),
                                load_time: f32::NAN,
                            }
                        };
                        let chart = it.chart.clone();
                        RecordItem {
                            record: it,
                            name: Task::new(async move { Ok(chart.fetch().await?.name.clone()) }),
                            btn: DRectButton::new(),
                            illu,
                        }
                    })
                    .collect())
            })),
            record_items: None,

            sf: SFader::new(),
            fader: Fader::new().with_distance(0.12),

            rank_icons,
        }
    }
}

impl Scene for ProfileScene {
    fn enter(&mut self, tm: &mut TimeManager, _target: Option<RenderTarget>) -> Result<()> {
        self.sf.enter(tm.now() as _);
        Ok(())
    }

    fn update(&mut self, tm: &mut TimeManager) -> Result<()> {
        let t = tm.now() as f32;

        self.pf_scroll.update(t);
        self.scroll.update(t);

        if let Some(task) = &mut self.load_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => show_error(err.context(tl!("load-user-failed"))),
                    Ok(res) => {
                        self.user_badges.clear();
                        for badge in &res.badges {
                            match badge.as_str() {
                                "admin" => self.user_badges.push(tl!("badge-admin").into_owned()),
                                "sponsor" => self.user_badges.push(tl!("badge-sponsor").into_owned()),
                                _ => {}
                            }
                        }
                        self.user = Some(res);
                    }
                }
                self.load_task = None;
            }
        }
        if let Some((id, file)) = take_file() {
            if id == "avatar" {
                self.avatar_task = Some(Task::new(async move {
                    let id = Client::upload_file("avatar", std::fs::read(file)?).await?;
                    recv_raw(Client::post("/edit/avatar", &json!({ "file": id }))).await?;
                    Ok(())
                }));
            } else {
                return_file(id, file);
            }
        }
        if let Some(task) = &mut self.avatar_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        show_error(err.context(tl!("edit-avatar-failed")));
                    }
                    Ok(_) => {
                        show_message(tl!("edit-avatar-success")).ok();
                        let id = get_data().me.as_ref().unwrap().id;
                        Client::clear_cache::<User>(id)?;
                        UserManager::clear_cache(id)?;
                        UserManager::request(id);
                    }
                }
                self.avatar_task = None;
            }
        }

        if let Some(task) = &mut self.delete_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        show_error(err.context(tl!("delete-failed")));
                    }
                    Ok(_) => {
                        show_message(tl!("delete-req-sent")).ok();
                    }
                }
                self.delete_task = None;
            }
        }

        if let Some(task) = &mut self.record_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => show_error(err.context(tl!("load-record-failed"))),
                    Ok(val) => {
                        self.record_items = Some(val);
                        self.fader.sub(t);
                    }
                }
                self.record_task = None;
            }
        }

        if self.should_delete.fetch_and(false, Ordering::Relaxed) {
            self.delete_task = Some(Task::new(async move {
                Client::post("/delete-account", &()).send().await?.error_for_status()?;
                Ok(())
            }));
        }

        if let Some(items) = &mut self.record_items {
            for item in items {
                item.illu.settle(t);
            }
        }

        Ok(())
    }

    fn touch(&mut self, tm: &mut TimeManager, touch: &Touch) -> Result<bool> {
        if self.sf.transiting() {
            return Ok(true);
        }
        if self.avatar_task.is_some() {
            return Ok(true);
        }
        let t = tm.now() as f32;
        if self.pf_scroll.touch(touch, t) {
            return Ok(true);
        }
        if self.btn_back.touch(touch) {
            button_hit();
            self.sf.next(t, NextScene::Pop);
            return Ok(true);
        }
        if self.btn_open_web.touch(touch, t) {
            open_url(&format!("https://phira.moe/user/{}", self.id))?;
            return Ok(true);
        }
        if self.btn_logout.touch(touch, t) {
            anti_addiction_action("exit", None);
            get_data_mut().me = None;
            get_data_mut().tokens = None;
            let _ = save_data();
            sync_data();
            show_message(tl!("logged-out")).ok();
            self.sf.next(t, NextScene::Pop);
            return Ok(true);
        }
        if self.btn_delete.touch(touch, t) {
            confirm_delete(Arc::clone(&self.should_delete));
            return Ok(true);
        }
        if get_data().me.as_ref().map_or(false, |it| it.id == self.id) && self.avatar_btn.touch(touch) {
            request_file("avatar");
            return Ok(true);
        }

        if self.scroll.touch(touch, t) {
            return Ok(true);
        }
        if let Some(items) = &mut self.record_items {
            for item in items {
                if item.btn.touch(touch, t) {
                    self.scroll.y_scroller.halt();

                    return Ok(true);
                }
            }
        }

        Ok(false)
    }

    fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        set_camera(&ui.camera());
        let t = tm.now() as f32;

        let r = ui.screen_rect();
        ui.fill_rect(r, (*self.background, r));
        let r = ui.back_rect();
        ui.fill_rect(r, (*self.icon_back, r));
        self.btn_back.set(ui, r);

        let r = Rect::new(-0.85, -ui.top + 0.1, 0.6, 2.);
        let radius = 0.02;
        rounded_rect_shadow(
            ui,
            r,
            &ShadowConfig {
                radius,
                elevation: 0.01,
                ..Default::default()
            },
        );
        ui.fill_path(&r.rounded(radius), ui.background());

        if let Some(user) = &self.user {
            ui.scope(|ui| {
                ui.dx(r.x);
                ui.dy(r.y);
                self.pf_scroll.size((r.w, ui.top - r.y));
                self.pf_scroll.render(ui, |ui| {
                    ui.dx(-r.x);
                    ui.dy(-r.y);
                    let ow = r.w;
                    let oy = r.y;
                    let pad = 0.02;
                    let mw = r.w - pad * 2.;
                    let cx = r.center().x;
                    let radius = 0.12;
                    let r = ui.avatar(cx, r.y + radius + 0.05, radius, t, UserManager::opt_avatar(self.id, &self.icon_user));
                    self.avatar_btn.set(ui, r);
                    let r = ui
                        .text(&user.name)
                        .size(0.74)
                        .pos(cx, r.bottom() + 0.03)
                        .anchor(0.5, 0.)
                        .max_width(mw)
                        .color(user.name_color())
                        .draw();
                    let r = ui
                        .text(format!("RKS {:.2}", user.rks))
                        .size(0.5)
                        .pos(cx, r.bottom() + 0.01)
                        .anchor(0.5, 0.)
                        .draw();
                    let mut r = ui
                        .text(user.bio.as_deref().unwrap_or(""))
                        .pos(cx, r.bottom() + 0.01)
                        .anchor(0.5, 0.)
                        .multiline()
                        .max_width(mw)
                        .size(0.4)
                        .draw();
                    if !self.user_badges.is_empty() {
                        r = ui
                            .text(self.user_badges.join(" "))
                            .pos(cx, r.bottom() + 0.01)
                            .anchor(0.5, 0.)
                            .size(0.5)
                            .draw();
                    }
                    let r = ui
                        .text(tl!("last-login", "time" => user.last_login.with_timezone(&Local).format("%Y-%m-%d %H:%M").to_string()))
                        .pos(cx, r.bottom() + 0.01)
                        .anchor(0.5, 0.)
                        .size(0.4)
                        .color(semi_white(0.6))
                        .draw();
                    let hw = 0.2;
                    let mut r = Rect::new(r.center().x - hw, r.bottom() + 0.02, hw * 2., 0.1);
                    self.btn_open_web.render_text(ui, r, t, ttl!("open-in-web"), 0.6, true);
                    r.y += r.h + 0.02;
                    if get_data().me.as_ref().map_or(false, |it| it.id == self.id) {
                        self.btn_logout.render_text(ui, r, t, tl!("logout"), 0.6, true);
                        r.y += r.h + 0.02;
                        self.btn_delete.render_text(ui, r, t, tl!("delete"), 0.6, true);
                    }
                    (ow, r.bottom() - oy + 0.04)
                });
            });
        } else {
            ui.loading(r.center().x, (r.y + r.bottom().min(ui.top)) / 2., t, WHITE, ());
        }

        let r = Rect::new(r.right() + 0.05, r.y, 0.9 - r.right(), 1.5);
        if let Some(items) = &mut self.record_items {
            self.fader.reset();
            self.fader.for_sub(|f| {
                ui.scope(|ui| {
                    ui.dx(r.x);
                    ui.dy(-ui.top);
                    let o = self.scroll.y_scroller.offset;
                    self.scroll.size((r.w, ui.top * 2.));
                    self.scroll.render(ui, |ui| {
                        let n = items.len();
                        let h = 0.2;
                        let pad = 0.02;
                        let mut iter = items.iter_mut();
                        for i in 0..((n + 1) / 2) {
                            for j in 0..(n - i * 2).min(2) {
                                let Some(item) = iter.next() else { unreachable!() };
                                f.render(ui, t, |ui| {
                                    let r = Rect::new(j as f32 * r.w / 2. + pad, r.y + ui.top + i as f32 * h, r.w / 2. - pad * 2., h - pad * 2.);
                                    if r.y - o > ui.top * 2. || r.bottom() - o < 0. {
                                        return;
                                    }
                                    item.illu.notify();
                                    item.btn.render_shadow(ui, r, t, |ui, path| {
                                        ui.fill_path(&path, (*item.illu.texture.0, r));
                                        ui.fill_path(&path, semi_black(0.6));
                                    });

                                    let icon = icon_index(item.record.score as _, item.record.full_combo);
                                    let s = r.h - pad * 2.;
                                    let ir = Rect::new(r.x + pad, r.y + pad, s, s);
                                    ui.fill_rect(ir, (*self.rank_icons[icon], ir, ScaleType::Fit));

                                    let lf = ir.right() + 0.02;

                                    if let Some(Ok(name)) = item.name.get().as_ref() {
                                        ui.text(name).pos(lf, ir.y).max_width(r.right() - lf - 0.03).size(0.56).draw();
                                    }

                                    ui.text(format!("{:07} {}", item.record.score, if item.record.full_combo { "[FC]" } else { "" }))
                                        .pos(lf, ir.bottom() - 0.02)
                                        .anchor(0., 1.)
                                        .size(0.6)
                                        .color(semi_white(0.6))
                                        .draw();
                                });
                            }
                        }
                        (r.w, r.y + ui.top + h * ((n + 1) / 2) as f32 + 0.04)
                    })
                });
            });
        } else {
            let ct = r.center();
            ui.loading(ct.x, ct.y, t, WHITE, ());
        }

        self.sf.render(ui, t);

        if self.avatar_task.is_some() {
            ui.full_loading(tl!("uploading-avatar"), t);
        }
        Ok(())
    }

    fn next_scene(&mut self, tm: &mut TimeManager) -> NextScene {
        self.sf.next_scene(tm.now() as f32).unwrap_or_default()
    }
}
