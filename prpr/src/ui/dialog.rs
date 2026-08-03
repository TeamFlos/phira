prpr_l10n::tl_file!("dialog");

use super::{DRectButton, RectButton, Scroll, Ui};
use crate::{core::BOLD_FONT, ext::RectExt, scene::show_message};
use anyhow::Error;
use macroquad::prelude::*;

const WIDTH_RADIO: f32 = 0.5;
const HEIGHT_RATIO: f32 = 0.7;

type DialogListener = dyn FnMut(&mut Dialog, i32) -> bool;
type LinkListener = dyn FnMut(usize);

#[must_use]
pub struct Dialog {
    title: String,
    message: String,
    buttons: Vec<String>,
    /// listener function returns `false` to close the dialog, `true` to keep it open
    /// the parameter is the *index* of the button clicked, `-1` for outside click, `-2` for text
    listener: Option<Box<DialogListener>>,

    /// Clickable link rows drawn below the message body. Each entry is
    /// `(label, url)`; `on_link` is invoked with the row index when tapped.
    links: Vec<(String, String)>,
    on_link: Option<Box<LinkListener>>,
    link_buttons: Vec<RectButton>,

    text_btn: RectButton,

    h: Option<f32>,

    scroll: Scroll,
    window_rect: Option<Rect>,
    rect_buttons: Vec<DRectButton>,
}

impl Default for Dialog {
    fn default() -> Self {
        Self {
            title: tl!("notice").to_string(),
            message: String::new(),
            buttons: vec![tl!("ok").to_string()],
            listener: None,

            links: Vec::new(),
            on_link: None,
            link_buttons: Vec::new(),

            text_btn: RectButton::new(),

            h: None,

            scroll: Scroll::new(),
            window_rect: None,
            rect_buttons: vec![DRectButton::new()],
        }
    }
}

