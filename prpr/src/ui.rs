//! UI utilities.
prpr_l10n::tl_file!("scene" ttl);
mod billboard;
pub use billboard::{BillBoard, Message, MessageHandle, MessageKind};

mod chart_info;
pub use chart_info::*;

mod dialog;
pub use dialog::Dialog;

mod scroll;
pub use scroll::*;

mod shading;
pub use shading::*;

mod shadow;
pub use shadow::*;

mod text;
pub use text::{DrawText, TextPainter};

pub use glyph_brush::ab_glyph::FontArc;

use crate::{
    core::{Matrix, Point, Vector},
    ext::{get_viewport, nalgebra_to_glm, semi_black, semi_white, source_of_image, RectExt, SafeTexture, ScaleType},
    judge::Judge,
    scene::{request_input_full, return_input, show_error, take_input},
};
use lyon::{
    lyon_tessellation::{
        BuffersBuilder, FillOptions, FillTessellator, FillVertex, FillVertexConstructor, StrokeOptions, StrokeTessellator, StrokeVertex,
        StrokeVertexConstructor, VertexBuffers,
    },
    math as lm,
    path::{LineCap, Path, PathEvent},
};
use macroquad::prelude::*;
use miniquad::PassAction;
use sasa::{AudioManager, PlaySfxParams, Sfx};
use std::{borrow::Cow, cell::RefCell, collections::HashMap, ops::Range};

#[derive(Default, Clone, Copy)]
pub struct Gravity(u8);

impl Gravity {
    pub const LEFT: u8 = 0;
    pub const HCENTER: u8 = 1;
    pub const RIGHT: u8 = 2;
    pub const TOP: u8 = 0;
    pub const VCENTER: u8 = 4;
    pub const BOTTOM: u8 = 8;

    pub const BEGIN: u8 = Self::LEFT | Self::TOP;
    pub const CENTER: u8 = Self::HCENTER | Self::VCENTER;
    pub const END: u8 = Self::RIGHT | Self::BOTTOM;

    fn value(mode: u8) -> f32 {
        match mode {
            0 => 0.,
            1 => 0.5,
            2 => 1.,
            _ => unreachable!(),
        }
    }

    pub fn offset(&self, total: (f32, f32), content: (f32, f32)) -> (f32, f32) {
        (Self::value(self.0 & 3) * (total.0 - content.0), Self::value((self.0 >> 2) & 3) * (total.1 - content.1))
    }

    pub fn from_point(&self, point: (f32, f32), content: (f32, f32)) -> (f32, f32) {
        (point.0 - content.0 * Self::value(self.0 & 3), point.1 - content.1 * Self::value((self.0 >> 2) & 3))
    }
}

impl From<u8> for Gravity {
    fn from(val: u8) -> Self {
        Self(val)
    }
}

struct ShadedConstructor<T: Shading>(Matrix, pub T, f32);
impl<T: Shading> FillVertexConstructor<Vertex> for ShadedConstructor<T> {
    fn new_vertex(&mut self, vertex: FillVertex) -> Vertex {
        let pos = vertex.position();
        self.1.new_vertex(&self.0, &Point::new(pos.x, pos.y), self.2)
    }
}
impl<T: Shading> StrokeVertexConstructor<Vertex> for ShadedConstructor<T> {
    fn new_vertex(&mut self, vertex: StrokeVertex) -> Vertex {
        let pos = vertex.position();
        self.1.new_vertex(&self.0, &Point::new(pos.x, pos.y), self.2)
    }
}

pub struct VertexBuilder<T: Shading> {
    matrix: Matrix,
    vertices: Vec<Vertex>,
    indices: Vec<u16>,
    shading: T,
    alpha: f32,
}

impl<T: Shading> VertexBuilder<T> {
    fn new(matrix: Matrix, shading: T, alpha: f32) -> Self {
        Self {
            matrix,
            vertices: Vec::new(),
            indices: Vec::new(),
            shading,
            alpha,
        }
    }

    pub fn add(&mut self, x: f32, y: f32) {
        self.vertices.push(self.shading.new_vertex(&self.matrix, &Point::new(x, y), self.alpha));
    }

    pub fn triangle(&mut self, x: u16, y: u16, z: u16) {
        self.indices.push(x);
        self.indices.push(y);
        self.indices.push(z);
    }

    pub fn commit(&self) {
        let gl = unsafe { get_internal_gl() }.quad_gl;
        gl.texture(self.shading.texture());
        gl.draw_mode(DrawMode::Triangles);
        gl.geometry(&self.vertices, &self.indices);
    }
}

#[derive(Clone, Copy)]
pub struct RectButton {
    pts: Option<[Vec2; 4]>,
    id: Option<u64>,
}

impl Default for RectButton {
    fn default() -> Self {
        Self::new()
    }
}

impl RectButton {
    pub fn new() -> Self {
        Self { pts: None, id: None }
    }

    pub fn touching(&self) -> bool {
        self.id.is_some()
    }

