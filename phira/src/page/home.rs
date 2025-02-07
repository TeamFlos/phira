prpr_l10n::tl_file!("home");

use super::{
    load_font_with_cksum, set_bold_font, EventPage, LibraryPage, MessagePage, NextPage, Page, ResPackPage, SFader, SettingsPage, SharedState,
    BOLD_FONT_CKSUM,
};
use crate::{
    anim::Anim,
    client::{recv_raw, Character, Client, LoginParams, User, UserManager},
    dir, get_data, get_data_mut,
    icons::Icons,
    login::Login,
    save_data,
    scene::{check_read_tos_and_policy, ProfileScene, JUST_LOADED_TOS},
    sync_data,
    threed::ThreeD,
    ttl,
};
use ::rand::{random, thread_rng, Rng};
use anyhow::{bail, Context, Result};
use chrono::NaiveDate;
use image::DynamicImage;
use macroquad::prelude::*;
use prpr::{
    core::BOLD_FONT,
    ext::{open_url, screen_aspect, semi_black, semi_white, RectExt, SafeTexture, ScaleType},
    info::ChartInfo,
    scene::{show_error, NextScene},
    task::Task,
    ui::{button_hit_large, clip_rounded_rect, ClipType, DRectButton, Dialog, FontArc, RectButton, Scroll, Ui},
};
use prpr_l10n::LANG_IDENTS;
use reqwest::StatusCode;
use serde::Deserialize;
use std::{
    borrow::Cow,
    sync::{atomic::Ordering, Arc},
};
use tap::Tap;
use tracing::{info, warn};

const BOARD_SWITCH_TIME: f32 = 4.;
const BOARD_TRANSIT_TIME: f32 = 1.2;

#[derive(Deserialize)]
struct Version {
    version: semver::Version,
    date: NaiveDate,
    description: String,
    url: String,
}

pub struct HomePage {
    icons: Arc<Icons>,

    btn_play: DRectButton,
    btn_event: DRectButton,
    btn_respack: DRectButton,
    btn_msg: DRectButton,
    btn_settings: DRectButton,
    btn_user: DRectButton,

    next_page: Option<NextPage>,

    login: Login,
    update_task: Option<Task<Result<User>>>,

    need_back: bool,
    sf: SFader,

    board_task: Option<Task<Result<Option<DynamicImage>>>>,
    board_last_time: f32,
    board_last: Option<String>,
    board_tex_last: Option<SafeTexture>,
    board_tex: Option<SafeTexture>,
    board_dir: bool,

    has_new_task: Option<Task<Result<bool>>>,
    has_new: bool,

    check_update_task: Option<Task<Result<Option<Version>>>>,
    check_bold_font_update_task: Option<Task<Result<Option<(FontArc, String)>>>>,

    btn_play_3d: ThreeD,
    btn_other_3d: ThreeD,

    character: Character,
    char_appear_p: Anim<f32>,
    char_last_illu: Option<String>,
    char_last_user_id: Option<i32>,
    char_fetch_task: Option<Task<Result<Character>>>,
    char_illu: Option<SafeTexture>,
    char_illu_task: Option<Task<Result<DynamicImage>>>,
    // progress of character screen
    char_screen_p: Anim<f32>,
    char_btn: RectButton,
    char_text_start: f32,
    char_cached_size: f32,
    char_scroll: Scroll,
    char_edit_btn: RectButton,

    #[cfg(feature = "aa")]
    beian_btn: RectButton,
}

