use super::{chart::ChartSettings, BpmList, CtrlObject, JudgeLine, Matrix, Object, Point, Resource};
pub use crate::{
    judge::{HitSound, JudgeStatus},
    parse::RPE_HEIGHT,
};
use macroquad::prelude::*;

const HOLD_PARTICLE_INTERVAL: f32 = 0.15;
const FADEOUT_TIME: f32 = 0.16;
const BAD_TIME: f32 = 0.5;

#[derive(Clone, Debug)]
pub enum NoteKind {
    Click,
    Hold { end_time: f32, end_height: f32 },
    Flick,
    Drag,
}

impl NoteKind {
    pub fn order(&self) -> i8 {
        match self {
            Self::Hold { .. } => 0,
            Self::Drag => 1,
            Self::Click => 2,
            Self::Flick => 3,
        }
    }
}

pub struct Note {
    pub object: Object,
    pub kind: NoteKind,
    pub hitsound: HitSound,
    pub time: f32,
    pub height: f32,
    pub speed: f32,

    /// From the other side of the line
    pub above: bool,
    pub multiple_hint: bool,
    pub fake: bool,
    pub judge: JudgeStatus,
}

pub struct RenderConfig<'a> {
    pub settings: &'a ChartSettings,
    pub ctrl_obj: &'a mut CtrlObject,
    pub line_height: f32,
    pub appear_before: f32,
    pub invisible_time: f32,
    pub draw_below: bool,
    pub incline_sin: f32,
}

fn draw_tex(res: &Resource, texture: Texture2D, order: i8, x: f32, y: f32, color: Color, mut params: DrawTextureParams, clip: bool) {
    let Vec2 { x: w, y: h } = params.dest_size.unwrap();
    if h < 0. {
        return;
    }
    let mut p = [Point::new(x, y), Point::new(x + w, y), Point::new(x + w, y + h), Point::new(x, y + h)];
    if clip {
        if y + h <= 0. {
            return;
        }
        if y <= 0. {
            let r = -y / (y + h);
            p[0].y = 0.;
            p[1].y = 0.;
            let mut source = params.source.unwrap_or_else(|| Rect::new(0., 0., 1., 1.));
            source.y += source.h * r;
            params.source = Some(source);
        }
    }
    params.flip_y = true;
    draw_tex_pts(res, texture, order, p, color, params);
}
fn draw_tex_pts(res: &Resource, texture: Texture2D, order: i8, p: [Point; 4], color: Color, params: DrawTextureParams) {
    let mut p = p.map(|it| res.world_to_screen(it));
    if p[0].x.min(p[1].x.min(p[2].x.min(p[3].x))) > 1.
        || p[0].x.max(p[1].x.max(p[2].x.max(p[3].x))) < -1.
        || p[0].y.min(p[1].y.min(p[2].y.min(p[3].y))) > 1.
        || p[0].y.max(p[1].y.max(p[2].y.max(p[3].y))) < -1.
    {
        return;
    }
    let Rect { x: sx, y: sy, w: sw, h: sh } = params.source.unwrap_or(Rect { x: 0., y: 0., w: 1., h: 1. });

    if params.flip_x {
        p.swap(0, 1);
        p.swap(2, 3);
    }
    if params.flip_y {
        p.swap(0, 3);
        p.swap(1, 2);
    }

    #[rustfmt::skip]
    let vertices = [
        Vertex::new(p[0].x, p[0].y, 0., sx     , sy     , color),
        Vertex::new(p[1].x, p[1].y, 0., sx + sw, sy     , color),
        Vertex::new(p[2].x, p[2].y, 0., sx + sw, sy + sh, color),
        Vertex::new(p[3].x, p[3].y, 0., sx     , sy + sh, color),
    ];
    res.note_buffer
        .borrow_mut()
        .push((order, texture.raw_miniquad_texture_handle().gl_internal_id()), vertices);
}

fn draw_center(res: &Resource, tex: Texture2D, order: i8, scale: f32, color: Color) {
    let hf = vec2(scale, tex.height() * scale / tex.width());
    draw_tex(
        res,
        tex,
        order,
        -hf.x,
        -hf.y,
        color,
        DrawTextureParams {
            dest_size: Some(hf * 2.),
            ..Default::default()
        },
        false,
    );
}

