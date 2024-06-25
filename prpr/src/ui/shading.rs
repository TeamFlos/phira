use crate::{
    core::{Point, Tweenable},
    ui::{source_of_image, Matrix, ScaleType},
};
use macroquad::prelude::*;

pub trait Shading {
    fn new_vertex(&self, mat: &Matrix, p: &Point, alpha: f32) -> Vertex;
    fn texture(&self) -> Option<Texture2D>;
}

pub struct GradientShading {
    origin: (f32, f32),
    color: Color,
    vector: (f32, f32),
    color_end: Color,
}

impl Shading for GradientShading {
    fn new_vertex(&self, mat: &Matrix, p: &Point, alpha: f32) -> Vertex {
        let t = mat.transform_point(p);
        let mut color = {
            let (dx, dy) = (p.x - self.origin.0, p.y - self.origin.1);
            Color::tween(&self.color, &self.color_end, dx * self.vector.0 + dy * self.vector.1)
        };
        color.a *= alpha;

        Vertex::new(t.x, t.y, 0., 0., 0., color)
    }

    fn texture(&self) -> Option<Texture2D> {
        None
    }
}

pub struct TextureShading {
    texture: (Texture2D, Rect, Rect),
    color: Color,
}

impl Shading for TextureShading {
    fn new_vertex(&self, mat: &Matrix, p: &Point, alpha: f32) -> Vertex {
        let t = mat.transform_point(p);
        let (_, tr, dr) = self.texture;
        let ux = (p.x - dr.x) / dr.w;
        let uy = (p.y - dr.y) / dr.h;
        // let ux = ux.clamp(0., 1.);
        // let uy = uy.clamp(0., 1.);
        Vertex::new(
            t.x,
            t.y,
            0.,
            tr.x + tr.w * ux,
            tr.y + tr.h * uy,
            Color {
                a: self.color.a * alpha,
                ..self.color
            },
        )
    }

    fn texture(&self) -> Option<Texture2D> {
        Some(self.texture.0)
    }
}

pub struct RadialShading {
    origin: Point,
    radius: f32,
    color: Color,
    color_end: Color,
}

impl Shading for RadialShading {
    fn new_vertex(&self, mat: &Matrix, p: &Point, alpha: f32) -> Vertex {
        let e = (p - self.origin).norm() / self.radius;
        let mut color = Color::tween(&self.color, &self.color_end, e);
        color.a *= alpha;
        let t = mat.transform_point(p);
        Vertex::new(t.x, t.y, 0., 0., 0., color)
    }

    fn texture(&self) -> Option<Texture2D> {
        None
    }
}

pub trait IntoShading {
    type Target: Shading;

    fn into_shading(self) -> Self::Target;
}

impl<T: Shading> IntoShading for T {
    type Target = T;

    fn into_shading(self) -> Self::Target {
        self
    }
}

impl IntoShading for Color {
    type Target = GradientShading;

    fn into_shading(self) -> Self::Target {
        GradientShading {
            origin: (0., 0.),
            color: self,
            vector: (1., 0.),
            color_end: self,
        }
    }
}

impl IntoShading for (Color, (f32, f32), Color, (f32, f32)) {
    type Target = GradientShading;

    fn into_shading(self) -> Self::Target {
        let (color, origin, color_end, end) = self;
        let vector = (end.0 - origin.0, end.1 - origin.1);
        let norm = vector.0.hypot(vector.1);
        let vector = (vector.0 / norm, vector.1 / norm);
        let color_end = Color::tween(&color, &color_end, 1. / norm);
        GradientShading {
            origin,
            color,
            vector,
            color_end,
        }
    }
}

impl IntoShading for (Color, (f32, f32), Color, f32) {
    type Target = RadialShading;

    fn into_shading(self) -> Self::Target {
        let (color, origin, color_end, radius) = self;
        RadialShading {
            origin: Point::new(origin.0, origin.1),
            radius,
            color,
            color_end,
        }
    }
}

impl IntoShading for (Texture2D, Rect) {
    type Target = TextureShading;

    #[inline]
    fn into_shading(self) -> Self::Target {
        let (tex, rect) = self;
        (tex, rect, ScaleType::default(), WHITE).into_shading()
    }
}

impl IntoShading for (Texture2D, Rect, ScaleType) {
    type Target = TextureShading;

    #[inline]
    fn into_shading(self) -> Self::Target {
        let (tex, rect, scale_type) = self;
        (tex, rect, scale_type, WHITE).into_shading()
    }
}

impl IntoShading for (Texture2D, Rect, ScaleType, Color) {
    type Target = TextureShading;

    fn into_shading(self) -> Self::Target {
        let (tex, rect, scale_type, color) = self;
        let source = source_of_image(&tex, rect, scale_type).unwrap_or_else(|| Rect::new(0., 0., 1., 1.));
        TextureShading {
            texture: (tex, source, rect),
            color,
        }
    }
}
