use super::Anim;
use crate::ext::{source_of_image, ScaleType};
use anyhow::Result;
use macroquad::prelude::*;
use miniquad::{Texture, TextureFormat, TextureParams, TextureWrap};
use prpr_avc::AVPixelFormat;
use serde::Deserialize;
use std::{cell::RefCell, io::Write};
use tempfile::NamedTempFile;

thread_local! {
    static VIDEO_BUFFERS: RefCell<[Vec<u8>; 3]> = RefCell::default();
}

#[derive(Deserialize)]
pub struct VideoAttach {
    pub line: usize,
}

pub struct Video {
    video: prpr_avc::Video,
    pub video_file: NamedTempFile,

    material: Material,
    tex_y: Texture2D,
    tex_u: Texture2D,
    tex_v: Texture2D,

    start_time: f64,
    pub duration: f64,
    last_pts: i64,
    scale_type: ScaleType,
    alpha: Anim<f32>,
    dim: Anim<f32>,
}

fn new_tex(w: u32, h: u32) -> Texture2D {
    Texture2D::from_miniquad_texture(Texture::new_render_texture(
        unsafe { get_internal_gl() }.quad_context,
        TextureParams {
            width: w,
            height: h,
            format: TextureFormat::Alpha,
            filter: FilterMode::Linear,
            wrap: TextureWrap::Clamp,
        },
    ))
}

impl Video {
    pub fn new(data: Vec<u8>, start_time: f64, scale_type: ScaleType, alpha: Anim<f32>, dim: Anim<f32>) -> Result<Self> {
        let mut video_file = NamedTempFile::new()?;
        video_file.write_all(&data)?;
        drop(data);
        let video = prpr_avc::Video::open(video_file.path().as_os_str().to_str().unwrap(), AVPixelFormat::YUV420P)?;
        let duration = video.duration();
        let format = video.stream_format();
        let w = format.width as u32;
        let h = format.height as u32;

        let material = load_material(
            shader::VERTEX,
            shader::FRAGMENT,
            MaterialParams {
                pipeline_params: PipelineParams::default(),
                uniforms: Vec::new(),
                textures: vec!["tex_y".to_owned(), "tex_u".to_owned(), "tex_v".to_owned()],
            },
        )?;
        let tex_y = new_tex(w, h);
        let tex_u = new_tex(w / 2, h / 2);
        let tex_v = new_tex(w / 2, h / 2);
        material.set_texture("tex_y", tex_y);
        material.set_texture("tex_u", tex_u);
        material.set_texture("tex_v", tex_v);

        Ok(Self {
            video,
            video_file,

            material,
            tex_y,
            tex_u,
            tex_v,

            start_time,
            duration,
            last_pts: -1,
            scale_type,
            alpha,
            dim,
        })
    }

    pub fn video_file(&self) -> &NamedTempFile {
        &self.video_file
    }

    pub fn update(&mut self, t: f64) -> Result<()> {
        if !(0f64..self.duration).contains(&(t - self.start_time)) {
            return Ok(());
        }
        self.alpha.set_time(t);
        self.dim.set_time(t);
        self.video.seek(self.video.elapsed_to_timestamp(t - self.start_time));

        self.video.with_frame(|frame, pts| {
            if self.last_pts == pts {
                return;
            }
            self.last_pts = pts;
            if pts == -1 {
                return;
            }

            VIDEO_BUFFERS.with_borrow_mut(|buf| {
                buf[0].clear();
                buf[1].clear();
                buf[2].clear();
                frame.get_data(0, &mut buf[0]);
                frame.get_data_half(1, &mut buf[1]);
                frame.get_data_half(2, &mut buf[2]);

                let ctx = unsafe { get_internal_gl() }.quad_context;
                self.tex_y.raw_miniquad_texture_handle().update(ctx, &buf[0]);
                self.tex_u.raw_miniquad_texture_handle().update(ctx, &buf[1]);
                self.tex_v.raw_miniquad_texture_handle().update(ctx, &buf[2]);
            });
        });

        Ok(())
    }

    pub fn render(&self, t: f64, aspect_ratio: f32, color: Color) {
        if !(0f64..self.duration).contains(&(t - self.start_time)) {
            return;
        }
        gl_use_material(self.material);
        let top = 1. / aspect_ratio;
        let r = Rect::new(-1., -top, 2., top * 2.);
        let s = source_of_image(&self.tex_y, r, self.scale_type).unwrap_or_else(|| Rect::new(0., 0., 1., 1.));
        let dim = 1. - self.dim.now();
        let color = Color::new(dim * color.r, dim * color.g, dim * color.b, self.alpha.now_opt().unwrap_or(1.) * color.a);
        let vertices = [
            Vertex::new(r.x, r.y, 0., s.x, s.y, color),
            Vertex::new(r.right(), r.y, 0., s.right(), s.y, color),
            Vertex::new(r.x, r.bottom(), 0., s.x, s.bottom(), color),
            Vertex::new(r.right(), r.bottom(), 0., s.right(), s.bottom(), color),
        ];
        let gl = unsafe { get_internal_gl() }.quad_gl;
        gl.draw_mode(DrawMode::Triangles);
        gl.geometry(&vertices, &[0, 2, 3, 0, 1, 3]);
        gl_use_default_material();
    }

    pub fn reset(&mut self) -> Result<()> {
        self.last_pts = -1;
        self.video.seek(0);
        Ok(())
    }
}

mod shader {
    pub const VERTEX: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;

varying lowp vec2 uv;
varying lowp vec4 color;

uniform mat4 Model;
uniform mat4 Projection;

void main() {
    gl_Position = Projection * Model * vec4(position, 1);
    color = color0 / 255.0;
    uv = texcoord;
}"#;

    pub const FRAGMENT: &str = r#"#version 100
precision lowp float;

varying lowp vec4 color;
varying lowp vec2 uv;

uniform sampler2D tex_y;
uniform sampler2D tex_u;
uniform sampler2D tex_v;

void main() {
    float clamp = step(uv.x, 1.0) * step(0.0, uv.x) * step(uv.y, 1.0) * step(0.0, uv.y);
    vec3 yuv = vec3(
        texture2D(tex_y, uv).a,
        texture2D(tex_u, uv).a - 0.5,
        texture2D(tex_v, uv).a - 0.5
    );
    yuv.x = 1.1643 * (yuv.x - 0.0625);
    mat3 color_matrix = mat3(
        vec3(1.0,   0.0,     1.402),
        vec3(1.0,  -0.344,  -0.714),
        vec3(1.0,   1.772,   0.0  )
    );

    gl_FragColor = vec4(yuv * color_matrix, 1.0) * color * clamp;
}"#;
}