    pub fn contains(&self, pos: Vec2) -> bool {
        if let Some([a, b, c, d]) = self.pts {
            let abp = (b - a).perp_dot(pos - a);
            let bcp = (c - b).perp_dot(pos - b);
            let cdp = (d - c).perp_dot(pos - c);
            let dap = (a - d).perp_dot(pos - d);
            (abp >= 0. && bcp >= 0. && cdp >= 0. && dap >= 0.) || (abp <= 0. && bcp <= 0. && cdp <= 0. && dap <= 0.)
        } else {
            false
        }
    }

    pub fn set(&mut self, ui: &mut Ui, rect: Rect) {
        let mat = nalgebra_to_glm(&ui.transform) * ui.gl_transform;
        let tr = |x: f32, y: f32| {
            let pos = mat * vec4(x, y, 0., 1.);
            pos.xy() / pos.w
        };
        self.pts = Some([
            tr(rect.x, rect.y),
            tr(rect.right(), rect.y),
            tr(rect.right(), rect.bottom()),
            tr(rect.x, rect.bottom()),
        ]);
    }

    pub fn touch(&mut self, touch: &Touch) -> bool {
        let inside = self.contains(touch.position);
        match touch.phase {
            TouchPhase::Started => {
                if inside {
                    self.id = Some(touch.id);
                }
            }
            TouchPhase::Moved | TouchPhase::Stationary => {
                if self.id == Some(touch.id) && !inside {
                    self.id = None;
                }
            }
            TouchPhase::Cancelled => {
                self.id = None;
            }
            TouchPhase::Ended => {
                if self.id.take() == Some(touch.id) && inside {
                    return true;
                }
            }
        }
        false
    }
}

#[derive(Clone)]
pub struct DRectButton {
    pub inner: RectButton,
    last_touching: bool,
    start_time: Option<f32>,
    pub config: ShadowConfig,
    delta: f32,
    play_sound: bool,
}
impl Default for DRectButton {
    fn default() -> Self {
        Self::new()
    }
}
impl DRectButton {
    pub const TIME: f32 = 0.2;

    pub fn new() -> Self {
        Self {
            inner: RectButton::new(),
            last_touching: false,
            start_time: None,
            config: ShadowConfig::default(),
            delta: -0.006,
            play_sound: true,
        }
    }

    pub fn build(&mut self, ui: &mut Ui, t: f32, r: Rect, f: impl FnOnce(&mut Ui, Path)) {
        self.inner.set(ui, r);
        // let r = r.feather((1. - self.progress(t)) * self.delta);
        let ct = r.center();
        let ct = Vector::new(ct.x, ct.y);
        ui.with(
            Matrix::new_translation(&-ct)
                .append_scaling(1. - (1. - self.progress(t)) * 0.04)
                .append_translation(&ct),
            |ui| {
                f(ui, r.rounded(self.config.radius));
            },
        );
    }

    pub fn invalidate(&mut self) {
        self.inner.pts = None;
    }

    pub fn render_shadow(&mut self, ui: &mut Ui, r: Rect, t: f32, f: impl FnOnce(&mut Ui, Path)) {
        let p = self.progress(t);
        let config = ShadowConfig {
            elevation: self.config.elevation * p,
            radius: self.config.radius,
            ..self.config
        };
        ui.scope(|ui| {
            ui.dy((1. - p) * 0.004);
            self.build(ui, t, r, |ui, path| {
                rounded_rect_shadow(ui, r, &config);
                f(ui, path);
            });
        });
    }

    pub fn render_text<'a>(&mut self, ui: &mut Ui, r: Rect, t: f32, text: impl Into<Cow<'a, str>>, size: f32, chosen: bool) {
        let oh = r.h;
        self.build(ui, t, r, |ui, path| {
            let ct = r.center();
            ui.fill_path(&path, if chosen { WHITE } else { semi_black(0.4) });
            ui.text(text)
                .pos(ct.x, ct.y)
                .anchor(0.5, 0.5)
                .no_baseline()
                .size(size * (1. - (1. - r.h / oh).powf(1.3)))
                .max_width(r.w)
                .color(if chosen { Color::new(0.3, 0.3, 0.3, 1.) } else { WHITE })
                .draw();
        });
    }

    pub fn render_text_left<'a>(&mut self, ui: &mut Ui, r: Rect, t: f32, alpha: f32, text: impl Into<Cow<'a, str>>, size: f32, chosen: bool) {
        let oh = r.h;
        self.build(ui, t, r, |ui, path| {
            ui.fill_path(&path, if chosen { WHITE } else { semi_black(0.4) });
            ui.text(text)
                .pos(r.x + 0.02, r.center().y)
                .anchor(0., 0.5)
                .max_width(r.w - 0.04)
                .no_baseline()
                .size(size * r.h / oh)
                .color(if chosen { Color::new(0.3, 0.3, 0.3, alpha) } else { semi_white(alpha) })
                .draw();
        });
    }

    #[inline]
    pub fn render_input<'a>(&mut self, ui: &mut Ui, r: Rect, t: f32, text: impl Into<Cow<'a, str>>, hint: impl Into<Cow<'a, str>>, size: f32) {
        let text = text.into();
        if text.trim().is_empty() {
            self.render_text_left(ui, r, t, 0.7, hint, size, false);
        } else {
            self.render_text_left(ui, r, t, 1., text, size, false);
        }
    }

    #[inline]
    pub fn no_sound(mut self) -> Self {
        self.play_sound = false;
        self
    }

    #[inline]
    pub fn with_radius(mut self, radius: f32) -> Self {
        self.config.radius = radius;
        self
    }

    #[inline]
    pub fn with_elevation(mut self, elevation: f32) -> Self {
        self.config.elevation = elevation;
        self
    }

    #[inline]
    pub fn with_base(mut self, base: f32) -> Self {
        self.config.base = base;
        self
    }

    #[inline]
    pub fn with_delta(mut self, delta: f32) -> Self {
        self.delta = delta;
        self
    }

    pub fn progress(&mut self, t: f32) -> f32 {
        if self.start_time.as_ref().is_some_and(|it| t > *it + Self::TIME) {
            self.start_time = None;
        }
        let p = if let Some(time) = &self.start_time {
            (t - time) / Self::TIME
        } else {
            1.
        };
        if self.inner.touching() {
            1. - p
        } else {
            p
        }
    }

    pub fn touch(&mut self, touch: &Touch, t: f32) -> bool {
        let res = self.inner.touch(touch);
        let touching = self.inner.touching();
        if self.last_touching != touching {
            self.last_touching = touching;
            self.start_time = Some(t);
        }
        if res && self.play_sound {
            button_hit();
        }
        res
    }
}