impl HomePage {
    pub async fn new() -> Result<Self> {
        let update_task = if get_data().config.offline_mode {
            None
        } else if let Some(u) = &get_data().me {
            UserManager::request(u.id);
            Some(Task::new(async {
                Client::login(LoginParams::RefreshToken {
                    token: &get_data().tokens.as_ref().unwrap().1,
                })
                .await?;
                Client::get_me().await
            }))
        } else {
            None
        };

        let flavor = match load_file("flavor").await.map(String::from_utf8) {
            Ok(Ok(flavor)) => flavor.trim().to_owned(),
            _ => "none".to_owned(),
        };

        let mut res = Self {
            icons: Arc::new(Icons::new().await?),

            btn_play: DRectButton::new().with_delta(-0.01).no_sound(),
            btn_event: DRectButton::new().with_elevation(0.002).no_sound(),
            btn_respack: DRectButton::new().with_elevation(0.002).no_sound(),
            btn_msg: DRectButton::new().with_radius(0.008).with_delta(-0.003).with_elevation(0.002),
            btn_settings: DRectButton::new().with_radius(0.008).with_delta(-0.003).with_elevation(0.002),
            btn_user: DRectButton::new().with_delta(-0.003),

            next_page: None,

            login: Login::new(),
            update_task,

            need_back: false,
            sf: SFader::new(),

            board_task: None,
            board_last_time: f32::NEG_INFINITY,
            board_last: None,
            board_tex_last: None,
            board_tex: None,
            board_dir: false,

            has_new_task: None,
            has_new: false,

            check_update_task: Some(Task::new(async move {
                Ok(recv_raw(Client::get("/check-update").query(&[("version", env!("CARGO_PKG_VERSION")), ("flavor", &flavor)]))
                    .await?
                    .json()
                    .await?)
            })),
            check_bold_font_update_task: {
                let cksum = BOLD_FONT_CKSUM.with(|it| it.borrow().clone());
                Some(Task::new(async move {
                    let resp = Client::get("/font-bold").query(&[("cksum", cksum)]).send().await?;
                    if resp.status() == StatusCode::NOT_MODIFIED {
                        info!("bold font not modified");
                        return Ok(None);
                    }
                    if !resp.status().is_success() {
                        let status = resp.status().as_str().to_owned();
                        let text = resp.text().await.context("failed to receive text")?;
                        if let Ok(what) = serde_json::from_str::<serde_json::Value>(&text) {
                            if let Some(detail) = what["detail"].as_str() {
                                bail!("request failed ({status}): {detail}");
                            }
                        }
                        bail!("request failed ({status}): {text}");
                    }
                    info!("downloading new bold font");
                    let bytes = resp.bytes().await?;
                    std::fs::write(dir::bold_font_path()?, &bytes).context("failed to save font")?;
                    Ok(Some(load_font_with_cksum(bytes.to_vec())?))
                }))
            },

            btn_play_3d: ThreeD::new(),
            btn_other_3d: ThreeD::new().tap_mut(|it| {
                it.anchor = vec2(0.2, -0.2);
                it.angle = 0.14;
                it.sync();
            }),

            character: get_data().character.clone().unwrap_or_default(),
            char_appear_p: Anim::new(0.),
            char_last_illu: None,
            char_last_user_id: None,
            char_fetch_task: None,
            char_illu: None,
            char_illu_task: None,
            char_screen_p: Anim::new(0.),
            char_btn: RectButton::new(),
            char_text_start: 0.,
            char_cached_size: 0.,
            char_scroll: Scroll::new().use_clip(ClipType::Clip),
            char_edit_btn: RectButton::new(),

            #[cfg(feature = "aa")]
            beian_btn: RectButton::new(),
        };
        res.load_char_illu();

        Ok(res)
    }
}

impl HomePage {
    fn load_char_illu(&mut self) {
        let key = if self.character.illust == "@" {
            format!("@{}", self.character.id)
        } else {
            self.character.illust.clone()
        };
        if self.char_last_illu.as_ref() == Some(&key) {
            return;
        }
        self.char_last_illu = Some(key);

        self.char_appear_p.set(0.);

        #[cfg(feature = "closed")]
        if self.character.illust == "@" {
            let id = self.character.id.clone();
            self.char_illu_task =
                Some(Task::new(
                    async move { Ok(image::load_from_memory(&crate::inner::resolve_data(load_file(&format!("res/{id}.char")).await?))?) },
                ));
        } else {
            let file = crate::page::File {
                url: self.character.illust.clone(),
            };
            self.char_illu_task =
                Some(Task::new(async move { Ok(image::load_from_memory(&crate::inner::resolve_data(file.fetch().await?.to_vec()))?) }));
        }
    }

