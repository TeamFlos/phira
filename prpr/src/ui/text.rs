use super::Ui;
use crate::{
    core::{Matrix, Point, Vector},
    ext::get_viewport,
};
use glyph_brush::{
    ab_glyph::{Font, FontArc, ScaleFont},
    BrushAction, BrushError, FontId, GlyphBrush, GlyphBrushBuilder, GlyphCruncher, HorizontalAlign, Layout, Section, SectionGlyph, Text,
};
use macroquad::{
    miniquad::{Texture, TextureParams},
    prelude::*,
};
use once_cell::sync::Lazy;
use std::{borrow::Cow, cell::RefCell, collections::HashSet, thread::LocalKey};
use tracing::debug;

#[must_use = "DrawText does nothing until you 'draw' it"]
pub struct DrawText<'a, 's, 'ui> {
    pub ui: &'ui mut Ui<'a>,
    text: Option<Cow<'s, str>>,
    size: f32,
    pos: (f32, f32),
    anchor: (f32, f32),
    color: Color,
    max_width: Option<f32>,
    baseline: bool,
    multiline: bool,
    scale: Matrix,
    h_align: HorizontalAlign,
}

impl<'a, 's, 'ui> DrawText<'a, 's, 'ui> {
    pub(crate) fn new(ui: &'ui mut Ui<'a>, text: Cow<'s, str>) -> Self {
        Self {
            ui,
            text: Some(text),
            size: 1.,
            pos: (0., 0.),
            anchor: (0., 0.),
            color: WHITE,
            max_width: None,
            baseline: true,
            multiline: false,
            scale: Matrix::identity(),
            h_align: HorizontalAlign::Left,
        }
    }

    pub fn h_center(mut self) -> Self {
        self.h_align = HorizontalAlign::Center;
        self
    }

    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    pub fn pos(mut self, x: f32, y: f32) -> Self {
        self.pos = (x, y);
        self
    }

    pub fn anchor(mut self, x: f32, y: f32) -> Self {
        self.anchor = (x, y);
        self
    }

    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    pub fn max_width(mut self, max_width: f32) -> Self {
        self.max_width = Some(max_width);
        self
    }

    pub fn no_baseline(mut self) -> Self {
        self.baseline = false;
        self
    }

    pub fn multiline(mut self) -> Self {
        self.multiline = true;
        self
    }

    pub fn scale(mut self, scale: Matrix) -> Self {
        self.scale = scale;
        self
    }

    fn get_scale(&self, w: i32) -> f32 {
        0.04 * self.size * w as f32
    }

    fn bounds(&self, (x, y, w, h): (f32, f32, f32, f32)) -> Rect {
        let vp = get_viewport();
        let s = 2. / vp.2 as f32;
        let mut rect = Rect::new(self.pos.0 - x * s, self.pos.1 - y * s, w * s, h * s);
        rect.x -= rect.w * self.anchor.0;
        rect.y -= rect.h * self.anchor.1;
        rect
    }