pub struct Slider {
    range: Range<f32>,
    step: f32,

    btn_dec: DRectButton,
    btn_inc: DRectButton,

    touch: Option<(u64, f32, bool)>,
    rect: Rect,
    pos: f32,
}

impl Slider {
    const RADIUS: f32 = 0.028;
    const THRESHOLD: f32 = 0.05;

    pub fn new(range: Range<f32>, step: f32) -> Self {
        Self {
            range,
            step,

            btn_dec: DRectButton::new().with_delta(-0.002),
            btn_inc: DRectButton::new().with_delta(-0.002),

            touch: None,
            rect: Rect::default(),
            pos: f32::INFINITY,
        }
    }

    pub fn touch(&mut self, touch: &Touch, t: f32, dst: &mut f32) -> Option<bool> {
        if self.btn_dec.touch(touch, t) {
            *dst = (*dst - self.step).max(self.range.start);
            return Some(true);
        }
        if self.btn_inc.touch(touch, t) {
            *dst = (*dst + self.step).min(self.range.end);
            return Some(true);
        }
        if let Some((id, start_pos, unlocked)) = &mut self.touch {
            if touch.id == *id {
                match touch.phase {
                    TouchPhase::Started | TouchPhase::Moved | TouchPhase::Stationary => {
                        if (touch.position.x - *start_pos).abs() >= Self::THRESHOLD {
                            *unlocked = true;
                        }
                        if *unlocked {
                            let p = (touch.position.x - self.rect.x) / self.rect.w;
                            let p = p.clamp(0., 1.);
                            let p = self.range.start + (self.range.end - self.range.start) * p;
                            *dst = (p / self.step).round() * self.step;
                            return Some(true);
                        }
                    }
                    TouchPhase::Cancelled | TouchPhase::Ended => {
                        self.touch = None;
                    }
                }
                return Some(false);
            }
        } else if touch.phase == TouchPhase::Started {
            let pos = (self.pos, self.rect.center().y);
            if (touch.position.x - pos.0).hypot(touch.position.y - pos.1) <= Self::RADIUS {
                self.touch = Some((touch.id, touch.position.x, false));
                return Some(false);
            }
        }
        None
    }

    pub fn render(&mut self, ui: &mut Ui, mut r: Rect, t: f32, p: f32, text: String) {
        r.x -= 0.1;
        r.x -= r.w * 0.2;
        r.w *= 1.2;
        let pad = 0.04;
        let size = 0.026;
        let cy = r.center().y;
        self.btn_dec
            .render_text(ui, Rect::new(r.x - pad - size, cy, 0., 0.).feather(size), t, "-", 0.7, true);
        self.btn_inc
            .render_text(ui, Rect::new(r.right() + pad + size, cy, 0., 0.).feather(size), t, "+", 0.7, true);
        self.rect = ui.rect_to_global(r);
        ui.text(text)
            .pos(r.x - (pad + size) * 2., cy)
            .anchor(1., 0.5)
            .no_baseline()
            .size(0.6)
            .draw();
        let p = (p - self.range.start) / (self.range.end - self.range.start);
        let pos = (r.x + r.w * p, cy);
        self.pos = ui.to_global(pos).0;
        use lyon::math::point;
        ui.stroke_options = ui.stroke_options.with_line_cap(LineCap::Round);
        ui.stroke_path(
            &{
                let mut p = Path::builder();
                p.begin(point(r.x, cy));
                p.line_to(point(pos.0, cy));
                p.end(false);
                p.build()
            },
            0.02,
            Color { a: 0.8, ..ui.background() },
        );
        ui.stroke_path(
            &{
                let mut p = Path::builder();
                p.begin(point(pos.0, cy));
                p.line_to(point(r.right(), cy));
                p.end(false);
                p.build()
            },
            0.02,
            semi_white(0.8),
        );
        ui.stroke_options = ui.stroke_options.with_line_cap(LineCap::Square);
        rounded_rect_shadow(
            ui,
            Rect::new(pos.0, pos.1, 0., 0.).feather(Self::RADIUS),
            &ShadowConfig {
                radius: Self::RADIUS,
                base: 0.7,
                ..Default::default()
            },
        );
        ui.fill_circle(pos.0, pos.1, Self::RADIUS, WHITE);
    }
}