impl Dialog {
    pub fn simple(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            ..Default::default()
        }
    }

    pub fn plain(title: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            ..Default::default()
        }
    }

    pub fn error(error: Error) -> Self {
        let error = format!("{error:?}");
        Self {
            title: tl!("error").to_string(),
            message: error.clone(),
            buttons: vec![tl!("error-copy").to_string(), tl!("ok").to_string()],
            listener: Some(Box::new(move |_dialog, pos| {
                if pos == 0 {
                    unsafe { get_internal_gl() }.quad_context.clipboard_set(&error);
                    show_message(tl!("error-copied")).ok();
                }
                false
            })),

            rect_buttons: vec![DRectButton::new(); 2],
            ..Default::default()
        }
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    pub fn message(mut self, message: impl Into<String>) -> Self {
        self.set_message(message);
        self
    }

    pub fn set_message(&mut self, message: impl Into<String>) {
        self.message = message.into();
    }

    pub fn buttons(mut self, buttons: Vec<String>) -> Self {
        self.set_buttons(buttons);
        self
    }

    pub fn set_buttons(&mut self, buttons: Vec<String>) {
        self.buttons = buttons;
        self.rect_buttons = vec![DRectButton::new(); self.buttons.len()];
    }

    pub fn listener(mut self, f: impl FnMut(&mut Dialog, i32) -> bool + 'static) -> Self {
        self.listener = Some(Box::new(f));
        self
    }

    /// Adds clickable link rows drawn below the message body. Each entry is
    /// `(label, url)`; tapping a row invokes `on_link` with its index.
    pub fn links(mut self, links: Vec<(String, String)>) -> Self {
        self.link_buttons = (0..links.len()).map(|_| RectButton::new()).collect();
        self.links = links;
        self
    }

    /// Sets the callback fired when a link row is tapped, receiving its index.
    pub fn on_link(mut self, f: impl FnMut(usize) + 'static) -> Self {
        self.on_link = Some(Box::new(f));
        self
    }

    pub fn show(self) {
        crate::scene::DIALOG.with(|it| *it.borrow_mut() = Some(self));
    }

    pub fn touch(&mut self, touch: &Touch, t: f32) -> bool {
        self.scroll.touch(touch, t);
        let mut exit = false;
        for (index, btn) in self.rect_buttons.iter_mut().enumerate() {
            if btn.touch(touch, t) {
                if let Some(mut listener) = self.listener.take() {
                    if !listener(self, index as i32) {
                        exit = true;
                    }
                    self.listener = Some(listener);
                    break;
                } else {
                    exit = true;
                    break;
                }
            }
        }
        // Link rows sit inside the message body, below the message text. Test
        // them before the whole-body `text_btn` so a link tap never falls
        // through to the `-2` text click or the `-1` outside-click close.
        for (index, btn) in self.link_buttons.iter_mut().enumerate() {
            if btn.touch(touch) {
                if let Some(cb) = self.on_link.as_mut() {
                    cb(index);
                }
                return true; // consume the touch, keep the dialog open
            }
        }
        if self.text_btn.touch(touch) {
            if let Some(mut listener) = self.listener.take() {
                listener(self, -2);
                self.listener = Some(listener);
            }
        }
        if exit {
            return false;
        }

        if self
            .window_rect
            .is_none_or(|rect| rect.contains(touch.position) || touch.phase != TouchPhase::Started)
        {
            true
        } else {
            if let Some(mut listener) = self.listener.take() {
                let result = listener(self, -1);
                self.listener = Some(listener);
                if result {
                    return true;
                }
            }
            false
        }
    }

    pub fn update(&mut self, t: f32) {
        self.scroll.update(t);
    }

    pub fn render(&mut self, ui: &mut Ui, t: f32) {
        ui.fill_rect(ui.screen_rect(), Color::new(0., 0., 0., 0.6));

        let mh = ui.top * 2. * HEIGHT_RATIO;
        let s = 0.02;
        let pad = 0.02;
        let bh = 0.09;

        if self.h.is_none() {
            let link_h = self.links.len() as f32 * 0.08;
            self.h = Some(
                (ui.text(&self.message)
                    .size(0.5)
                    .max_width(2. * WIDTH_RADIO - pad * 3.)
                    .multiline()
                    .measure()
                    .h
                    + ui.text(&self.title).size(0.95).no_baseline().measure().h
                    + bh
                    + 0.22
                    + link_h)
                    .min(mh),
            );
        }
        let mut wr = Rect::new(0., 0., 2. * WIDTH_RADIO, self.h.unwrap());
        wr.x = -wr.w / 2.;
        wr.y = -wr.h / 2.;
        self.window_rect = Some(ui.rect_to_global(wr));
        ui.fill_path(&wr.rounded(0.01), ui.background());

        ui.scope(|ui| {
            let s = 0.01;
            let pad = 0.02;
            let mut h = 0.;
            macro_rules! dy {
                ($val:expr) => {{
                    let dy = $val;
                    h += dy;
                    ui.dy(dy);
                }};
            }
            dy!(wr.y + s * 3.);
            let r = ui
                .text(&self.title)
                .pos(wr.x + pad * 2., 0.)
                .anchor(0., 0.)
                .size(0.95)
                .max_width(wr.w - pad * 2.)
                .no_baseline()
                .draw_using(&BOLD_FONT);
            dy!(r.h + s * 2.);
            self.scroll.size((wr.w - pad * 2., wr.bottom() - h - bh - s * 2.));
            ui.dx(wr.x + pad);
            self.scroll.render(ui, |ui| {
                let r = ui
                    .text(&self.message)
                    .pos(pad, 0.)
                    .size(0.5)
                    .max_width(wr.w - pad * 3.)
                    .multiline()
                    .draw();
                self.text_btn.set(ui, r);
                ui.dy(r.h + 0.04);

                let accent = ui.accent();
                let mut link_h = 0.;
                for ((label, _url), btn) in self.links.iter().zip(self.link_buttons.iter_mut()) {
                    let lr = ui
                        .text(label)
                        .pos(pad, 0.)
                        .anchor(0., 0.)
                        .size(0.45)
                        .max_width(wr.w - pad * 3.)
                        .color(accent)
                        .draw();
                    // underline
                    ui.fill_rect(Rect::new(lr.x, lr.bottom() + 0.005, lr.w, 0.004), accent);
                    // generous tap target
                    btn.set(ui, lr.feather(0.012));
                    let dh = lr.h + 0.03;
                    link_h += dh;
                    ui.dy(dh);
                }

                (r.w, r.h + 0.04 + link_h)
            });
        });
        ui.scope(|ui| {
            let bw = (wr.w - pad * (self.buttons.len() + 1) as f32) / self.buttons.len() as f32;
            let mut r = Rect::new(wr.x + pad, wr.bottom() - s - bh, bw, bh);
            for (text, btn) in self.buttons.iter().zip(self.rect_buttons.iter_mut()) {
                btn.render_text(ui, r, t, text, 0.5, true);
                r.x += bw + pad;
            }
        });
    }
}