impl Note {
    pub fn rotation(&self, line: &JudgeLine) -> f32 {
        line.object.rotation.now() + if self.above { 0. } else { 180. }
    }

    pub fn plain(&self) -> bool {
        !self.fake && !matches!(self.kind, NoteKind::Hold { .. }) && self.object.translation.1.keyframes.len() <= 1
        // && self.ctrl_obj.is_default()
    }

    pub fn update(&mut self, res: &mut Resource, parent_rot: f32, parent_tr: &Matrix, ctrl_obj: &mut CtrlObject, line_height: f32) {
        self.object.set_time(res.time);
        if let Some(color) = if let JudgeStatus::Hold(perfect, at, ..) = &mut self.judge {
            if res.time > *at {
                *at += HOLD_PARTICLE_INTERVAL / res.config.speed;
                Some(if *perfect {
                    res.res_pack.info.fx_perfect()
                } else {
                    res.res_pack.info.fx_good()
                })
            } else {
                None
            }
        } else {
            None
        } {
            self.init_ctrl_obj(ctrl_obj, line_height);
            res.with_model(parent_tr * self.now_transform(res, ctrl_obj, 0., 0.), |res| {
                res.emit_at_origin(parent_rot + if self.above { 0. } else { 180. }, color)
            });
        }
    }

    pub fn dead(&self) -> bool {
        (!matches!(self.kind, NoteKind::Hold { .. }) || matches!(self.judge, JudgeStatus::Judged)) && self.object.dead()
        // && self.ctrl_obj.dead()
    }

    fn init_ctrl_obj(&self, ctrl_obj: &mut CtrlObject, line_height: f32) {
        ctrl_obj.set_height((self.height - line_height + self.object.translation.1.now() / self.speed) * RPE_HEIGHT / 2.);
    }

    pub fn now_transform(&self, res: &Resource, ctrl_obj: &CtrlObject, base: f32, incline_sin: f32) -> Matrix {
        let incline_val = 1. - incline_sin * (base * res.aspect_ratio + self.object.translation.1.now()) * RPE_HEIGHT / 2. / 360.;
        let mut tr = self.object.now_translation(res);
        tr.x *= incline_val * ctrl_obj.pos.now_opt().unwrap_or(1.);
        tr.y += base;
        let mut scale = self.object.scale.now_with_def(1.0, 1.0);
        scale.x *= ctrl_obj.size.now_opt().unwrap_or(1.0);
        if res.info.note_uniform_scale {
            scale.y *= ctrl_obj.size.now_opt().unwrap_or(1.0);
        } else {
            scale.y = 1.0;
        };
        self.object.now_rotation().append_nonuniform_scaling(&scale).append_translation(&tr)
    }