thread_local! {
    static STATE: RefCell<HashMap<String, Option<u64>>> = RefCell::new(HashMap::new());
}

pub struct InputParams<'a> {
    changed: Option<&'a mut bool>,
    password: bool,
    length: f32,
}

impl From<()> for InputParams<'_> {
    fn from(_: ()) -> Self {
        Self {
            changed: None,
            password: false,
            length: 0.3,
        }
    }
}

impl From<bool> for InputParams<'_> {
    fn from(password: bool) -> Self {
        Self { password, ..().into() }
    }
}

impl From<f32> for InputParams<'_> {
    fn from(length: f32) -> Self {
        Self { length, ..().into() }
    }
}

impl<'a> From<(f32, &'a mut bool)> for InputParams<'a> {
    fn from((length, changed): (f32, &'a mut bool)) -> Self {
        Self {
            changed: Some(changed),
            password: false,
            length,
        }
    }
}

pub struct Ui<'a> {
    pub top: f32,
    pub viewport: (i32, i32, i32, i32),

    pub text_painter: &'a mut TextPainter,

    pub transform: Matrix,
    pub gl_transform: Mat4,
    scissor: Option<(i32, i32, i32, i32)>,
    touches: Option<Vec<Touch>>,

    vertex_buffers: VertexBuffers<Vertex, u16>,
    fill_tess: FillTessellator,
    fill_options: FillOptions,
    stroke_tess: StrokeTessellator,
    pub stroke_options: StrokeOptions,

    pub alpha: f32,
}

impl<'a> Ui<'a> {
    pub fn new(text_painter: &'a mut TextPainter, viewport: Option<(i32, i32, i32, i32)>) -> Self {
        unsafe { get_internal_gl() }.quad_context.begin_default_pass(PassAction::Clear {
            depth: None,
            stencil: Some(0),
            color: None,
        });
        let viewport = viewport.unwrap_or_else(|| (0, 0, screen_width() as i32, screen_height() as i32));
        Self {
            top: viewport.3 as f32 / viewport.2 as f32,
            viewport,

            text_painter,

            transform: Matrix::identity(),
            gl_transform: Mat4::IDENTITY,
            scissor: None,
            touches: None,

            vertex_buffers: VertexBuffers::new(),
            fill_tess: FillTessellator::new(),
            fill_options: FillOptions::default(),
            stroke_tess: StrokeTessellator::new(),
            stroke_options: StrokeOptions::default(),

            alpha: 1.,
        }
    }

    pub fn camera(&self) -> Camera2D {
        Camera2D {
            zoom: vec2(1., -self.viewport.2 as f32 / self.viewport.3 as f32),
            viewport: Some(self.viewport),
            ..Default::default()
        }
    }

    pub fn ensure_touches(&mut self) -> &mut Vec<Touch> {
        if self.touches.is_none() {
            self.touches = Some(Judge::get_touches());
        }
        self.touches.as_mut().unwrap()
    }

    pub(crate) fn set_touches(&mut self, touches: Vec<Touch>) {
        self.touches = Some(touches);
    }

    pub fn builder<T: IntoShading>(&self, shading: T) -> VertexBuilder<T::Target> {
        VertexBuilder::new(self.transform, shading.into_shading(), self.alpha)
    }

    pub fn fill_rect(&mut self, rect: Rect, shading: impl IntoShading) {
        let mut b = self.builder(shading);
        b.add(rect.x, rect.y);
        b.add(rect.x + rect.w, rect.y);
        b.add(rect.x, rect.y + rect.h);
        b.add(rect.x + rect.w, rect.y + rect.h);
        b.triangle(0, 1, 2);
        b.triangle(1, 2, 3);
        b.commit();
    }

    fn set_tolerance(&mut self) {
        let tol = 0.15 / (self.transform.transform_vector(&Vector::new(1., 0.)).norm() * screen_width() / 2.);
        self.fill_options.tolerance = tol;
        self.stroke_options.tolerance = tol;
    }

    fn draw_lyon<T: Shading>(&mut self, shading: T, f: impl FnOnce(&mut Self, ShadedConstructor<T>)) {
        self.set_tolerance();
        let shaded = ShadedConstructor(self.transform, shading.into_shading(), self.alpha);
        let tex = shaded.1.texture();
        f(self, shaded);
        self.emit_lyon(tex);
    }

    pub fn fill_path(&mut self, path: impl IntoIterator<Item = PathEvent>, shading: impl IntoShading) {
        self.draw_lyon(shading.into_shading(), |this, shaded| {
            this.fill_tess
                .tessellate(path, &this.fill_options, &mut BuffersBuilder::new(&mut this.vertex_buffers, shaded))
                .unwrap();
        });
    }

