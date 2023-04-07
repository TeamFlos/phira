prpr::tl_file!("home");

use super::{LibraryPage, NextPage, Page, ResPackPage, SFader, SettingsPage, SharedState};
use crate::{
    client::{Client, LoginParams, User, UserManager},
    get_data, get_data_mut,
    login::Login,
    save_data,
    scene::{ProfileScene, TEX_ICON_BACK},
    sync_data,
};
use anyhow::Result;
use macroquad::prelude::*;
use prpr::{
    ext::{semi_black, semi_white, RectExt, SafeTexture, ScaleType},
    scene::{show_error, show_message, NextScene},
    task::Task,
    ui::{button_hit_large, DRectButton, Ui},
};

pub struct HomePage {
    character: SafeTexture,
    icon_play: SafeTexture,
    icon_medal: SafeTexture,
    icon_respack: SafeTexture,
    icon_msg: SafeTexture,
    icon_settings: SafeTexture,
    icon_back: SafeTexture,
    icon_lang: SafeTexture,
    icon_download: SafeTexture,
    icon_user: SafeTexture,
    icon_info: SafeTexture,
    icon_delete: SafeTexture,

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
}

impl HomePage {
    pub async fn new() -> Result<Self> {
        let character = SafeTexture::from(load_texture("char.png").await?).with_mipmap();
        let update_task = if let Some(u) = &get_data().me {
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
        Ok(Self {
            character,
            icon_play: load_texture("resume.png").await?.into(),
            icon_medal: load_texture("medal.png").await?.into(),
            icon_respack: load_texture("respack.png").await?.into(),
            icon_msg: load_texture("message.png").await?.into(),
            icon_settings: load_texture("settings.png").await?.into(),
            icon_lang: load_texture("language.png").await?.into(),
            icon_back: TEX_ICON_BACK.with(|it| it.borrow().clone().unwrap()),
            icon_download: load_texture("download.png").await?.into(),
            icon_user: load_texture("user.png").await?.into(),
            icon_info: load_texture("info.png").await?.into(),
            icon_delete: load_texture("delete.png").await?.into(),

            btn_play: DRectButton::new().with_delta(-0.01).no_sound(),
            btn_event: DRectButton::new().with_elevation(0.002).no_sound(),
            btn_respack: DRectButton::new().with_elevation(0.002).no_sound(),
            btn_msg: DRectButton::new().with_radius(0.03).with_delta(-0.003).with_elevation(0.002),
            btn_settings: DRectButton::new().with_radius(0.03).with_delta(-0.003).with_elevation(0.002),
            btn_user: DRectButton::new().with_delta(-0.003),

            next_page: None,

            login: Login::new(),
            update_task,

            need_back: false,
            sf: SFader::new(),
        })
    }
}

impl Page for HomePage {
    fn label(&self) -> std::borrow::Cow<'static, str> {
        "PHIRA".into()
    }