    fn fetch_has_new(&mut self) {
        let time = get_data().message_check_time.unwrap_or_default();
        self.has_new_task = Some(Task::new(async move {
            #[derive(Deserialize)]
            struct Resp {
                has: bool,
            }
            let resp: Resp = recv_raw(Client::get("/message/has_new").query(&[("checked", time)]))
                .await?
                .json()
                .await?;
            Ok(resp.has)
        }));
    }

    fn render_not_char(&mut self, ui: &mut Ui, s: &mut SharedState) {
        let t = s.t;

        let pad = 0.04;
        // play button
        let r = Rect::new(0., -0.33, 0.83, 0.45);
        let mat = self.btn_play_3d.now(ui, r, t);
        let top = ui.with_gl(mat, |ui| {
            s.render_fader(ui, |ui| {
                let top = r.bottom() + 0.02;
                let rad = self.btn_play.config.radius;
                self.btn_play.render_shadow(ui, r, t, |ui, path| {
                    ui.fill_path(&path, semi_black(0.4));
                    if let Some(cur) = &self.board_tex {
                        let p = (t - self.board_last_time) / BOARD_TRANSIT_TIME;
                        if p > 1. {
                            self.board_tex_last = None;
                            ui.fill_path(&path, (**cur, r));
                        } else if let Some(last) = &self.board_tex_last {
                            let (cur, last) = if self.board_dir { (last, cur) } else { (cur, last) };
                            let p = 1. - (1. - p).powi(3);
                            let p = if self.board_dir { 1. - p } else { p };
                            clip_rounded_rect(ui, r, rad, |ui| {
                                let mut nr = r;
                                nr.h = r.h * (1. - p);
                                ui.fill_rect(nr, (**last, nr));

                                nr.h = r.h * p;
                                nr.y = r.bottom() - nr.h;
                                ui.fill_rect(nr, (**cur, nr));
                            });
                        } else {
                            ui.fill_path(&path, (**cur, r, ScaleType::CropCenter, semi_white(p)));
                        }
                    }
                    ui.fill_path(&path, (semi_black(0.7), (r.x, r.y), Color::default(), (r.x + 0.6, r.y)));
                    ui.text(tl!("play")).pos(r.x + pad, r.y + pad).draw();
                    let r = Rect::new(r.x + 0.02, r.bottom() - 0.18, 0.17, 0.17);
                    ui.fill_rect(r, (*self.icons.play, r, ScaleType::Fit, semi_white(0.6)));
                });
                top + 0.03
            })
        });

        let text_and_icon = |s: &mut SharedState, ui: &mut Ui, r: Rect, btn: &mut DRectButton, text, icon| {
            let ow = r.w;
            s.render_fader(ui, |ui| {
                btn.render_shadow(ui, r, t, |ui, path| {
                    ui.fill_path(&path, semi_black(0.4));
                    let ir = Rect::new(r.x + 0.02, r.bottom() - 0.08, 0.14, 0.14);
                    ui.text(text).pos(r.x + 0.026, r.y + 0.026).size(0.7 * r.w / ow).draw();
                    ui.fill_rect(
                        {
                            let mut ir = ir;
                            ir.h = ir.h.min(r.bottom() - ir.y);
                            ir
                        },
                        (icon, ir, ScaleType::Fit, semi_white(0.4)),
                    );
                });
            });
        };

        let mat = self.btn_other_3d.now(ui, Rect::new(0., top - 0.4, 0.83, 0.23), t);
        ui.with_gl(mat, |ui| {
            let r = Rect::new(0., top, 0.38, 0.23);
            text_and_icon(s, ui, r, &mut self.btn_event, tl!("event"), *self.icons.medal);

            let r = Rect::new(r.right() + 0.02, top, 0.29, 0.23);
            text_and_icon(s, ui, r, &mut self.btn_respack, tl!("respack"), *self.icons.respack);

            let lf = r.right() + 0.02;

            s.render_fader(ui, |ui| {
                let r = Rect::new(lf, top, 0.11, 0.11);
                self.btn_msg.render_shadow(ui, r, t, |ui, path| {
                    ui.fill_path(&path, semi_black(0.4));
                    let r = r.feather(-0.01);
                    ui.fill_rect(r, (*self.icons.msg, r, ScaleType::Fit));
                    if self.has_new {
                        let pad = 0.007;
                        ui.fill_circle(r.right() - pad, r.y + pad, 0.01, RED);
                    }
                });

                let r = Rect::new(lf, top + 0.12, 0.11, 0.11);
                self.btn_settings.render_shadow(ui, r, t, |ui, path| {
                    ui.fill_path(&path, semi_black(0.4));
                    let r = r.feather(0.004);
                    ui.fill_rect(r, (*self.icons.settings, r, ScaleType::Fit));
                });
            });
        });
    }
}