    pub fn fill_circle(&mut self, x: f32, y: f32, radius: f32, shading: impl IntoShading) {
        self.draw_lyon(shading.into_shading(), |this, shaded| {
            this.fill_tess
                .tessellate_circle(lm::point(x, y), radius, &this.fill_options, &mut BuffersBuilder::new(&mut this.vertex_buffers, shaded))
                .unwrap();
        });
    }

    pub fn stroke_circle(&mut self, x: f32, y: f32, radius: f32, width: f32, shading: impl IntoShading) {
        self.draw_lyon(shading.into_shading(), |this, shaded| {
            this.stroke_options.line_width = width;
            this.stroke_tess
                .tessellate_circle(lm::point(x, y), radius, &this.stroke_options, &mut BuffersBuilder::new(&mut this.vertex_buffers, shaded))
                .unwrap();
        });
    }

    pub fn stroke_path(&mut self, path: &Path, width: f32, shading: impl IntoShading) {
        self.draw_lyon(shading.into_shading(), |this, shaded| {
            this.stroke_options.line_width = width;
            this.stroke_tess
                .tessellate_path(path, &this.stroke_options, &mut BuffersBuilder::new(&mut this.vertex_buffers, shaded))
                .unwrap();
        });
    }

    fn emit_lyon(&mut self, texture: Option<Texture2D>) {
        let gl = unsafe { get_internal_gl() }.quad_gl;
        gl.texture(texture);
        gl.draw_mode(DrawMode::Triangles);
        gl.geometry(&std::mem::take(&mut self.vertex_buffers.vertices), &std::mem::take(&mut self.vertex_buffers.indices));
    }

    pub fn screen_rect(&self) -> Rect {
        Rect::new(-1., -self.top, 2., self.top * 2.)
    }

    pub fn dialog_rect() -> Rect {
        let hw = 0.45;
        let hh = 0.34;
        Rect::new(-hw, -hh, hw * 2., hh * 2.)
    }

    pub fn rect_to_global(&self, rect: Rect) -> Rect {
        let pt = self.to_global((rect.x, rect.y));
        let vec = self.vec_to_global((rect.w, rect.h));
        Rect::new(pt.0, pt.1, vec.0, vec.1)
    }

    pub fn vec_to_global(&self, vec: (f32, f32)) -> (f32, f32) {
        let r = self.transform.transform_vector(&Vector::new(vec.0, vec.1));
        (r.x, r.y)
    }

    pub fn to_global(&self, pt: (f32, f32)) -> (f32, f32) {
        let r = self.transform.transform_point(&Point::new(pt.0, pt.1));
        (r.x, r.y)
    }

    pub fn to_local(&self, pt: (f32, f32)) -> (f32, f32) {
        let r = self.transform.try_inverse().unwrap().transform_point(&Point::new(pt.0, pt.1));
        (r.x, r.y)
    }

    pub fn dx(&mut self, x: f32) {
        self.transform.append_translation_mut(&Vector::new(x, 0.));
    }

    pub fn dy(&mut self, y: f32) {
        self.transform.append_translation_mut(&Vector::new(0., y));
    }

    #[inline]
    pub fn alpha<R>(&mut self, alpha: f32, f: impl FnOnce(&mut Self) -> R) -> R {
        let old = self.alpha;
        self.alpha = old * alpha;
        let res = f(self);
        self.alpha = old;
        res
    }

    #[inline]
    pub fn with<R>(&mut self, transform: Matrix, f: impl FnOnce(&mut Self) -> R) -> R {
        let old = self.transform;
        self.transform = old * transform;
        let res = f(self);
        self.transform = old;
        res
    }