    fn enter(&mut self, s: &mut SharedState) -> Result<()> {
        if self.need_back {
            self.sf.enter(s.t);
            self.need_back = false;
        }
        Ok(())
    }

    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        if self.sf.transiting() {
            return Ok(true);
        }
        let t = s.t;
        if self.login.touch(touch, s.t) {
            return Ok(true);
        }
        if self.btn_play.touch(touch, t) {
            button_hit_large();
            self.next_page =
                Some(NextPage::Overlay(Box::new(LibraryPage::new(self.icon_back.clone(), self.icon_play.clone(), self.icon_download.clone())?)));
            return Ok(true);
        }
        if self.btn_event.touch(touch, t) {
            button_hit_large();
            show_message(tl!("not-opened")).warn();
            return Ok(true);
        }
        if self.btn_respack.touch(touch, t) {
            button_hit_large();
            self.next_page = Some(NextPage::Overlay(Box::new(ResPackPage::new(self.icon_info.clone(), self.icon_delete.clone())?)));
            return Ok(true);
        }
        if self.btn_msg.touch(touch, t) {
            return Ok(true);
        }
        if self.btn_settings.touch(touch, t) {
            self.next_page = Some(NextPage::Overlay(Box::new(SettingsPage::new(self.icon_lang.clone()))));
            return Ok(true);
        }
        if self.btn_user.touch(touch, t) {
            if let Some(me) = &get_data().me {
                self.need_back = true;
                self.sf.goto(t, ProfileScene::new(me.id));
            } else {
                self.login.enter(t);
            }
            return Ok(true);
        }
        Ok(false)
    }

    fn update(&mut self, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        self.login.update(t)?;
        if let Some(task) = &mut self.update_task {
            if let Some(res) = task.take() {
                if let Err(err) = res {
                    // wtf bro
                    if format!("{err:?}").contains("invalid token") {
                        get_data_mut().me = None;
                        get_data_mut().tokens = None;
                        let _ = save_data();
                        sync_data();
                    }
                    show_error(err.context(tl!("failed-to-update")));
                }
                self.update_task = None;
            }
        }
        Ok(())
    }

    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        let pad = 0.04;

        s.render_fader(ui, |ui, c| {
            let r = Rect::new(-1., -ui.top + 0.1, 1., 1.7);
            ui.fill_rect(r, (*self.character, r, ScaleType::CropCenter, c));
        });

        // play button
        let top = s.render_fader(ui, |ui, c| {
            let r = Rect::new(0., -0.28, 0.8, 0.43);
            let top = r.bottom() + 0.02;
            let (r, path) = self.btn_play.render_shadow(ui, r, t, c.a, |_| semi_black(0.4 * c.a));
            // ui.fill_path(&path, (semi_black(0.7 * c.a), (r.x, r.y), Color::default(), (r.x + 0.6, r.y)));
            ui.text(tl!("play")).pos(r.x + pad, r.y + pad).color(c).draw();
            let r = Rect::new(r.x + 0.02, r.bottom() - 0.18, 0.17, 0.17);
            ui.fill_rect(r, (*self.icon_play, r, ScaleType::Fit, semi_white(0.6 * c.a)));
            top
        });

        let text_and_icon = |ui: &mut Ui, r: Rect, btn: &mut DRectButton, text, icon, c: Color| {
            let ow = r.w;
            let (r, _) = btn.render_shadow(ui, r, t, c.a, |_| semi_black(0.4 * c.a));
            let ir = Rect::new(r.x + 0.02, r.bottom() - 0.08, 0.14, 0.14);
            ui.text(text).pos(r.x + 0.026, r.y + 0.026).size(0.7 * r.w / ow).color(c).draw();
            ui.fill_rect(
                {
                    let mut ir = ir;
                    ir.h = ir.h.min(r.bottom() - ir.y);
                    ir
                },
                (icon, ir, ScaleType::Fit, semi_white(0.4 * c.a)),
            );
        };

        let r = s.render_fader(ui, |ui, c| {
            let r = Rect::new(0., top, 0.38, 0.23);
            text_and_icon(ui, r, &mut self.btn_event, tl!("event"), *self.icon_medal, c);
            r
        });

        let r = s.render_fader(ui, |ui, c| {
            let r = Rect::new(r.right() + 0.02, top, 0.27, 0.23);
            text_and_icon(ui, r, &mut self.btn_respack, tl!("respack"), *self.icon_respack, c);
            r
        });

        let lf = r.right() + 0.02;

        s.render_fader(ui, |ui, c| {
            let r = Rect::new(lf, top, 0.11, 0.11);
            let (r, _) = self.btn_msg.render_shadow(ui, r, t, c.a, |_| semi_black(0.4 * c.a));
            let r = r.feather(-0.01);
            ui.fill_rect(r, (*self.icon_msg, r, ScaleType::Fit, c));

            let r = Rect::new(lf, top + 0.12, 0.11, 0.11);
            let (r, _) = self.btn_settings.render_shadow(ui, r, t, c.a, |_| semi_black(0.4 * c.a));
            let r = r.feather(0.004);
            ui.fill_rect(r, (*self.icon_settings, r, ScaleType::Fit, c));
        });

        s.fader.roll_back();
        s.render_fader(ui, |ui, c| {
            let rad = 0.05;
            let ct = (0.9, -ui.top + 0.1);
            self.btn_user.config.radius = rad;
            let r = Rect::new(ct.0, ct.1, 0., 0.).feather(rad);
            let (r, _) = self.btn_user.build(ui, t, r);
            ui.avatar(
                ct.0,
                ct.1,
                r.w / 2.,
                c,
                t,
                get_data()
                    .me
                    .as_ref()
                    .map(|user| Ok(UserManager::get_avatar(user.id)))
                    .unwrap_or(Err(self.icon_user.clone())),
            );
            let rt = ct.0 - rad - 0.02;
            if let Some(me) = &get_data().me {
                ui.text(&me.name).pos(rt, r.center().y + 0.002).anchor(1., 1.).size(0.6).color(c).draw();
                ui.text(format!("RKS {:.2}", me.rks))
                    .pos(rt, r.center().y + 0.008)
                    .anchor(1., 0.)
                    .size(0.4)
                    .color(Color { a: c.a * 0.6, ..c })
                    .draw();
            } else {
                ui.text(tl!("not-logged-in"))
                    .pos(rt, r.center().y)
                    .anchor(1., 0.5)
                    .no_baseline()
                    .size(0.6)
                    .color(c)
                    .draw();
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