    pub fn render(&self, res: &mut Resource, config: &mut RenderConfig, bpm_list: &mut BpmList) {
        if matches!(self.judge, JudgeStatus::Judged) && !matches!(self.kind, NoteKind::Hold { .. }) {
            return;
        }
        if config.appear_before.is_finite() {
            // TODO optimize
            let beat = bpm_list.beat(self.time);
            let time = bpm_list.time_beats(beat - config.appear_before);
            if time > res.time {
                return;
            }
        }
        if config.invisible_time.is_finite() && self.time - config.invisible_time < res.time {
            return;
        }
        let scale = (if res.config.double_hint && self.multiple_hint {
            res.res_pack.note_style_mh.click.width() / res.res_pack.note_style.click.width()
        } else {
            1.0
        }) * res.note_width;
        let ctrl_obj = &mut config.ctrl_obj;
        self.init_ctrl_obj(ctrl_obj, config.line_height);
        let mut color = self.object.now_color();
        color.a *= res.alpha * ctrl_obj.alpha.now_opt().unwrap_or(1.);
        let spd = self.speed * ctrl_obj.y.now_opt().unwrap_or(1.);

        let line_height = config.line_height / res.aspect_ratio * spd;
        let height = self.height / res.aspect_ratio * spd;

        let base = height - line_height;
        let cover_base = if !config.settings.hold_partial_cover {
            height - line_height
        } else {
            match self.kind {
                NoteKind::Hold { end_time: _, end_height } => {
                    let end_height = end_height / res.aspect_ratio * spd;
                    end_height - line_height
                }
                _ => height - line_height,
            }
        };

        if !config.draw_below
            && ((res.time - FADEOUT_TIME >= self.time && !matches!(self.kind, NoteKind::Hold { .. }))
                || (self.time > res.time && cover_base <= -0.001))
        {
            return;
        }
        let order = self.kind.order();
        let style = if res.config.double_hint && self.multiple_hint {
            &res.res_pack.note_style_mh
        } else {
            &res.res_pack.note_style
        };
        let draw = |res: &mut Resource, tex: Texture2D| {
            let mut color = color;
            if !config.draw_below {
                color.a *= (self.time - res.time).min(0.) / FADEOUT_TIME + 1.;
            }
            res.with_model(self.now_transform(res, ctrl_obj, base, config.incline_sin), |res| {
                draw_center(res, tex, order, scale, color);
            });
        };
        match self.kind {
            NoteKind::Click => {
                draw(res, *style.click);
            }
            NoteKind::Hold { end_time, end_height } => {
                res.with_model(self.now_transform(res, ctrl_obj, 0., 0.), |res| {
                    let style = if res.config.double_hint && self.multiple_hint {
                        &res.res_pack.note_style_mh
                    } else {
                        &res.res_pack.note_style
                    };
                    if matches!(self.judge, JudgeStatus::Judged) {
                        // miss
                        color.a *= 0.5;
                    }
                    if res.time >= end_time {
                        return;
                    }
                    let end_height = end_height / res.aspect_ratio * spd;

                    let h = if self.time <= res.time { line_height } else { height };
                    let bottom = h - line_height;
                    let top = end_height - line_height;
                    let tex = &style.hold;
                    let ratio = style.hold_ratio();
                    // body
                    // TODO (end_height - height) is not always total height
                    draw_tex(
                        res,
                        **(if res.res_pack.info.hold_repeat {
                            style.hold_body.as_ref().unwrap()
                        } else {
                            tex
                        }),
                        order,
                        -scale,
                        bottom,
                        color,
                        DrawTextureParams {
                            source: Some({
                                if res.res_pack.info.hold_repeat {
                                    let hold_body = style.hold_body.as_ref().unwrap();
                                    let width = hold_body.width();
                                    let height = hold_body.height();
                                    Rect::new(0., 0., 1., (top - bottom) / scale / 2. * width / height)
                                } else {
                                    style.hold_body_rect()
                                }
                            }),
                            dest_size: Some(vec2(scale * 2., top - bottom)),
                            ..Default::default()
                        },
                        false,
                    );
                    // head
                    if res.time < self.time || res.res_pack.info.hold_keep_head {
                        let r = style.hold_head_rect();
                        let hf = vec2(scale, r.h / r.w * scale * ratio);
                        draw_tex(
                            res,
                            **tex,
                            order,
                            -scale,
                            bottom - if res.res_pack.info.hold_compact { hf.y } else { hf.y * 2. },
                            color,
                            DrawTextureParams {
                                source: Some(r),
                                dest_size: Some(hf * 2.),
                                ..Default::default()
                            },
                            false,
                        );
                    }
                    // tail
                    let r = style.hold_tail_rect();
                    let hf = vec2(scale, r.h / r.w * scale * ratio);
                    draw_tex(
                        res,
                        **tex,
                        order,
                        -scale,
                        top - if res.res_pack.info.hold_compact { hf.y } else { 0. },
                        color,
                        DrawTextureParams {
                            source: Some(r),
                            dest_size: Some(hf * 2.),
                            ..Default::default()
                        },
                        false,
                    );
                });
            }
            NoteKind::Flick => {
                draw(res, *style.flick);
            }
            NoteKind::Drag => {
                draw(res, *style.drag);
            }
        }
    }
}

pub struct BadNote {
    pub time: f32,
    pub kind: NoteKind,
    pub matrix: Matrix,
}

impl BadNote {
    pub fn render(&self, res: &mut Resource) -> bool {
        if res.time > self.time + BAD_TIME {
            return false;
        }
        res.with_model(self.matrix, |res| {
            let style = &res.res_pack.note_style;
            draw_center(
                res,
                match &self.kind {
                    NoteKind::Click => *style.click,
                    NoteKind::Drag => *style.drag,
                    NoteKind::Flick => *style.flick,
                    _ => unreachable!(),
                },
                self.kind.order(),
                res.note_width,
                Color::new(0.423529, 0.262745, 0.262745, (self.time - res.time).max(-1.) / BAD_TIME + 1.),
            );
        });
        true
    }
}
