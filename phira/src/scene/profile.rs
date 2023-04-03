prpr::tl_file!("profile");

use super::{TEX_BACKGROUND, TEX_ICON_BACK};
use crate::{
    client::{Client, PZUser, UserManager},
    get_data, get_data_mut,
    page::SFader,
    save_data, sync_data,
};
use anyhow::Result;
use chrono::Utc;
use macroquad::prelude::*;
use prpr::{
    ext::{screen_aspect, RectExt, SafeTexture},
    scene::{show_error, show_message, NextScene, Scene},
    task::Task,
    time::TimeManager,
    ui::{button_hit, rounded_rect_shadow, DRectButton, RectButton, ShadowConfig, Ui},
};
use std::sync::Arc;

pub struct ProfileScene {
    id: i32,
    user: Option<Arc<PZUser>>,

    background: SafeTexture,

    icon_back: SafeTexture,
    btn_back: RectButton,

    btn_logout: DRectButton,

    load_task: Option<Task<Result<Arc<PZUser>>>>,

    sf: SFader,
}

impl ProfileScene {
    pub fn new(id: i32) -> Self {
        UserManager::request(id);

        let load_task = Some(Task::new(Client::load(id)));

        Self {
            id,
            user: None,

            background: TEX_BACKGROUND.with(|it| it.borrow().clone().unwrap()),

            icon_back: TEX_ICON_BACK.with(|it| it.borrow().clone().unwrap()),
            btn_back: RectButton::new(),

            btn_logout: DRectButton::new(),

            load_task,

            sf: SFader::new(),
        }
    }
}

impl Scene for ProfileScene {
    fn enter(&mut self, tm: &mut TimeManager, _target: Option<RenderTarget>) -> Result<()> {
        self.sf.enter(tm.now() as _);
        Ok(())
    }

    fn update(&mut self, tm: &mut TimeManager) -> Result<()> {
        if let Some(task) = &mut self.load_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => show_error(err.context(tl!("load-user-failed"))),
                    Ok(res) => {
                        self.user = Some(res);
                    }
                }
                self.load_task = None;
            }
        }
        Ok(())
    }

    fn touch(&mut self, tm: &mut TimeManager, touch: &Touch) -> Result<bool> {
        let t = tm.now() as f32;
        if self.btn_back.touch(touch) {
            button_hit();
            self.sf.next(t, NextScene::Pop);
            return Ok(true);
        }
        if self.btn_logout.touch(touch, t) {
            get_data_mut().me = None;
            get_data_mut().tokens = None;
            let _ = save_data();
            sync_data();
            show_message(tl!("logged-out")).ok();
            self.sf.next(t, NextScene::Pop);
            return Ok(true);
        }
        Ok(false)
    }

    fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        set_camera(&Camera2D {
            zoom: vec2(1., -screen_aspect()),
            ..Default::default()
        });
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
            let pad = 0.02;
            let mw = r.w - pad * 2.;
            let lf = r.x + pad;
            let cx = r.center().x;
            let radius = 0.12;
            let mut r = ui.avatar(cx, r.y + radius + 0.05, radius, WHITE, t, Ok(UserManager::get_avatar(self.id)));
            if get_data().me.as_ref().map_or(false, |it| it.id == self.id) {
                let hw = 0.2;
                r = Rect::new(r.center().x - hw, r.bottom() + 0.02, hw * 2., 0.1);
                self.btn_logout.render_text(ui, r, t, 1., tl!("logout"), 0.6, false);
            }
            let r = ui
                .text(&user.name)
                .size(0.74)
                .pos(cx, r.bottom() + 0.03)
                .anchor(0.5, 0.)
                .max_width(mw)
                .draw();
            let r = ui
                .text(format!("RKS {:.2}", user.rks))
                .size(0.5)
                .pos(cx, r.bottom() + 0.01)
                .anchor(0.5, 0.)
                .draw();
            let r = ui
                .text(user.bio.as_deref().unwrap_or(""))
                .pos(lf, r.y + 0.1)
                .multiline()
                .max_width(mw)
                .size(0.4)
                .draw();
        } else {
            ui.loading(r.center().x, (r.y + r.bottom().min(ui.top)) / 2., t, WHITE, ());
        }

        self.sf.render(ui, t);
        Ok(())
    }

    fn next_scene(&mut self, tm: &mut TimeManager) -> NextScene {
        self.sf.next_scene(tm.now() as f32).unwrap_or_default()
    }
}
