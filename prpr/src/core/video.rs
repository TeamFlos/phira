use crate::ext::{source_of_image, ScaleType};

use super::{Anim, Resource};
use anyhow::{bail, Context, Result};
use macroquad::prelude::*;
use miniquad::{Texture, TextureFormat, TextureParams, TextureWrap};
use std::{
    cell::RefCell,
    io::{BufRead, Read, Write},
    path::Path,
    process::{Child, ChildStdout, Command, Stdio},
};
use tempfile::NamedTempFile;

thread_local! {
    static VIDEO_BUFFERS: RefCell<[Vec<u8>; 3]> = RefCell::default();
}

pub struct Video {
    child: Child,
    child_output: Option<ChildStdout>,
    _video_file: NamedTempFile,

    material: Material,
    tex_y: Texture2D,
    tex_u: Texture2D,
    tex_v: Texture2D,

    start_time: f32,
    scale_type: ScaleType,
    alpha: Anim<f32>,
    dim: Anim<f32>,
    size: (u32, u32),
    frame_delta: f64,
    next_frame: usize,
    ended: bool,
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
    pub fn new(ffmpeg: &Path, data: Vec<u8>, start_time: f32, scale_type: ScaleType, alpha: Anim<f32>, dim: Anim<f32>) -> Result<Self> {
        let mut video_file = NamedTempFile::new()?;
        video_file.write_all(&data)?;
        drop(data);
        let (fps, (w, h)) = || -> Result<(f64, (u32, u32))> {
            for line in Command::new(ffmpeg)
                .arg("-i")
                .arg(video_file.path())
                .arg("-hide_banner")
                .output()?
                .stderr
                .lines()
            {
                let line = line?;
                let line = line.trim();
                if line.starts_with("Stream #0") {
                    let mut fps: Option<f64> = None;
                    let mut size: Option<(u32, u32)> = None;
                    for info in line.split(',') {
                        if let Some(s) = info.strip_suffix(" fps") {
                            fps = Some(s.trim().parse()?);
                        } else if let Some(s) = info.trim().split(' ').next() {
                            if let Some((w, h)) = s.split_once('x') {
                                size = Some((w.parse()?, h.parse()?));
                            }
                        }
                    }
                    if let (Some(fps), Some(size)) = (fps, size) {
                        return Ok((fps, size));
                    } else {
                        bail!("Video info line is not complete");
                    }
                }
            }
            bail!("Video info line is not found");
        }()
        .context("Failed to get frame rate")?;
        let frame_delta = 1. / fps;

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

        let mut child = Command::new(ffmpeg)
            .arg("-i")
            .arg(video_file.path())
            .args(["-f", "rawvideo", "-pix_fmt", "yuv420p", "-"])
            .stderr(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()?;
        let child_output = child.stdout.take().unwrap();
        Ok(Self {
            child,
            child_output: Some(child_output),
            _video_file: video_file,

            material,
            tex_y,
            tex_u,
            tex_v,

            start_time,
            scale_type,
            alpha,
            dim,
            size: (w, h),
            frame_delta,
            next_frame: 0,
            ended: false,
        })
    }

    pub fn update(&mut self, t: f32) -> Result<()> {
        if t < self.start_time || self.ended {
            return Ok(());
        }
        self.alpha.set_time(t);
        self.dim.set_time(t);
        let that_frame = ((t - self.start_time) as f64 / self.frame_delta) as usize;
        if self.next_frame <= that_frame {
            let result = VIDEO_BUFFERS.with(|it| -> Result<()> {
                let mut buf = it.borrow_mut();
                let (w, h) = self.size;
                let (w, h) = (w as usize, h as usize);
                buf[0].resize(w * h, 0);
                buf[1].resize(w * h / 4, 0);
                buf[2].resize(w * h / 4, 0);
                let out = self.child_output.as_mut().unwrap();
                while self.next_frame <= that_frame {
                    out.read_exact(&mut buf[0])?;
                    out.read_exact(&mut buf[1])?;
                    out.read_exact(&mut buf[2])?;
                    self.next_frame += 1;
                }
                let ctx = unsafe { get_internal_gl() }.quad_context;
                self.tex_y.raw_miniquad_texture_handle().update(ctx, &buf[0]);
                self.tex_u.raw_miniquad_texture_handle().update(ctx, &buf[1]);
                self.tex_v.raw_miniquad_texture_handle().update(ctx, &buf[2]);
                Ok(())
            });
            if result.is_err() {
                self.ended = true;
            }
        }
        Ok(())
    }

    pub fn render(&self, res: &Resource) {
        if res.time < self.start_time || self.ended {
            return;
        }
        gl_use_material(self.material);
        let top = 1. / res.aspect_ratio;
        let r = Rect::new(-1., -top, 2., top * 2.);
        let s = source_of_image(&self.tex_y, r, self.scale_type).unwrap_or_else(|| Rect::new(0., 0., 1., 1.));
        let dim = 1. - self.dim.now();
        let color = Color::new(dim, dim, dim, self.alpha.now_opt().unwrap_or(1.));
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
}

impl Drop for Video {
    fn drop(&mut self) {
        drop(self.child_output.take().unwrap());
        let _ = self.child.wait();
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

    gl_FragColor = vec4(yuv * color_matrix, 1.0) * color;
}"#;
}
