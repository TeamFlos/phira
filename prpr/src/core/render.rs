use macroquad::{
    texture::{RenderTarget, Texture2D},
    window::get_internal_gl,
};
use macroquad::miniquad::{self as miniquad, gl::GLuint};

pub struct MSRenderTarget {
    dim: (u32, u32),
    fbo: GLuint,
    input: RenderTarget,
    output: [Option<RenderTarget>; 2],
}

pub fn copy_fbo(src: GLuint, dst: GLuint, dim: (u32, u32)) -> bool {
    unsafe {
        use miniquad::gl::*;
        glBindFramebuffer(GL_READ_FRAMEBUFFER, src);
        glBindFramebuffer(GL_DRAW_FRAMEBUFFER, dst);
        let (w, h) = (dim.0 as i32, dim.1 as i32);
        glBlitFramebuffer(0, 0, w, h, 0, 0, w, h, GL_COLOR_BUFFER_BIT, GL_NEAREST);
        glGetError() == GL_NO_ERROR
    }
}

fn get_fbo(target: &RenderTarget) -> GLuint {
    let gl = unsafe { get_internal_gl() };
    let rp = target.render_pass.raw_miniquad_id();
    unsafe {
        gl.quad_context.begin_pass(Some(rp), miniquad::PassAction::Nothing);
        let mut fbo: GLuint = 0;
        miniquad::gl::glGetIntegerv(miniquad::gl::GL_FRAMEBUFFER_BINDING, &mut fbo as *mut _ as *mut _);
        gl.quad_context.end_render_pass();
        fbo
    }
}

pub fn internal_id(target: RenderTarget) -> GLuint {
    get_fbo(&target)
}

fn create_render_target_rgb8(width: u32, height: u32, sample_count: i32) -> RenderTarget {
    let gl = unsafe { get_internal_gl() };
    let ctx = gl.quad_context;

    let color_texture = ctx.new_render_texture(miniquad::TextureParams {
        width,
        height,
        format: miniquad::TextureFormat::RGB8,
        sample_count,
        ..Default::default()
    });

    let render_pass = if sample_count > 1 {
        let resolve_texture = ctx.new_render_texture(miniquad::TextureParams {
            width,
            height,
            format: miniquad::TextureFormat::RGB8,
            sample_count: 1,
            ..Default::default()
        });
        ctx.new_render_pass_mrt(&[color_texture], Some(&[resolve_texture]), None)
    } else {
        ctx.new_render_pass(color_texture, None)
    };

    // Get the texture that contains the final result (resolve texture for MSAA, color texture otherwise)
    let result_texture_id = if sample_count > 1 {
        ctx.render_pass_color_attachments(render_pass)[0]
    } else {
        color_texture
    };

    RenderTarget {
        texture: Texture2D::from_miniquad_texture(result_texture_id),
        render_pass: macroquad::texture::RenderPass {
            color_texture: Texture2D::from_miniquad_texture(result_texture_id),
            depth_texture: None,
            render_pass: std::sync::Arc::new(render_pass),
        },
    }
}

impl MSRenderTarget {
    pub fn new(dim: (u32, u32), samples: u32) -> Self {
        let input = create_render_target_rgb8(dim.0, dim.1, samples as i32);
        let output = create_render_target_rgb8(dim.0, dim.1, 1);
        let fbo = get_fbo(&input);
        Self {
            dim,
            fbo,
            input,
            output: [Some(output), None],
        }
    }

    pub fn blit(&self) {
        if let Some(target) = &self.output[0] {
            let dst_fbo = get_fbo(target);
            copy_fbo(self.fbo, dst_fbo, self.dim);
        }
    }

    pub fn swap(&mut self) {
        self.output.swap(0, 1);
        if self.output[0].is_none() {
            self.output[0] = Some(create_render_target_rgb8(self.dim.0, self.dim.1, 1));
        }
    }

    pub fn input(&self) -> RenderTarget {
        self.input.clone()
    }

    pub fn output(&self) -> RenderTarget {
        self.output[0].clone().unwrap()
    }

    pub fn old(&self) -> RenderTarget {
        self.output[1].clone().unwrap()
    }
}

impl Drop for MSRenderTarget {
    fn drop(&mut self) {
        // Render pass and texture cleanup is handled by macroquad's RenderPass Drop impl
    }
}
