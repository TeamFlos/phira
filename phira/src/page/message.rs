prpr::tl_file!("message");

use std::borrow::Cow;

use super::{Page, SharedState};
use crate::{
    client::{recv_raw, Client, Message},
    get_data, get_data_mut, save_data,
};
use anyhow::Result;
use chrono::Local;
use macroquad::prelude::*;
use prpr::{
    ext::{semi_black, semi_white, RectExt},
    scene::show_error,
    task::Task,
    ui::{DRectButton, Scroll, Ui},
};

pub struct MessagePage {
    msgs: Option<Vec<(Message, DRectButton)>>,
    load_task: Option<Task<Result<Vec<Message>>>>,

    index: Option<usize>,

    btns_scroll: Scroll,
    scroll: Scroll,
}

impl MessagePage {
    pub fn new() -> Self {
        Self {
            msgs: None,
            load_task: None,

            index: None,

            btns_scroll: Scroll::new(),
            scroll: Scroll::new(),
        }
    }

    pub fn load(&mut self) {
        if self.load_task.is_some() {
            return;
        }
        let before = self.msgs.as_ref().and_then(|it| it.last().map(|it| it.0.time));
        self.load_task = Some(Task::new(async move {
            let mut req = Client::get("/message/list");
            if let Some(before) = before {
                req = req.query(&[("before", before)]);
            }
            Ok(recv_raw(req).await?.json().await?)
        }));
    }
}

impl Page for MessagePage {
    fn label(&self) -> Cow<'static, str> {
        tl!("label")
    }

    fn enter(&mut self, _s: &mut SharedState) -> Result<()> {
        self.load();
        Ok(())
    }

    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        let t = s.t;
        if self.load_task.is_none() {
            if self.btns_scroll.touch(touch, t) {
                return Ok(true);
            }
            if let Some(msgs) = &mut self.msgs {
                for (index, item) in msgs.iter_mut().enumerate() {
                    if item.1.touch(touch, t) {
                        if self.index == Some(index) {
                            self.index = None;
                        } else {
                            if get_data().message_check_time.map_or(true, |it| it < item.0.time) {
                                get_data_mut().message_check_time = Some(item.0.time);
                                save_data()?;
                            }
                            self.index = Some(index);
                        }
                        return Ok(true);
                    }
                }
            }
        }
        if self.scroll.touch(touch, t) {
            return Ok(true);
        }
        Ok(false)
    }

    fn update(&mut self, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        if self.btns_scroll.y_scroller.pulled_down {
            self.load();
        }
        self.btns_scroll.update(t);
        self.scroll.update(t);
        if let Some(task) = &mut self.load_task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => {
                        show_error(err.context(tl!("load-msg-fail")));
                    }
                    Ok(val) => {
                        let mt = match &mut self.msgs {
                            None => self.msgs.insert(Vec::new()),
                            Some(x) => x,
                        };
                        mt.extend(val.into_iter().map(|it| (it, DRectButton::new().with_delta(-0.001))));
                    }
                }
                self.load_task = None;
            }
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
            let ct = r.center();
            let pad = 0.014;
            self.btns_scroll.size((r.w, r.h - pad));
            if let Some(msgs) = &mut self.msgs {
                if msgs.is_empty() {
                    ui.text(tl!("no-msg")).pos(ct.x, ct.y).anchor(0.5, 0.5).no_baseline().size(0.8).draw();
                } else {
                    ui.scope(|ui| {
                        ui.dx(r.x);
                        ui.dy(r.y + pad);
                        self.btns_scroll.render(ui, |ui| {
                            let w = r.w - pad * 2.;
                            let mut h = 0.;
                            let r = Rect::new(pad, 0., r.w - pad * 2., 0.09);
                            for (index, item) in msgs.iter_mut().enumerate() {
                                item.1.render_text_left(ui, r, t, 1., &item.0.title, 0.5, Some(index) == self.index);
                                ui.dy(r.h + pad);
                                h += r.h + pad;
                            }
                            h += pad;
                            (w, h)
                        });
                    });
                }
            }
            if self.load_task.is_some() {
                ui.fill_path(&r.rounded(0.005), semi_white(0.3));
                ui.loading(ct.x, ct.y, t, WHITE, ());
            }
        });
        s.render_fader(ui, |ui| {
            ui.fill_path(&cr.rounded(0.005), semi_black(0.4));
            let pad = 0.03;
            ui.dx(cr.x + pad + 0.01);
            ui.dy(cr.y + pad);
            if let Some(msg) = self.index.and_then(|it| self.msgs.as_ref().map(|msgs| &msgs[it].0)) {
                let mw = cr.w - pad * 2. - 0.01;
                let mut h = 0.;
                macro_rules! dy {
                    ($e:expr) => {{
                        let e = $e;
                        ui.dy(e);
                        h += e;
                    }};
                }
                dy!(ui.text(&msg.title).size(0.9).multiline().max_width(mw).draw().h + 0.017);
                let th = ui.text(
                    tl!("subtitle", "author" => msg.author.as_str(), "time" => msg.time.with_timezone(&Local).format("%Y-%m-%d %H:%M").to_string()),
                )
                .pos(0.01, 0.)
                .size(0.4)
                .color(semi_white( 0.7))
                .draw().h;
                dy!(th + 0.016);
                ui.fill_rect(Rect::new(0., 0., mw, 0.006), semi_white(0.8));
                dy!(0.015);
                self.scroll.size((mw, cr.h - h - pad));
                self.scroll.render(ui, |ui| {
                    let r = ui.text(&msg.content).size(0.46).multiline().max_width(mw).draw();
                    (mw, r.h + 0.04)
                });
            }
        });
        Ok(())
    }
}