    #[inline]
    pub fn scope<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let old = self.transform;
        let res = f(self);
        self.transform = old;
        res
    }

    #[inline]
    pub fn abs_scope<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let old = self.transform;
        self.transform = Matrix::identity();
        let res = f(self);
        self.transform = old;
        res
    }

    #[inline]
    pub fn with_gl<R>(&mut self, transform: Mat4, f: impl FnOnce(&mut Self) -> R) -> R {
        let old = self.gl_transform;
        // self.gl_transform = old * transform;
        let gl = unsafe { get_internal_gl() }.quad_gl;
        gl.push_model_matrix(transform);
        let res = f(self);
        self.gl_transform = old;
        unsafe { get_internal_gl() }.flush();
        gl.pop_model_matrix();
        res
    }

    #[inline]
    pub fn apply<R>(&mut self, f: impl FnOnce(&mut Ui) -> R) -> R {
        unsafe { get_internal_gl() }.quad_gl.push_model_matrix(nalgebra_to_glm(&self.transform));
        let res = f(self);
        unsafe { get_internal_gl() }.quad_gl.pop_model_matrix();
        res
    }

    pub fn scissor<R>(&mut self, rect: Rect, f: impl FnOnce(&mut Ui) -> R) -> R {
        let igl = unsafe { get_internal_gl() };
        let gl = igl.quad_gl;
        let rect = self.rect_to_global(rect);
        let vp = get_viewport();
        let pt = (
            vp.0 as f32 + (rect.x + 1.) / 2. * vp.2 as f32,
            (screen_height() - (vp.1 + vp.3) as f32) + (rect.y * vp.2 as f32 / vp.3 as f32 + 1.) / 2. * vp.3 as f32,
        );

        let old = self.scissor;
        self.scissor = {
            let mut l = pt.0 as i32;
            let mut t = pt.1 as i32;
            let mut r = (pt.0 + rect.w * vp.2 as f32 / 2.) as i32;
            let mut b = (pt.1 + rect.h * vp.2 as f32 / 2.) as i32;
            if let Some((l0, t0, w0, h0)) = old {
                l = l.max(l0);
                t = t.max(t0);
                r = r.min(l0 + w0);
                b = b.min(t0 + h0);
            }
            Some((l, t, r - l, b - t))
        };

        gl.scissor(self.scissor);
        let res = f(self);
        self.scissor = old;
        gl.scissor(old);
        res
    }

    pub fn text<'s, 'ui>(&'ui mut self, text: impl Into<Cow<'s, str>>) -> DrawText<'a, 's, 'ui> {
        DrawText::new(self, text.into())
    }

    fn clicked(&mut self, rect: Rect, entry: &mut Option<u64>) -> bool {
        let rect = self.rect_to_global(rect);
        let mut exists = false;
        let mut any = false;
        let old_entry = *entry;
        let mut res = false;
        self.ensure_touches().retain(|touch| {
            exists = exists || old_entry == Some(touch.id);
            if !rect.contains(touch.position) {
                return true;
            }
            any = true;
            match touch.phase {
                TouchPhase::Started => {
                    *entry = Some(touch.id);
                    false
                }
                TouchPhase::Moved | TouchPhase::Stationary => {
                    if *entry != Some(touch.id) {
                        *entry = None;
                        true
                    } else {
                        false
                    }
                }
                TouchPhase::Cancelled => {
                    *entry = None;
                    true
                }
                TouchPhase::Ended => {
                    if entry.take() == Some(touch.id) {
                        res = true;
                        false
                    } else {
                        true
                    }
                }
            }
        });
        if res {
            return true;
        }
        if !any && exists {
            *entry = None;
        }
        false
    }

    pub fn accent(&self) -> Color {
        Color::from_hex(0xff2196f3)
    }

    pub fn background(&self) -> Color {
        Color::from_hex(0xff2a323c)
    }

    pub fn button(&mut self, id: &str, rect: Rect, text: impl Into<String>) -> bool {
        let text = text.into();
        STATE.with(|state| {
            let mut state = state.borrow_mut();
            let entry = state.entry(id.to_owned()).or_default();
            self.fill_path(
                &rect.rounded(0.01),
                Color {
                    a: if entry.is_some() { 0.5 } else { 1. },
                    ..self.background()
                },
            );
            let ct = rect.center();
            self.text(text)
                .pos(ct.x, ct.y)
                .anchor(0.5, 0.5)
                .max_width(rect.w)
                .size(0.42)
                .color(WHITE)
                .no_baseline()
                .draw();
            self.clicked(rect, entry)
        })
    }

    pub fn checkbox(&mut self, text: impl Into<String>, value: &mut bool) -> Rect {
        let text = text.into();
        STATE.with(|state| {
            let mut state = state.borrow_mut();
            let entry = state.entry(format!("chkbox#{text}")).or_default();
            let w = 0.08;
            let s = 0.03;
            let text = self.text(text).pos(w, 0.).size(0.5).no_baseline().draw();
            let r = Rect::new(w / 2. - s, text.center().y - s, s * 2., s * 2.);
            self.fill_rect(r, if *value { self.accent() } else { WHITE });
            let r = Rect::new(r.x, r.y, text.right() - r.x, (text.bottom() - r.y).max(w));
            if self.clicked(r, entry) {
                *value ^= true;
            }
            r
        })
    }

    pub fn input<'b>(&mut self, label: impl Into<String>, value: &mut String, params: impl Into<InputParams<'b>>) -> Rect {
        let label = label.into();
        let params = params.into();
        let id = format!("input#{label}");
        let r = self.text(label).anchor(1., 0.).size(0.47).draw();
        let lf = r.x;
        let r = Rect::new(0.02, r.y - 0.01, params.length, r.h + 0.02);
        if if params.password {
            self.button(&id, r, "*".repeat(value.chars().count()))
        } else {
            self.button(&id, r, value.lines().next().unwrap_or_default())
        } {
            request_input_full(&id, value, params.password);
        }
        if let Some((its_id, text)) = take_input() {
            if its_id == id {
                if let Some(changed) = params.changed {
                    *changed = true;
                }
                *value = text;
            } else {
                return_input(its_id, text);
            }
        }
        Rect::new(lf, r.y, r.right() - lf, r.h)
    }

    pub fn slider(&mut self, text: impl Into<String>, range: Range<f32>, step: f32, value: &mut f32, length: Option<f32>) -> Rect {
        let text = text.into();
        STATE.with(|state| {
            let mut state = state.borrow_mut();
            let entry = state.entry(text.clone()).or_default();

            let len = length.unwrap_or(0.3);
            let s = 0.002;
            let tr = self.text(format!("{text}: {value:.3}")).size(0.4).draw();
            let cy = tr.h + 0.03;
            let r = Rect::new(0., cy - s, len, s * 2.);
            self.fill_rect(r, WHITE);
            let p = (*value - range.start) / (range.end - range.start);
            let p = p.clamp(0., 1.);
            self.fill_circle(len * p, cy, 0.015, self.accent());
            let r = r.feather(0.015 - s);
            let r = self.rect_to_global(r);
            self.ensure_touches();
            if let Some(id) = entry {
                if let Some(touch) = self.touches.as_ref().unwrap().iter().rfind(|it| it.id == *id) {
                    let Vec2 { x, y } = touch.position;
                    let (x, _) = self.to_local((x, y));
                    let p = (x / len).clamp(0., 1.);
                    *value = range.start + (range.end - range.start) * p;
                    *value = (*value / step).round() * step;
                    if matches!(touch.phase, TouchPhase::Cancelled | TouchPhase::Ended) {
                        *entry = None;
                    }
                }
            } else if let Some(touch) = self.touches.as_ref().unwrap().iter().find(|it| r.contains(it.position)) {
                if touch.phase == TouchPhase::Started {
                    *entry = Some(touch.id);
                }
            }

            let s = 0.025;
            let mut x = len + 0.02;
            let r = Rect::new(x, cy - s, s * 2., s * 2.);
            self.fill_path(&r.rounded(0.008), self.background());
            self.text("-")
                .pos(r.center().x, r.center().y)
                .anchor(0.5, 0.5)
                .size(0.4)
                .color(WHITE)
                .no_baseline()
                .draw();
            if self.clicked(r, state.entry(format!("{text}:-")).or_default()) {
                *value = (*value - step).max(range.start);
            }
            x += s * 2. + 0.01;
            let r = Rect::new(x, cy - s, s * 2., s * 2.);
            self.fill_path(&r.rounded(0.008), self.background());
            self.text("+")
                .pos(r.center().x, r.center().y)
                .anchor(0.5, 0.5)
                .size(0.4)
                .color(WHITE)
                .no_baseline()
                .draw();
            if self.clicked(r, state.entry(format!("{text}:+")).or_default()) {
                *value = (*value + step).min(range.end);
            }

            Rect::new(0., 0., x + s * 2., cy + s)
        })
    }

    pub fn hgrids(&mut self, width: f32, height: f32, row_num: u32, count: u32, mut content: impl FnMut(&mut Self, u32)) -> (f32, f32) {
        let mut sh = 0.;
        let w = width / row_num as f32;
        for i in (0..count).step_by(row_num as usize) {
            let mut sw = 0.;
            for j in 0..(count - i).min(row_num) {
                content(self, i + j);
                self.dx(w);
                sw += w;
            }
            self.dx(-sw);
            self.dy(height);
            sh += height;
        }
        self.dy(-sh);
        (width, sh)
    }

    pub fn avatar(&mut self, cx: f32, cy: f32, r: f32, t: f32, avatar: Result<Option<SafeTexture>, SafeTexture>) -> Rect {
        rounded_rect_shadow(
            self,
            Rect::new(cx - r, cy - r, r * 2., r * 2.),
            &ShadowConfig {
                radius: r,
                ..Default::default()
            },
        );
        let rect = Rect::new(cx - r, cy - r, r * 2., r * 2.);
        match avatar {
            Ok(Some(avatar)) => {
                self.fill_circle(cx, cy, r, (*avatar, rect));
            }
            Ok(None) => {
                self.loading(
                    cx,
                    cy,
                    t,
                    WHITE,
                    LoadingParams {
                        radius: r * 0.6,
                        width: 0.008,
                        ..Default::default()
                    },
                );
            }
            Err(icon) => {
                self.fill_circle(cx, cy, r, semi_black(0.2));
                self.fill_circle(cx, cy, r, (*icon, rect.feather(-0.025), ScaleType::CropCenter, WHITE));
            }
        }
        self.stroke_circle(cx, cy, r, 0.004, WHITE);
        rect
    }

    pub fn loading_path(start: f32, len: f32, r: f32) -> Path {
        use lyon::math::{point, vector, Angle};
        let mut path = Path::svg_builder();
        let pt = |a: f32| {
            let (sin, cos) = a.sin_cos();
            point(sin * r, cos * r)
        };
        path.move_to(pt(-start));
        path.arc(point(0., 0.), vector(r, r), Angle::radians(len), Angle::radians(0.));
        path.build()
    }

    const LOADING_SCALE: f32 = 0.74;
    const LOADING_CHANGE_SPEED: f32 = 3.5;
    const LOADING_ROTATE_SPEED: f32 = 4.1;

    pub fn loading<'b>(&mut self, cx: f32, cy: f32, t: f32, shading: impl IntoShading, params: impl Into<LoadingParams<'b>>) {
        use std::f32::consts::PI;

        let params = params.into();
        let (st, mut len) = if let Some(p) = params.progress {
            (t * Self::LOADING_ROTATE_SPEED, p * PI * 2.)
        } else {
            let ct = t * Self::LOADING_CHANGE_SPEED;
            let round = (ct / (PI * 2.)).floor();
            let st = round * Self::LOADING_SCALE + {
                let t = ct - round * PI * 2.;
                if t < PI {
                    0.
                } else {
                    ((t - PI * 3. / 2.).sin() + 1.) * Self::LOADING_SCALE / 2.
                }
            };
            let st = st * PI * 2. + t * Self::LOADING_ROTATE_SPEED;
            let len = (-ct.cos() * Self::LOADING_SCALE / 2. + 0.5) * PI * 2.;
            (st, len)
        };
        if let Some(last) = params.last {
            len = (*last * 5. + len) / 6.;
            *last = len;
        }
        self.scope(|ui| {
            ui.dx(cx);
            ui.dy(cy);
            ui.stroke_path(&Self::loading_path(st, len, params.radius), params.width, shading);
        });
    }

    #[inline]
    pub fn back_rect(&self) -> Rect {
        Rect::new(-0.97, -self.top + 0.04, 0.08, 0.08)
    }

    #[inline]
    pub fn tab_rects<'b>(&mut self, t: f32, it: impl IntoIterator<Item = (&'b mut DRectButton, Cow<'b, str>, bool)>) {
        let mut r = Rect::new(-0.92, -self.top + 0.18, 0.2, 0.11);
        for (btn, text, chosen) in it {
            btn.render_text(self, r, t, text, 0.5, chosen);
            r.y += 0.125;
        }
    }

    #[inline]
    pub fn content_rect(&self) -> Rect {
        Rect::new(-0.7, -self.top + 0.15, 1.67, self.top * 2. - 0.18)
    }

    pub fn full_loading<'b>(&mut self, text: impl Into<Cow<'b, str>>, t: f32) {
        self.fill_rect(self.screen_rect(), semi_black(0.6));
        self.loading(0., -0.03, t, WHITE, ());
        self.text(text.into()).pos(0., 0.05).anchor(0.5, 0.).size(0.6).draw();
    }

    pub fn full_loading_simple(&mut self, t: f32) {
        self.fill_rect(self.screen_rect(), semi_black(0.6));
        self.loading(0., 0., t, WHITE, ());
    }

    pub fn main_sub_colors(use_black: bool, alpha: f32) -> (Color, Color) {
        if use_black {
            (semi_black(alpha), semi_black(alpha * 0.64))
        } else {
            (semi_white(alpha), semi_white(alpha * 0.64))
        }
    }
}