    fn measure_inner<'c>(&mut self, text: &'c str, painter: &mut Option<&mut TextPainter>) -> (Section<'c>, (f32, f32, f32, f32)) {
        use glyph_brush::ab_glyph;
        let vp = get_viewport();
        let scale = self.get_scale(vp.2);

        let default_text_painter = &mut self.ui.text_painter;
        let painter = painter.as_deref_mut().unwrap_or(default_text_painter);

        let mut section = Section::new().with_layout(Layout::default().h_align(self.h_align));
        if painter.brush.fonts().len() > 1 {
            let mut last = 0;
            let mut last_contain = false;
            for (i, c) in text.char_indices() {
                let contain = painter.valid_chars.contains(&c);
                if last_contain != contain {
                    if last != i {
                        section = section.add_text(
                            Text::new(&text[last..i])
                                .with_scale(scale)
                                .with_color(self.color)
                                .with_font_id(FontId((!last_contain) as usize)),
                        );
                    }
                    last = i;
                    last_contain = contain;
                }
            }
            if last != text.len() {
                section = section.add_text(
                    Text::new(&text[last..])
                        .with_scale(scale)
                        .with_color(self.color)
                        .with_font_id(FontId((!last_contain) as usize)),
                );
            }
        } else {
            section = section.add_text(Text::new(text).with_scale(scale).with_color(self.color));
        }

        let s = 2. / vp.2 as f32;
        if let Some(max_width) = self.max_width {
            section = section.with_bounds((max_width / s, f32::INFINITY));
        }
        let font = painter.brush.fonts()[0].as_scaled(scale);
        let line_height = if self.baseline { font.ascent() } else { font.height() };

        if !self.multiline {
            let bounds = section.bounds;
            let bounds = ab_glyph::Rect {
                min: ab_glyph::Point { x: 0., y: 0. },
                max: ab_glyph::Point { x: bounds.0, y: bounds.1 },
            };
            section.bounds.0 = f32::INFINITY;
            let glyphs: Vec<_> = painter.brush.glyphs(section.clone()).cloned().collect();
            let Some(last) = glyphs.last() else {
                return (section, (0., 0., 0., line_height));
            };
            let end = |glyph: &SectionGlyph| glyph.glyph.position.x + painter.brush.fonts()[glyph.font_id].as_scaled(scale).h_advance(glyph.glyph.id);
            if end(last) <= bounds.max.x {
                return (section, (0., 0., end(last), line_height));
            }
            let font = painter.brush.fonts()[0].as_scaled(scale);
            let id = font.glyph_id('…');
            let w = font.h_advance(id);
            if w > bounds.max.x {
                return (section, (0., 0., 0., line_height));
            }
            let index = glyphs.partition_point(|it| end(it) <= bounds.max.x - w);
            let st = if index == 0 { 0. } else { end(&glyphs[index - 1]) };
            let byte_index = if index == 0 { 0 } else { glyphs[index - 1].byte_index };
            // Round to char boundary
            let byte_index = text[..byte_index].char_indices().next_back().map_or(0, |(i, _)| i);
            return (
                section.with_text(vec![
                    Text::new(&text[..byte_index]).with_scale(scale).with_color(self.color),
                    Text::new("…").with_scale(scale).with_color(self.color),
                ]),
                (0., 0., st + w, line_height),
            );
        }
        let bound = painter.brush.glyph_bounds(&section).unwrap_or_default();
        let mut height = bound.height();
        height += text.chars().take_while(|it| *it == '\n').count() as f32 * painter.line_gap(scale) * 3.;
        if self.baseline {
            height += painter.brush.fonts()[0].as_scaled(scale).descent();
        }
        (section, (bound.min.x, bound.min.y, bound.width(), height))
    }

    pub fn measure_with_font(&mut self, mut painter: Option<&mut TextPainter>) -> Rect {
        let text = self.text.take().unwrap();
        let (_, bound) = self.measure_inner(&text, &mut painter);
        self.text = Some(text);
        self.bounds(bound)
    }

    pub fn measure_using(&mut self, font: &'static LocalKey<RefCell<Option<TextPainter>>>) -> Rect {
        font.with(|it| self.measure_with_font(it.borrow_mut().as_mut()))
    }

    #[inline]
    pub fn measure(&mut self) -> Rect {
        self.measure_with_font(None)
    }

    pub fn draw_with_font(&mut self, mut painter: Option<&mut TextPainter>) -> Rect {
        let text = std::mem::take(&mut self.text).unwrap();
        let (section, bound) = self.measure_inner(&text, &mut painter);
        let rect = self.bounds(bound);
        let vp = get_viewport();
        let s = vp.2 as f32 / 2.;
        if let Some(painter) = &mut painter {
            painter.brush.queue(section);
        } else {
            self.ui.text_painter.brush.queue(section);
        }
        self.ui
            .with((Matrix::new_scaling(1. / s) * self.scale).append_translation(&Vector::new(rect.x, rect.y)), |ui| {
                /* ui.apply(|ui| {
                    let tr = Matrix::identity();
                    if let Some(painter) = painter {
                        painter.submit(tr, ui.alpha);
                    } else {
                        ui.text_painter.submit(tr, ui.alpha);
                    }
                }); */
                if let Some(painter) = painter {
                    painter.submit(ui.transform, ui.alpha);
                } else {
                    ui.text_painter.submit(ui.transform, ui.alpha);
                }
            });
        self.text = Some(text);
        rect
    }

    pub fn draw_using(&mut self, font: &'static LocalKey<RefCell<Option<TextPainter>>>) -> Rect {
        font.with(|it| self.draw_with_font(it.borrow_mut().as_mut()))
    }

    #[inline]
    pub fn draw(&mut self) -> Rect {
        self.draw_with_font(None)
    }
}

static TEXTURE_DIM: Lazy<u32> = Lazy::new(|| unsafe {
    use miniquad::gl::*;
    let mut size = 0;
    glGetIntegerv(GL_MAX_TEXTURE_SIZE, &mut size);
    (size as u32).min(2048)
});

#[derive(Clone)]
struct MyVertex {
    pos: (f32, f32),
    uv: (f32, f32),
    color: Color,
}
impl MyVertex {
    pub fn new(x: f32, y: f32, u: f32, v: f32, color: Color) -> Self {
        Self {
            pos: (x, y),
            uv: (u, v),
            color,
        }
    }
}

pub struct TextPainter {
    brush: GlyphBrush<[MyVertex; 4]>,
    cache_texture: Texture2D,
    data_buffer: Vec<u8>,
    vertices_buffer: Vec<MyVertex>,