impl Page for HomePage {
    fn label(&self) -> Cow<'static, str> {
        "PHIRA".into()
    }

    fn enter(&mut self, s: &mut SharedState) -> Result<()> {
        if self.need_back {
            self.sf.enter(s.t);
            self.need_back = false;
        }
        self.fetch_has_new();
        Ok(())
    }

    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        if self.sf.transiting() {
            return Ok(true);
        }
        let t = s.t;
        let rt = s.rt;
        if self.login.touch(touch, s.t) {
            return Ok(true);
        }
        if self.char_screen_p.now(rt) < 1e-2 {
            self.btn_play_3d.touch(touch, t);
            if self.btn_play.touch(touch, t) {
                button_hit_large();
                self.next_page = Some(NextPage::Overlay(Box::new(LibraryPage::new(Arc::clone(&self.icons), s.icons.clone())?)));
                return Ok(true);
            }
            if self.btn_event.touch(touch, t) {
                button_hit_large();
                if get_data().me.is_none() {
                    self.login.enter(t);
                } else {
                    self.next_page = Some(NextPage::Overlay(Box::new(EventPage::new(Arc::clone(&self.icons), s.icons.clone()))));
                }
                return Ok(true);
            }
            if self.btn_respack.touch(touch, t) {
                button_hit_large();
                self.next_page = Some(NextPage::Overlay(Box::new(ResPackPage::new(Arc::clone(&self.icons))?)));
                return Ok(true);
            }
            if self.btn_msg.touch(touch, t) {
                self.next_page = Some(NextPage::Overlay(Box::new(MessagePage::new())));
                return Ok(true);
            }
            if self.btn_settings.touch(touch, t) {
                self.next_page = Some(NextPage::Overlay(Box::new(SettingsPage::new(self.icons.icon.clone(), self.icons.lang.clone()))));
                return Ok(true);
            }
        } else {
            if self.char_scroll.touch(touch, t) {
                return Ok(true);
            }
            if self.char_edit_btn.touch(touch) {
                let _ = open_url("https://phira.moe/settings/account");
            }
        }
        if self.btn_user.touch(touch, t) {
            if let Some(me) = &get_data().me {
                self.need_back = true;
                self.sf.goto(t, ProfileScene::new(me.id, self.icons.user.clone(), s.icons.clone()));
            } else {
                self.login.enter(t);
            }
            return Ok(true);
        }
        #[cfg(feature = "aa")]
        if self.beian_btn.touch(touch) {
            let _ = open_url("https://beian.miit.gov.cn/#/home");
            return Ok(true);
        }
        if self.char_btn.touch(touch) {
            if !self.char_screen_p.transiting(rt) {
                let to = if self.char_screen_p.now(rt) < 0.5 {
                    self.char_text_start = rt;
                    1.
                } else {
                    0.
                };
                self.char_screen_p.goto(to, rt, 0.5);
            }
            return Ok(true);
        }
        Ok(false)
    }

    fn update(&mut self, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        self.login.update(t)?;
        let current_user = Some(get_data().me.as_ref().map_or(-1, |it| it.id));
        self.char_scroll.update(t);
        if self.char_last_user_id != current_user {
            let locale = get_data().language.clone().unwrap_or(LANG_IDENTS[0].to_string());
            self.char_last_user_id = current_user;
            self.char_fetch_task =
                Some(Task::new(async move { Ok(recv_raw(Client::get("/me/char").query(&[("locale", locale)])).await?.json().await?) }));
        }
        if let Some(task) = &mut self.update_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        // wtf bro
                        if format!("{err:?}").contains("invalid token") {
                            get_data_mut().me = None;
                            get_data_mut().tokens = None;
                            let _ = save_data();
                            sync_data();
                        }
                        // TODO: better error handling
                        show_error(err.context(tl!("failed-to-update") + "\n" + tl!("note-try-login-again")));
                    }
                    Ok(val) => {
                        get_data_mut().me = Some(val);
                        save_data()?;
                    }
                }
                self.update_task = None;
            }
        }
        if self.board_task.is_none() && t - self.board_last_time > BOARD_SWITCH_TIME {
            let charts = &get_data().charts;
            let last_index = self
                .board_last
                .as_ref()
                .and_then(|path| charts.iter().position(|it| &it.local_path == path));
            if charts.is_empty() || (charts.len() == 1 && last_index.is_some()) {
                self.board_task = Some(Task::new(async move { Ok(None) }));
            } else {
                let mut index = thread_rng().gen_range(0..(charts.len() - last_index.is_some() as usize));
                if last_index.map_or(false, |it| it <= index) {
                    index += 1;
                }
                let path = charts[index].local_path.clone();
                let dir = prpr::dir::Dir::new(format!("{}/{}", dir::charts()?, path))?;
                self.board_last = Some(path);
                self.board_task = Some(Task::new(async move {
                    let info: ChartInfo = serde_yaml::from_reader(dir.open("info.yml")?)?;
                    let bytes = dir.read(info.illustration)?;
                    Ok(Some(image::load_from_memory(&bytes)?))
                }));
            }
        }
        if let Some(task) = &mut self.board_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        warn!(?err, "failed to load illustration for board");
                    }
                    Ok(image) => {
                        if let Some(image) = image {
                            let tex: SafeTexture = image.into();
                            self.board_tex_last = std::mem::replace(&mut self.board_tex, Some(tex));
                            self.board_dir = random();
                        }
                    }
                }
                self.board_last_time = t;
                self.board_task = None;
            }
        }
        if let Some(task) = &mut self.has_new_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        warn!("fail to load has new {:?}", err);
                    }
                    Ok(has) => {
                        self.has_new = has;
                    }
                }
                self.has_new_task = None;
            }
        }
        if let Some(task) = &mut self.check_update_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        warn!("fail to check update {:?}", err);
                    }
                    Ok(Some(ver)) => {
                        if get_data().ignored_version.as_ref().map_or(true, |it| it < &ver.version) {
                            Dialog::plain(
                                tl!("update", "version" => ver.version.to_string()),
                                tl!("update-desc", "date" => ver.date.to_string(), "desc" => ver.description),
                            )
                            .buttons(vec![
                                ttl!("cancel").into_owned(),
                                tl!("update-ignore").into_owned(),
                                tl!("update-go").into_owned(),
                            ])
                            .listener(move |_dialog, pos| {
                                match pos {
                                    1 => {
                                        get_data_mut().ignored_version = Some(ver.version.clone());
                                        let _ = save_data();
                                    }
                                    2 => {
                                        let _ = open_url(&ver.url);
                                    }
                                    _ => {}
                                }
                                false
                            })
                            .show();
                        }
                    }
                    _ => {}
                }
                self.check_update_task = None;
            }
        }
        if let Some(task) = &mut self.check_bold_font_update_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        warn!("fail to check bold font update {:?}", err);
                    }
                    Ok(None) => {}
                    Ok(Some(parsed)) => {
                        info!(cksum = parsed.1, "new bold font");
                        set_bold_font(parsed);
                    }
                }
                self.check_bold_font_update_task = None;
            }
        }
        if let Some(task) = &mut self.char_illu_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        warn!(?err, "fail to load char illu");
                    }
                    Ok(image) => {
                        self.char_appear_p.goto(1., t, 0.5);
                        let tex: SafeTexture = image.into();
                        self.char_illu = Some(tex.with_mipmap());
                    }
                }
                self.char_illu_task = None;
            }
        }
        if let Some(task) = &mut self.char_fetch_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        warn!(?err, "fail to load char");
                    }
                    Ok(char) => {
                        info!(?char, "char loaded");
                        self.character = char;
                        get_data_mut().character = Some(self.character.clone());
                        let _ = save_data();
                        self.char_cached_size = 0.;
                        self.load_char_illu();
                    }
                }
                self.char_fetch_task = None;
            }
        }
        if JUST_LOADED_TOS.fetch_and(false, Ordering::Relaxed) {
            check_read_tos_and_policy(true, true);
        }

        Ok(())
    }

    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        let rt = s.rt;

        let cp = self.char_screen_p.now(rt);
        s.render_fader(ui, |ui| {
            let r = Rect::new(-1. + 0.14 * cp, -ui.top + 0.12, 1., 1.7);
            if let Some(illu) = &self.char_illu {
                let p = self.char_appear_p.now(t);
                let (ox, oy, ow, oh) = self.character.illu_adjust;
                let r = Rect::new(r.x + ox, r.y + (1. - p) * 0.05 + oy, r.w + ow, r.h + oh);
                ui.fill_rect(ui.screen_rect(), (**illu, r, ScaleType::CropCenter, semi_white(p)));
            }
            self.char_btn.set(ui, r);

            if cp > 1e-5 {
                let height = 0.8 - ((screen_aspect() - 16. / 9.) * 0.2).min(0.2);
                let r = Rect::new(0.16, (-height - height * cp) / 4., 0.6, height);
                let mat = ThreeD::build(vec2(0., 0.), r, 0.12);
                let gl = unsafe { get_internal_gl() }.quad_gl;
                gl.push_model_matrix(mat);

                ui.alpha(cp, |ui| {
                    let mut r = Rect::new(r.x, r.y + 0.14, r.w, r.h - 0.14);
                    ui.fill_rect(r, semi_black(0.3));
                    ui.fill_rect(Rect::new(r.x, r.y, 0.01, r.h), WHITE);
                    let mut t = ui.text(tl!("change-char")).pos(r.x + 0.01, r.bottom() + 0.015).size(0.3);
                    let ir = t.measure().feather(0.007);
                    t.ui.fill_rect(ir, semi_black(0.2));
                    self.char_edit_btn.set(t.ui, ir);
                    t.draw();
                    let pad = 0.01;

                    let mut t = ui
                        .text(self.character.name_en())
                        .pos(r.right() - pad, r.bottom() - pad)
                        .anchor(1., 1.)
                        .color(semi_white(0.2));
                    if self.char_cached_size < 1e-6 {
                        let mut initial = 2.;
                        loop {
                            t = t.size(initial);
                            if t.measure().w < r.w * 0.7 {
                                break;
                            }
                            initial *= 0.95;
                        }
                        self.char_cached_size = initial;
                    } else {
                        t = t.size(self.char_cached_size);
                    }
                    t.draw();

                    r.x += 0.01;
                    r.w -= 0.01;

                    self.char_scroll.size((r.w, r.h));
                    ui.scope(|ui| {
                        ui.dx(r.x);
                        ui.dy(r.y);
                        let ow = r.w;
                        self.char_scroll.render(ui, |ui| {
                            let r = Rect::new(0., 0., r.w, r.h);
                            let r = r.feather(-0.03);
                            let r = ui.text(&self.character.intro).pos(r.x, r.y).max_width(r.w).multiline().size(0.4).draw();
                            (ow, r.h + 0.1)
                        });
                    });
                });

                let r = Rect::new(r.x, r.y, 0.4, 0.12);

                ui.alpha(cp, |ui| {
                    let r = ui
                        .text(&self.character.name)
                        .pos(r.x + (1. - cp) * 0.12 + 0.01, r.center().y)
                        .anchor(0., 0.5)
                        .size(self.character.name_size.unwrap_or(1.4))
                        .draw_using(&BOLD_FONT);

                    let off = if self.character.baseline { 0. } else { 0.01 };
                    ui.text(format!("Artist: {}", self.character.artist))
                        .pos(r.right() + (1. - cp) * 0.1 + 0.02, r.bottom() + off - 0.03)
                        .anchor(0., 1.)
                        .size(0.34)
                        .color(semi_white(0.7))
                        .draw();
                    ui.text(format!("Designer: {}", self.character.designer))
                        .pos(r.right() + (1. - cp) * 0.1 + 0.016, r.bottom() + off)
                        .anchor(0., 1.)
                        .size(0.34)
                        .color(semi_white(0.7))
                        .draw();
                });

                gl.pop_model_matrix();
            }
        });

        ui.alpha(1. - cp, |ui| {
            self.render_not_char(ui, s);
        });

        s.fader.roll_back();
        s.render_fader(ui, |ui| {
            let rad = 0.05;
            let ct = (0.92, -ui.top + 0.08);
            self.btn_user.config.radius = rad;
            let r = Rect::new(ct.0, ct.1, 0., 0.).feather(rad);
            self.btn_user.build(ui, t, r, |ui, _| {
                ui.avatar(
                    ct.0,
                    ct.1,
                    r.w / 2.,
                    t,
                    get_data()
                        .me
                        .as_ref()
                        .map(|user| UserManager::opt_avatar(user.id, &self.icons.user))
                        .unwrap_or(Err(self.icons.user.clone())),
                );
            });
            let rt = ct.0 - rad - 0.02;
            if let Some(me) = &get_data().me {
                ui.text(&me.name).pos(rt, r.center().y + 0.002).anchor(1., 1.).size(0.6).draw();
                ui.text(format!("RKS {:.2}", me.rks))
                    .pos(rt, r.center().y + 0.008)
                    .anchor(1., 0.)
                    .size(0.4)
                    .color(semi_white(0.6))
                    .draw();
            } else {
                ui.text(tl!("not-logged-in"))
                    .pos(rt, r.center().y)
                    .anchor(1., 0.5)
                    .no_baseline()
                    .size(0.6)
                    .draw();
            }

            #[cfg(feature = "aa")]
            {
                let r = ui.screen_rect();
                let r = ui
                    .text("备案号：闽ICP备18008307号-64A")
                    .pos(r.x + 0.02, r.bottom() - 0.03)
                    .size(0.5)
                    .anchor(0., 1.)
                    .draw();
                self.beian_btn.set(ui, r);
            }
        });

        self.login.render(ui, t);
        self.sf.render(ui, t);

        Ok(())
    }

    fn next_page(&mut self) -> NextPage {
        self.next_page.take().unwrap_or_default()
    }

    fn next_scene(&mut self, s: &mut SharedState) -> NextScene {
        self.sf.next_scene(s.t).unwrap_or_default()
    }
}