pub struct LoadingParams<'a> {
    pub radius: f32,
    pub width: f32,
    pub progress: Option<f32>,
    pub last: Option<&'a mut f32>,
}
impl Default for LoadingParams<'_> {
    fn default() -> Self {
        Self {
            radius: 0.05,
            width: 0.012,
            progress: None,
            last: None,
        }
    }
}
impl From<()> for LoadingParams<'_> {
    fn from(_: ()) -> Self {
        Self::default()
    }
}
impl From<f32> for LoadingParams<'_> {
    fn from(progress: f32) -> Self {
        Self {
            progress: Some(progress),
            ..Self::default()
        }
    }
}
impl<'a> From<(Option<f32>, &'a mut f32)> for LoadingParams<'a> {
    fn from((progress, last): (Option<f32>, &'a mut f32)) -> Self {
        Self {
            progress,
            last: Some(last),
            ..Self::default()
        }
    }
}

fn build_audio() -> AudioManager {
    match {
        #[cfg(target_os = "android")]
        {
            use sasa::backend::oboe::*;
            AudioManager::new(OboeBackend::new(OboeSettings {
                performance_mode: PerformanceMode::PowerSaving,
                usage: Usage::Game,
                ..Default::default()
            }))
        }
        #[cfg(not(target_os = "android"))]
        {
            use sasa::backend::cpal::*;
            AudioManager::new(CpalBackend::new(CpalSettings::default()))
        }
    } {
        Ok(manager) => manager,
        Err(e) => {
            show_error(e.context(ttl!("audio-backend-init-failed")));
            AudioManager::new(DummyBackend).expect("Failed to create dummy audio backend, this should not happen")
        }
    }
}

