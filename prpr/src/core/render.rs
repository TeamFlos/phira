use macroquad::{
    texture::{RenderTarget, Texture2D},
    window::get_internal_gl,
};
use miniquad::{gl::GLuint, RenderPass, Texture, TextureFormat};

// TODO: doc
pub struct MSRenderTarget {
    dim: (u32, u32),
    fbo: GLuint,
    rbo: GLuint,
    dummy: RenderTarget,
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

pub fn internal_id(target: RenderTarget) -> GLuint {
    target.render_pass.gl_internal_id(unsafe { get_internal_gl() }.quad_context)
}

impl MSRenderTarget {
    pub fn new(dim: (u32, u32), samples: u32) -> Self {
        let mut fbo = 0;
        let mut rbo = 0;
        unsafe {
            use miniquad::gl::*;
            glGenRenderbuffers(1, &mut rbo as *mut _);
            glBindRenderbuffer(GL_RENDERBUFFER, rbo);
            glRenderbufferStorageMultisample(GL_RENDERBUFFER, samples as _, GL_RGB8, dim.0 as _, dim.1 as _);
            glGenFramebuffers(1, &mut fbo as *mut _);
            glBindFramebuffer(GL_FRAMEBUFFER, fbo);
            glFramebufferRenderbuffer(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_RENDERBUFFER, rbo);
        }
        let gl = unsafe { get_internal_gl() };
        let texture = Texture::new_render_texture(
            gl.quad_context,
            miniquad::TextureParams {
                width: dim.0,
                height: dim.1,
                format: TextureFormat::RGB8,
                ..Default::default()
            },
        );
        let render_pass = RenderPass::new(gl.quad_context, texture, None);
        let dummy_render_pass = RenderPass::from_raw(gl.quad_context, fbo, texture);
        Self {
            dim,
            fbo,
            rbo,
            dummy: RenderTarget {
                texture: Texture2D::from_miniquad_texture(texture),
                render_pass: dummy_render_pass,
            },
            output: [
                Some(RenderTarget {
                    texture: Texture2D::from_miniquad_texture(texture),
                    render_pass,
                }),
                None,
            ],
        }
    }

    pub fn blit(&self) {
        copy_fbo(self.fbo, internal_id(self.output[0].unwrap()), self.dim);
    }

    pub fn swap(&mut self) {
        self.output.swap(0, 1);
        if self.output[0].is_none() {
            let gl = unsafe { get_internal_gl() };
            let texture = miniquad::Texture::new_render_texture(
                gl.quad_context,
                miniquad::TextureParams {
                    width: self.dim.0,
                    height: self.dim.1,
                    format: TextureFormat::RGB8,
                    ..Default::default()
                },
            );
            let render_pass = RenderPass::new(gl.quad_context, texture, None);
            self.output[0] = Some(RenderTarget {
                texture: Texture2D::from_miniquad_texture(texture),
                render_pass,
            });
            copy_fbo(internal_id(self.output[1].unwrap()), internal_id(self.output[0].unwrap()), self.dim);
        }
    }

    pub fn input(&self) -> RenderTarget {
        self.dummy
    }

    pub fn output(&self) -> RenderTarget {
        self.output[0].unwrap()
    }

    pub fn old(&self) -> RenderTarget {
        self.output[1].unwrap()
    }
}

impl Drop for MSRenderTarget {
    fn drop(&mut self) {
        unsafe {
            use miniquad::gl::*;
            glDeleteRenderbuffers(1, &self.rbo as *const _);
            glDeleteFramebuffers(1, &self.fbo as *const _);
        }
        for target in self.output.iter().flatten() {
            target.delete();
        }
    }
}
