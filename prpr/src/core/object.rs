use super::{AnimFloat, AnimVector, Color, Matrix, Resource, Vector};
use macroquad::prelude::*;
use nalgebra::Rotation2;

/// Describes the animation of an in-game object's local coordinate system in the parent coordinate system
#[derive(Default)]
pub struct Object {
    pub alpha: AnimFloat,
    pub scale: AnimVector,
    /// Rotation in degrees
    pub rotation: AnimFloat,
    pub translation: AnimVector,
}

impl Object {
    pub fn is_default(&self) -> bool {
        self.alpha.is_default()
            && self.scale.0.is_default()
            && self.scale.1.is_default()
            && self.rotation.is_default()
            && self.translation.0.is_default()
            && self.translation.1.is_default()
    }

    pub fn set_time(&mut self, time: f32) {
        self.alpha.set_time(time);
        self.scale.0.set_time(time);
        self.scale.1.set_time(time);
        self.rotation.set_time(time);
        self.translation.0.set_time(time);
        self.translation.1.set_time(time);
    }

    pub fn dead(&self) -> bool {
        self.alpha.dead()
            && self.scale.0.dead()
            && self.scale.1.dead()
            && self.rotation.dead()
            && self.translation.0.dead()
            && self.translation.1.dead()
    }

    pub fn now(&self, res: &Resource) -> Matrix {
        self.now_rotation().append_translation(&self.now_translation(res))
    }

    #[inline]
    pub fn now_rotation(&self) -> Matrix {
        Rotation2::new(self.rotation.now().to_radians()).to_homogeneous()
    }

    #[inline]
    pub fn now_translation(&self, res: &Resource) -> Vector {
        let mut tr = self.translation.now();
        tr.y /= res.aspect_ratio;
        tr
    }

    #[inline]
    pub fn now_alpha(&self) -> f32 {
        self.alpha.now_opt().unwrap_or(1.0).max(0.)
    }

    #[inline]
    pub fn now_color(&self) -> Color {
        Color::new(1.0, 1.0, 1.0, self.now_alpha())
    }

    #[inline]
    pub fn now_scale(&self, ct: Vector) -> Matrix {
        let scale = self.scale.now_with_def(1.0, 1.0);
        Matrix::new_translation(&-ct).append_nonuniform_scaling(&scale).append_translation(&ct)
    }
}

/// Describes the animation of an in-game object in its local coordinate system
#[derive(Default, Clone)]
pub struct CtrlObject {
    pub alpha: AnimFloat,
    pub size: AnimFloat,
    pub pos: AnimFloat,
    pub y: AnimFloat,
}

impl CtrlObject {
    pub fn set_height(&mut self, height: f32) {
        self.alpha.set_time(height);
        self.size.set_time(height);
        self.pos.set_time(height);
        self.y.set_time(height);
    }
}
