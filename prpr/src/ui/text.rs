use crate::{
    core::{Matrix, Vector},
    ext::get_viewport,
};
use glyph_brush::{
    ab_glyph::{Font, FontArc, Glyph, ScaleFont},
    BrushAction, BrushError, FontId, GlyphBrush, GlyphBrushBuilder, GlyphCruncher, Layout, Section, SectionGlyph, Text,
};
use macroquad::{
    miniquad::{Texture, TextureParams},
    prelude::*,
};
use std::borrow::Cow;

use super::Ui;

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
        }
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

    fn measure_inner<'c>(&mut self, text: &'c str, painter: &mut Option<&mut TextPainter>) -> (Section<'c>, Rect) {
        let vp = get_viewport();
        let scale = self.get_scale(vp.2);
        let mut section = Section::new().add_text(Text::new(text).with_scale(scale).with_color(self.color));
        let s = 2. / vp.2 as f32;
        if let Some(max_width) = self.max_width {
            section = section.with_bounds((max_width / s, f32::INFINITY));
        }
        if !self.multiline {
            section = section.with_layout(Layout::default_single_line());
        }
        macro_rules! painter {
            ($t:expr) => {
                if let Some(painter) = painter.as_mut() {
                    ($t)(painter)
                } else {
                    let painter = &mut self.ui.text_painter;
                    ($t)(painter)
                }
            };
        }
        let bound = painter!(|p: &mut TextPainter| p.brush.glyph_bounds(&section).unwrap_or_default());
        let mut height = bound.height();
        height += text.chars().take_while(|it| *it == '\n').count() as f32 * painter!(|p: &mut TextPainter| p.line_gap(scale)) * 3.;
        if self.baseline {
            height += painter!(|p: &mut TextPainter| p.brush.fonts()[0].as_scaled(scale).descent());
        }
        let mut rect = Rect::new(self.pos.0, self.pos.1, bound.width() * s, height * s);
        rect.x -= rect.w * self.anchor.0;
        rect.y -= rect.h * self.anchor.1;
        (section, rect)
    }

    pub fn measure_with_font(&mut self, mut painter: Option<&mut TextPainter>) -> Rect {
        let text = self.text.take().unwrap();
        let (_, rect) = self.measure_inner(&text, &mut painter);
        self.text = Some(text);
        rect
    }

    #[inline]
    pub fn measure(&mut self) -> Rect {
        self.measure_with_font(None)
    }

    fn paint_on(painter: &mut TextPainter, mut section: Section, scale: f32, ml: bool) {
        use glyph_brush::ab_glyph::{Point, Rect};
        if ml {
            painter.brush.queue(section);
            return;
        }
        let extras = section.text.iter().map(|it| it.extra).collect();
        let bounds = section.bounds;
        let bounds = Rect {
            min: Point { x: 0., y: 0. },
            max: Point { x: bounds.0, y: bounds.1 },
        };
        section.bounds.0 = f32::INFINITY;
        let mut glyphs: Vec<_> = painter.brush.glyphs(section).cloned().collect();
        let Some(last) = glyphs.last() else { return };
        let end = |glyph: &SectionGlyph| glyph.glyph.position.x + painter.brush.fonts()[glyph.font_id].as_scaled(scale).h_advance(glyph.glyph.id);
        if end(last) <= bounds.max.x {
            painter.brush.queue_pre_positioned(glyphs, extras, bounds);
            return;
        }
        const C: char = '…';
        let font = painter.brush.fonts()[0].as_scaled(scale);
        let id = font.glyph_id(C);
        let w = font.h_advance(id);
        if w > bounds.max.x {
            return;
        }
        let index = glyphs.partition_point(|it| end(it) <= bounds.max.x - w);
        let y = last.glyph.position.y;
        let st = if index == 0 { 0. } else { end(&glyphs[index - 1]) };
        glyphs.truncate(index);
        glyphs.push(SectionGlyph {
            section_index: 0,
            byte_index: index,
            glyph: Glyph {
                id,
                position: Point { x: st, y },
                scale: scale.into(),
            },
            font_id: FontId(0),
        });
        painter.brush.queue_pre_positioned(glyphs, extras, bounds);
    }

    pub fn draw_with_font(mut self, mut painter: Option<&mut TextPainter>) -> Rect {
        let text = std::mem::take(&mut self.text).unwrap();
        let (section, rect) = self.measure_inner(&text, &mut painter);
        let vp = get_viewport();
        let s = vp.2 as f32 / 2.;
        let scale = self.get_scale(vp.2);
        if let Some(painter) = &mut painter {
            Self::paint_on(painter, section, scale, self.multiline);
        } else {
            Self::paint_on(self.ui.text_painter, section, scale, self.multiline);
        }
        self.ui
            .with((Matrix::new_scaling(1. / s) * self.scale).append_translation(&Vector::new(rect.x, rect.y)), |ui| {
                ui.apply(|ui| {
                    if let Some(painter) = painter {
                        painter.submit();
                    } else {
                        ui.text_painter.submit();
                    }
                });
            });
        rect
    }

    #[inline]
    pub fn draw(self) -> Rect {
        self.draw_with_font(None)
    }
}

pub struct TextPainter {
    brush: GlyphBrush<[Vertex; 4]>,
    cache_texture: Texture2D,
    data_buffer: Vec<u8>,
    vertices_buffer: Vec<Vertex>,
}

impl TextPainter {
    pub fn new(font: FontArc) -> Self {
        let mut brush = GlyphBrushBuilder::using_font(font).build();
        brush.resize_texture(2048, 2048);
        // TODO optimize
        let cache_texture = Self::new_cache_texture(brush.texture_dimensions());
        Self {
            brush,
            cache_texture,
            data_buffer: Vec::new(),
            vertices_buffer: Vec::new(),
        }
    }

    fn new_cache_texture(dim: (u32, u32)) -> Texture2D {
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

    fn submit(&mut self) {
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
                    let color = vertex.extra.color.into();
                    [
                        Vertex::new(pos.min.x, pos.min.y, 0., uv.min.x, uv.min.y, color),
                        Vertex::new(pos.max.x, pos.min.y, 0., uv.max.x, uv.min.y, color),
                        Vertex::new(pos.min.x, pos.max.y, 0., uv.min.x, uv.max.y, color),
                        Vertex::new(pos.max.x, pos.max.y, 0., uv.max.x, uv.max.y, color),
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
                    self.redraw();
                    break;
                }
                Ok(BrushAction::ReDraw) => {
                    self.redraw();
                    break;
                }
            }
        }
    }

    fn redraw(&self) {
        let gl = unsafe { get_internal_gl() }.quad_gl;
        gl.texture(Some(self.cache_texture));
        for vertices in self.vertices_buffer.chunks_exact(4) {
            gl.geometry(vertices, &[0, 2, 3, 0, 1, 3]);
        }
    }
}

impl Drop for TextPainter {
    fn drop(&mut self) {
        self.cache_texture.delete();
    }
}
