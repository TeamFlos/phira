use crate::{
    ext::{RectExt, SafeTexture, ScaleType},
    ui::Ui,
};
use macroquad::prelude::*;
use std::{
    mem::ManuallyDrop,
    rc::{Rc, Weak},
};

pub const OUT_TIME: f32 = 0.8;
pub const PADDING: f32 = 0.02;

#[derive(Default, Clone)]
#[repr(u8)]
pub enum MessageKind {
    #[default]
    Info,
    Warn,
    Ok,
    Error,
}

impl MessageKind {
    pub fn color(&self) -> Color {
        match self {
            Self::Info => Color::new(0.16, 0.71, 0.96, 1.),
            Self::Warn => Color::new(1., 0.66, 0.15, 1.),
            Self::Ok => Color::new(0.4, 0.73, 0.42, 1.),
            Self::Error => Color::new(0.96, 0.26, 0.21, 1.),
        }
    }
}

pub struct Message {
    content: String,
    time: f32,
    end_time: f32,
    position: f32,
    target_position: f32,
    last_time: f32,
    width: f32,
    kind: MessageKind,
    handle: Weak<()>,
}

impl Message {
    pub fn new(content: String, time: f32, duration: f32, kind: MessageKind) -> (Self, MessageHandle) {
        let rc = Rc::new(());
        let handle = Rc::downgrade(&rc);
        (
            Self {
                content,
                time,
                end_time: time + duration,
                position: 0.,
                target_position: 0.,
                last_time: time,
                width: 0.,
                kind,
                handle,
            },
            MessageHandle(Some(ManuallyDrop::new(rc))),
        )
    }
}

pub struct MessageHandle(Option<ManuallyDrop<Rc<()>>>);
impl MessageHandle {
    pub fn cancel(&mut self) {
        if let Some(rc) = self.0.take() {
            ManuallyDrop::into_inner(rc);
        }
    }
}

pub struct BillBoard {
    messages: Vec<Message>,
    icons: Option<[SafeTexture; 4]>,
}

impl Default for BillBoard {
    fn default() -> Self {
        Self::new()
    }
}

impl BillBoard {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            icons: None,
        }
    }

    pub fn set_icons(&mut self, icons: [SafeTexture; 4]) {
        self.icons = Some(icons);
    }

    pub fn add(&mut self, mut msg: Message) {
        msg.position = self.messages.len() as f32;
        msg.target_position = msg.position;
        self.messages.push(msg);
    }

    pub fn render(&mut self, ui: &mut Ui, t: f32) {
        let rt = 1. - PADDING;
        let tp = -ui.top + PADDING;
        let h = 0.1;
        let pd = 0.014;
        let rh = h + 0.02;
        let mut pos = 0;
        self.messages.retain_mut(|msg| {
            if msg.end_time > t && msg.handle.strong_count() == 0 {
                msg.end_time = t;
            }
            let rt = if t >= msg.end_time {
                let p = (t - msg.end_time) / OUT_TIME;
                if p > 1. {
                    return false;
                }
                let p = 1. - (1. - p).powi(3);
                rt + msg.width * p
            } else {
                msg.target_position = pos as f32;
                pos += 1;
                if msg.width == 0. {
                    3.
                } else {
                    let p = ((t - msg.time) / OUT_TIME).min(1.);
                    let p = (1. - p).powi(3);
                    rt + msg.width * p
                }
            };
            let p = (0.5_f32).powf((t - msg.last_time) / 0.1);
            msg.position = msg.position * p + msg.target_position * (1. - p);
            msg.last_time = t;
            let tp = tp + msg.position * rh;
            let mut tx = ui
                .text(&msg.content)
                .pos(rt - pd, tp + h / 2.)
                .anchor(1., 0.5)
                .no_baseline()
                .size(0.64)
                .max_width(0.8);
            let r = tx.measure();
            let mut r = Rect::new(r.x - pd - h, tp, r.w + pd * 2. + h, h);
            msg.width = r.w + 0.2;
            tx.ui.fill_rect(r, msg.kind.color());
            if t < msg.end_time {
                tx.ui.fill_rect(
                    Rect::new(r.x, r.bottom() - 0.01, r.w * (1. - (t - msg.time) / (msg.end_time - msg.time)), 0.01),
                    Color::new(1., 1., 1., 0.3),
                );
            }
            r.w = h;
            tx.ui.fill_rect(r, Color::new(1., 1., 1., 0.4));
            if let Some(icons) = self.icons.as_ref() {
                let r = r.feather(-0.02);
                tx.ui.fill_rect(r, (*icons[msg.kind.clone() as u8 as usize], r, ScaleType::Fit));
            }
            tx.draw();
            true
        });
    }
}