    valid_chars: HashSet<char>,
}

impl TextPainter {
    pub fn new(font: FontArc, fallback: Option<FontArc>) -> Self {
        let valid_chars = font.codepoint_ids().map(|it| it.1).chain(" \n\t".chars()).collect();

        let mut fonts = vec![font];
        if let Some(fallback) = fallback {
            fonts.push(fallback);
        }
        let mut brush = GlyphBrushBuilder::using_fonts(fonts).build();
        let dim = *TEXTURE_DIM;
        brush.resize_texture(dim, dim);
        // TODO optimize
        let cache_texture = Self::new_cache_texture(brush.texture_dimensions());
        Self {
            brush,
            cache_texture,
            data_buffer: Vec::new(),
            vertices_buffer: Vec::new(),

            valid_chars,
        }
    }

    fn new_cache_texture(dim: (u32, u32)) -> Texture2D {
        debug!("creating cache texture: {}x{}", dim.0, dim.1);
        Texture2D::from_miniquad_texture(Texture::new_render_texture(
            unsafe { get_internal_gl() }.quad_context,
            TextureParams {
                width: dim.0,
                height: dim.1,
                filter: FilterMode::Linear,
                format: miniquad::TextureFormat::RGBA8,
                wrap: miniquad::TextureWrap::Clamp,
            },
        ))
    }

    pub fn line_gap(&self, scale: f32) -> f32 {
        self.brush.fonts()[0].as_scaled(scale).line_gap()
    }

    fn submit(&mut self, tr: Matrix, alpha: f32) {
        let mut flushed = false;
        loop {
            match self.brush.process_queued(
                |rect, tex_data| unsafe {
                    if !flushed {
                        get_internal_gl().flush();
                        flushed = true;
                    }
                    use miniquad::gl::*;
                    glBindTexture(GL_TEXTURE_2D, self.cache_texture.raw_miniquad_texture_handle().gl_internal_id());
                    self.data_buffer.clear();
                    self.data_buffer.reserve(tex_data.len() * 4);
                    for alpha in tex_data {
                        self.data_buffer.extend_from_slice(&[255, 255, 255, *alpha]);
                    }
                    glTexSubImage2D(
                        GL_TEXTURE_2D,
                        0,
                        rect.min[0] as _,
                        rect.min[1] as _,
                        rect.width() as _,
                        rect.height() as _,
                        GL_RGBA,
                        GL_UNSIGNED_BYTE,
                        self.data_buffer.as_ptr() as _,
                    );
                },
                |vertex| {
                    let pos = &vertex.pixel_coords;
                    let uv = &vertex.tex_coords;
                    let mut color: Color = vertex.extra.color.into();
                    color.a *= alpha;
                    [
                        MyVertex::new(pos.min.x, pos.min.y, uv.min.x, uv.min.y, color),
                        MyVertex::new(pos.max.x, pos.min.y, uv.max.x, uv.min.y, color),
                        MyVertex::new(pos.min.x, pos.max.y, uv.min.x, uv.max.y, color),
                        MyVertex::new(pos.max.x, pos.max.y, uv.max.x, uv.max.y, color),
                    ]
                },
            ) {
                Err(BrushError::TextureTooSmall { suggested }) => {
                    if !flushed {
                        unsafe { get_internal_gl() }.flush();
                        flushed = true;
                    }
                    self.cache_texture.delete();
                    self.cache_texture = Self::new_cache_texture(suggested);
                    self.brush.resize_texture(suggested.0, suggested.1);
                }
                Ok(BrushAction::Draw(vertices)) => {
                    self.vertices_buffer.clear();
                    self.vertices_buffer.extend(vertices.into_iter().flatten());
                    self.redraw(tr);
                    break;
                }
                Ok(BrushAction::ReDraw) => {
                    self.redraw(tr);
                    break;
                }
            }
        }
    }

    fn transform(&self, vertex: &MyVertex, tr: Matrix) -> Vertex {
        let pos = tr.transform_point(&Point::new(vertex.pos.0, vertex.pos.1));
        Vertex::new(pos.x, pos.y, 0., vertex.uv.0, vertex.uv.1, vertex.color)
    }

    fn redraw(&self, tr: Matrix) {
        let gl = unsafe { get_internal_gl() }.quad_gl;
        gl.texture(Some(self.cache_texture));
        for vertices in self.vertices_buffer.chunks_exact(4) {
            let vertices = [
                self.transform(&vertices[0], tr),
                self.transform(&vertices[1], tr),
                self.transform(&vertices[2], tr),
                self.transform(&vertices[3], tr),
            ];
            gl.geometry(&vertices, &[0, 2, 3, 0, 1, 3]);
        }
    }
}

impl Drop for TextPainter {
    fn drop(&mut self) {
        self.cache_texture.delete();
    }
}