struct DummyBackend;

impl sasa::backend::Backend for DummyBackend {
    fn setup(&mut self, setup: sasa::backend::BackendSetup) -> anyhow::Result<()> {
        let _ = setup;
        Ok(())
    }
    fn start(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
    fn consume_broken(&self) -> bool {
        false
    }
}

thread_local! {
    pub static UI_AUDIO: RefCell<AudioManager> = RefCell::new(build_audio());
    pub static UI_BTN_HITSOUND_LARGE: RefCell<Option<Sfx>> = const { RefCell::new(None) };
    pub static UI_BTN_HITSOUND: RefCell<Option<Sfx>> = const { RefCell::new(None) };
    pub static UI_SWITCH_SOUND: RefCell<Option<Sfx>> = const { RefCell::new(None) };
}

pub fn button_hit() {
    UI_BTN_HITSOUND.with(|it| {
        if let Some(sfx) = it.borrow_mut().as_mut() {
            let _ = sfx.play(PlaySfxParams::default());
        }
    });
}

pub fn button_hit_large() {
    UI_BTN_HITSOUND_LARGE.with(|it| {
        if let Some(sfx) = it.borrow_mut().as_mut() {
            let _ = sfx.play(PlaySfxParams::default());
        }
    });
}

pub fn list_switch() {
    UI_SWITCH_SOUND.with(|it| {
        if let Some(sfx) = it.borrow_mut().as_mut() {
            let _ = sfx.play(PlaySfxParams::default());
        }
    });
}
