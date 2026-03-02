//! Miscellaneous utilities.

use crate::{
    config::Config,
    core::{Matrix, Point, Vector},
    ui::Ui,
};
use anyhow::{anyhow, Result};
use image::DynamicImage;
use lyon::{
    math::Box2D,
    path::{builder::BorderRadii, Path, Winding},
};
use macroquad::prelude::*;
use miniquad::{gl::GLenum, BlendFactor, BlendState, BlendValue, CompareFunc, Equation, PrimitiveType, StencilFaceState, StencilOp, StencilState};
use once_cell::sync::Lazy;
use ordered_float::{FloatCore, NotNan};
use sasa::AudioManager;
use serde::Deserialize;
use std::{
    future::Future,
    ops::Deref,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Poll, RawWaker, RawWakerVTable, Waker},
};
use tracing::{debug, info_span};

pub type LocalTask<R> = Option<Pin<Box<dyn Future<Output = R>>>>;

pub trait JoinToString {
    fn join(self, sep: &str) -> String;
}

impl<V: AsRef<str>, T: Iterator<Item = V>> JoinToString for T {
    fn join(mut self, sep: &str) -> String {
        let mut result = String::new();
        if let Some(first) = self.next() {
            result += first.as_ref();
            for element in self {
                result += sep;
                result += element.as_ref();
            }
        }
        result
    }
}

pub trait NotNanExt: Sized {
    fn not_nan(self) -> NotNan<Self>;
}

impl<T: FloatCore> NotNanExt for T {
    fn not_nan(self) -> NotNan<Self> {
        NotNan::new(self).unwrap()
    }
}

pub trait RectExt: Sized {
    fn feather(&self, radius: f32) -> Self;
    fn nonuniform_feather(&self, x: f32, y: f32) -> Self;
    fn to_euclid(&self) -> Box2D;
    fn rounded(&self, radius: f32) -> Path;
}

impl RectExt for Rect {
    fn feather(&self, radius: f32) -> Self {
        Self::new(self.x - radius, self.y - radius, self.w + radius * 2., self.h + radius * 2.)
    }

    fn nonuniform_feather(&self, x: f32, y: f32) -> Self {
        Self::new(self.x - x, self.y - y, self.w + x * 2., self.h + y * 2.)
    }

    fn to_euclid(&self) -> Box2D {
        Box2D::new(lyon::math::point(self.x, self.y), lyon::math::point(self.right(), self.bottom()))
    }

    fn rounded(&self, radius: f32) -> Path {
        let mut path = Path::builder();
        path.add_rounded_rectangle(&self.to_euclid(), &BorderRadii::new(radius), Winding::Positive);
        path.build()
    }
}

struct SafeTextureInner(Texture2D);
impl Drop for SafeTextureInner {
    fn drop(&mut self) {
        self.0.delete()
    }
}

pub struct SafeTexture(Arc<SafeTextureInner>);
impl SafeTexture {
    pub fn into_inner(self) -> Texture2D {
        let arc = self.0;
        let res = arc.0;
        std::mem::forget(arc);
        res
    }

    pub fn with_mipmap(self) -> Self {
        let id = self.0 .0.raw_miniquad_texture_handle().gl_internal_id();
        unsafe {
            use miniquad::gl::*;
            glBindTexture(GL_TEXTURE_2D, id);
            glGenerateMipmap(GL_TEXTURE_2D);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR_MIPMAP_LINEAR as _);
        }
        self
    }

    pub fn with_filter(self, filter: GLenum) -> Self {
        let id = self.0 .0.raw_miniquad_texture_handle().gl_internal_id();
        unsafe {
            use miniquad::gl::*;
            glBindTexture(GL_TEXTURE_2D, id);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, filter as _);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, filter as _);
        }
        self
    }
}

impl Clone for SafeTexture {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl Deref for SafeTexture {
    type Target = Texture2D;

    fn deref(&self) -> &Self::Target {
        &self.0.as_ref().0
    }
}

impl PartialEq for SafeTexture {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for SafeTexture {}

impl From<Texture2D> for SafeTexture {
    fn from(tex: Texture2D) -> Self {
        Self(Arc::new(SafeTextureInner(tex)))
    }
}

impl From<DynamicImage> for SafeTexture {
    fn from(image: DynamicImage) -> Self {
        Texture2D::from_rgba8(image.width() as _, image.height() as _, &image.into_rgba8()).into()
    }
}

pub static BLACK_TEXTURE: Lazy<SafeTexture> = Lazy::new(|| Texture2D::from_rgba8(1, 1, &[0, 0, 0, 255]).into());

pub fn nalgebra_to_glm(mat: &Matrix) -> Mat4 {
    /*
        [11] [12]  0  [13]
        [21] [22]  0  [23]
          0    0   1    0
        [31] [32]  0  [33]
    */
    Mat4::from_cols_array(&[
        mat.m11, mat.m21, 0., mat.m31, mat.m12, mat.m22, 0., mat.m32, 0., 0., 1., 0., mat.m13, mat.m23, 0., mat.m33,
    ])
}

pub fn get_viewport() -> (i32, i32, i32, i32) {
    let gl = unsafe { get_internal_gl() };
    gl.quad_gl.get_viewport().unwrap_or_else(|| {
        let (w, h) = gl
            .quad_gl
            .get_active_render_pass()
            .map(|it| {
                let tex = it.texture(gl.quad_context);
                (tex.width as i32, tex.height as i32)
            })
            .unwrap_or_else(|| (screen_width() as _, screen_height() as _));
        (0, 0, w, h)
    })
}

#[inline]
pub fn draw_text_aligned(ui: &mut Ui, text: &str, x: f32, y: f32, anchor: (f32, f32), scale: f32, color: Color) -> Rect {
    ui.text(text).pos(x, y).anchor(anchor.0, anchor.1).size(scale).color(color).draw()
}

#[derive(Debug, Default, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScaleType {
    #[default]
    CropCenter,
    Inside,
    Fit,
}

pub fn source_of_image(tex: &Texture2D, rect: Rect, scale_type: ScaleType) -> Option<Rect> {
    match scale_type {
        ScaleType::CropCenter => {
            let exp = rect.w / rect.h;
            let act = tex.width() / tex.height();
            Some(if exp > act {
                let h = act / exp;
                Rect::new(0., 0.5 - h / 2., 1., h)
            } else {
                let w = exp / act;
                Rect::new(0.5 - w / 2., 0., w, 1.)
            })
        }
        ScaleType::Inside => {
            let exp = rect.w / rect.h;
            let act = tex.width() / tex.height();
            Some(if exp > act {
                let w = exp / act;
                Rect::new(0.5 - w / 2., 0., w, 1.)
            } else {
                let h = act / exp;
                Rect::new(0., 0.5 - h / 2., 1., h)
            })
        }
        ScaleType::Fit => None,
    }
}

pub fn draw_image(tex: Texture2D, rect: Rect, scale_type: ScaleType) {
    let source = source_of_image(&tex, rect, scale_type);
    let (w, h) = (tex.width(), tex.height());
    draw_texture_ex(
        tex,
        rect.x,
        rect.y,
        WHITE,
        DrawTextureParams {
            source: source.map(|it| Rect::new(it.x * w, it.y * h, it.w * w, it.h * h)),
            dest_size: Some(rect.size()),
            ..Default::default()
        },
    );
}

pub const PARALLELOGRAM_SLOPE: f32 = 0.13 / (7. / 13.);

pub fn draw_parallelogram(rect: Rect, texture: Option<(Texture2D, Rect)>, color: Color, shadow: bool) {
    draw_parallelogram_ex(rect, texture, color, color, shadow);
}

pub fn draw_parallelogram_ex(rect: Rect, texture: Option<(Texture2D, Rect)>, top: Color, bottom: Color, shadow: bool) {
    let l = rect.h * PARALLELOGRAM_SLOPE;
    let gl = unsafe { get_internal_gl() }.quad_gl;
    let p = [
        Point::new(rect.x + l, rect.y),
        Point::new(rect.right(), rect.y),
        Point::new(rect.x, rect.bottom()),
        Point::new(rect.right() - l, rect.bottom()),
    ];
    let v = if let Some((tex, tex_rect)) = texture {
        let lt = tex_rect.h * tex.height() * PARALLELOGRAM_SLOPE / tex.width();
        gl.texture(Some(tex));
        [
            Vertex::new(p[0].x, p[0].y, 0., tex_rect.x + lt, tex_rect.y, top),
            Vertex::new(p[1].x, p[1].y, 0., tex_rect.right(), tex_rect.y, top),
            Vertex::new(p[2].x, p[2].y, 0., tex_rect.x, tex_rect.bottom(), bottom),
            Vertex::new(p[3].x, p[3].y, 0., tex_rect.right() - lt, tex_rect.bottom(), bottom),
        ]
    } else {
        gl.texture(None);
        [
            Vertex::new(p[0].x, p[0].y, 0., 0., 0., top),
            Vertex::new(p[1].x, p[1].y, 0., 0., 0., top),
            Vertex::new(p[2].x, p[2].y, 0., 0., 0., bottom),
            Vertex::new(p[3].x, p[3].y, 0., 0., 0., bottom),
        ]
    };
    gl.draw_mode(DrawMode::Triangles);
    gl.geometry(&v, &[0, 2, 3, 0, 1, 3]);
    if shadow {
        drop_shadow(p, top.a.min(bottom.a));
    }
}

fn drop_shadow(p: [Point; 4], alpha: f32) {
    const RADIUS: f32 = 0.018;
    let len = (PARALLELOGRAM_SLOPE * PARALLELOGRAM_SLOPE + 1.).sqrt();
    let n1 = Vector::new(PARALLELOGRAM_SLOPE / len - 1., -1. / len) * RADIUS;
    let n2 = Vector::new(n1.x + RADIUS * 2., n1.y);
    let c1 = Color::new(0., 0., 0., alpha * 0.11);
    let c2 = Color::default();
    let v = |p: Point, c: Color| Vertex::new(p.x, p.y, 0., 0., 0., c);
    let p = [
        v(p[0], c1),
        v(p[0] + n1, c2),
        v(p[1], c1),
        v(p[1] + n2, c2),
        v(p[2], c1),
        v(p[2] - n2, c2),
        v(p[3], c1),
        v(p[3] - n1, c2),
    ];
    let gl = unsafe { get_internal_gl() }.quad_gl;
    gl.texture(None);
    gl.draw_mode(DrawMode::Triangles);
    gl.geometry(&p, &[0, 1, 2, 1, 2, 3, 0, 1, 5, 0, 5, 4, 4, 5, 6, 5, 6, 7, 6, 7, 2, 7, 2, 3]);
}

pub fn rect_shadow(r: Rect, radius: f32, alpha: f32) {
    let t = r.feather(radius);
    let v = |x: f32, y: f32, c: Color| Vertex::new(x, y, 0., 0., 0., c);
    let a = Color::new(0., 0., 0., alpha);
    let b = Color::default();
    let p = [
        v(t.x, t.y, b),
        v(t.right(), t.y, b),
        v(r.x, r.y, a),
        v(r.right(), r.y, a),
        v(r.x, r.bottom(), a),
        v(r.right(), r.bottom(), a),
        v(t.x, t.bottom(), b),
        v(t.right(), t.bottom(), b),
    ];
    let gl = unsafe { get_internal_gl() }.quad_gl;
    gl.texture(None);
    gl.draw_mode(DrawMode::Triangles);
    gl.geometry(&p, &[0, 1, 2, 1, 2, 3, 0, 2, 4, 4, 0, 6, 4, 5, 6, 5, 6, 7, 1, 3, 5, 5, 1, 7]);
}

pub fn thread_as_future<R: Send + 'static>(f: impl FnOnce() -> R + Send + 'static) -> impl Future<Output = R> {
    struct DummyFuture<R>(Arc<Mutex<Option<R>>>);
    impl<R> Future for DummyFuture<R> {
        type Output = R;

        fn poll(self: std::pin::Pin<&mut Self>, _: &mut std::task::Context<'_>) -> Poll<Self::Output> {
            match self.0.lock().unwrap().take() {
                Some(res) => Poll::Ready(res),
                None => Poll::Pending,
            }
        }
    }
    let arc = Arc::new(Mutex::new(None));
    std::thread::spawn({
        let arc = Arc::clone(&arc);
        move || {
            let res = f();
            *arc.lock().unwrap() = Some(res);
        }
    });
    DummyFuture(arc)
}

pub async fn spawn_task<R: Send + 'static>(f: impl FnOnce() -> Result<R> + Send + 'static) -> Result<R> {
    #[cfg(target_arch = "wasm32")]
    {
        f()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        tokio::task::spawn_blocking(f).await?
    }
}

pub fn poll_future<R>(future: Pin<&mut (impl Future<Output = R> + ?Sized)>) -> Option<R> {
    fn waker() -> Waker {
        unsafe fn clone(data: *const ()) -> RawWaker {
            RawWaker::new(data, &VTABLE)
        }
        unsafe fn wake(_data: *const ()) {
            // panic!()
        }
        unsafe fn wake_by_ref(data: *const ()) {
            wake(data)
        }
        unsafe fn drop(_data: *const ()) {}
        const VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);
        let raw_waker = RawWaker::new(std::ptr::null(), &VTABLE);
        unsafe { Waker::from_raw(raw_waker) }
    }
    let waker = waker();
    let mut futures_context = std::task::Context::from_waker(&waker);
    match future.poll(&mut futures_context) {
        Poll::Ready(val) => Some(val),
        Poll::Pending => None,
    }
}

pub fn screen_aspect() -> f32 {
    let vp = get_viewport();
    vp.2 as f32 / vp.3 as f32
}

pub fn create_audio_manger(config: &Config) -> Result<AudioManager> {
    #[cfg(target_os = "android")]
    {
        use sasa::backend::oboe::*;
        AudioManager::new(OboeBackend::new(OboeSettings {
            buffer_size: config.audio_buffer_size,
            performance_mode: PerformanceMode::LowLatency,
            usage: Usage::Game,
        }))
    }
    #[cfg(not(target_os = "android"))]
    {
        use sasa::backend::cpal::*;
        AudioManager::new(CpalBackend::new(CpalSettings {
            preferred_sample_rate: config.preferred_sample_rate,
            buffer_size: config.audio_buffer_size,
        }))
    }
}

pub fn make_pipeline(write_color: bool, pass_op: StencilOp, test_func: CompareFunc, test_ref: i32) -> GlPipeline {
    let InternalGlContext {
        quad_gl: gl,
        quad_context: context,
    } = unsafe { get_internal_gl() };
    gl.make_pipeline(
        context,
        shader::VERTEX,
        shader::FRAGMENT,
        PipelineParams {
            color_write: (write_color, write_color, write_color, write_color),
            color_blend: Some(BlendState::new(
                Equation::Add,
                BlendFactor::Value(BlendValue::SourceAlpha),
                BlendFactor::OneMinusValue(BlendValue::SourceAlpha),
            )),
            stencil_test: {
                let state = StencilFaceState {
                    fail_op: StencilOp::Keep,
                    depth_fail_op: StencilOp::Keep,
                    pass_op,
                    test_func,
                    test_ref,
                    test_mask: u32::MAX,
                    write_mask: u32::MAX,
                };
                Some(StencilState { front: state, back: state })
            },
            primitive_type: PrimitiveType::Triangles,
            ..Default::default()
        },
        Vec::new(),
        Vec::new(),
    )
    .unwrap()
}

#[inline]
pub fn semi_black(alpha: f32) -> Color {
    Color::new(0., 0., 0., alpha)
}

#[inline]
pub fn semi_white(alpha: f32) -> Color {
    Color::new(1., 1., 1., alpha)
}

pub fn unzip_into<R: std::io::Read + std::io::Seek>(reader: R, dir: &crate::dir::Dir, strip_root: bool) -> Result<()> {
    let mut zip = zip::ZipArchive::new(reader)?;
    let root = if strip_root {
        if let Some(root) = zip.file_names().min_by_key(|it| it.len()) {
            if root.ends_with('/') && zip.file_names().all(|it| it.starts_with(root)) {
                root.to_owned()
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    } else {
        String::new()
    };
    let _span = info_span!("unzip").entered();
    debug!("root is {root}");
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let path = entry.enclosed_name().ok_or_else(|| anyhow!("invalid zip"))?;
        let path = path.display().to_string();
        debug!("entry: {path}");
        if entry.is_dir() && entry.name() != root {
            if let Some(after) = path.strip_prefix(&root) {
                debug!("mkdir: {after}");
                dir.create_dir_all(after)?;
            }
        } else if entry.is_file() {
            if let Some(after) = path.strip_prefix(&root) {
                if let Some(p) = std::path::Path::new(after).parent() {
                    if !dir.exists(p)? {
                        debug!("mkdir {}", p.display());
                        dir.create_dir_all(p)?;
                    }
                }
                debug!("create {}", after);
                let mut file = dir.create(after)?;
                std::io::copy(&mut entry, &mut file)?;
            }
        }
    }
    Ok(())
}

pub fn parse_time(s: &str) -> Option<f32> {
    if s.is_empty() {
        return None;
    }
    let r = s.split(':').collect::<Vec<_>>();
    if r.len() > 3 {
        return None;
    }
    let mut iter = r.into_iter().rev();
    let mut res = iter.next().unwrap().parse::<f32>().ok()?;
    if res < 0. {
        return None;
    }
    if let Some(mins) = iter.next() {
        res += mins.parse::<u32>().ok()? as f32 * 60.;
    }
    if let Some(hrs) = iter.next() {
        res += hrs.parse::<u32>().ok()? as f32 * 3600.;
    }
    Some(res)
}

pub fn open_url(url: &str) -> Result<()> {
    cfg_if::cfg_if! {
        if #[cfg(target_os = "android")] {
            unsafe {
                let env = miniquad::native::attach_jni_env();
                let ctx = ndk_context::android_context().context();
                let class = (**env).GetObjectClass.unwrap()(env, ctx);
                let method =
                    (**env).GetMethodID.unwrap()(env, class, b"openUrl\0".as_ptr() as _, b"(Ljava/lang/String;)V\0".as_ptr() as _);
                let url = std::ffi::CString::new(url.to_owned()).unwrap();
                (**env).CallVoidMethod.unwrap()(
                    env,
                    ctx,
                    method,
                    (**env).NewStringUTF.unwrap()(env, url.as_ptr()),
                );
            }
        } else if #[cfg(target_os = "ios")] {
            unsafe {
                use crate::objc::*;

                let application: ObjcId = msg_send![class!(UIApplication), sharedApplication];
                let url: ObjcId = msg_send![class!(NSURL), URLWithString: str_to_ns(url)];
                let _: () = msg_send![application, openURL: url];
            }
        } else {
            open::that(url)?;
        }
    }

    Ok(())
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
varying lowp vec4 color;
varying lowp vec2 uv;

uniform sampler2D Texture;

void main() {
    gl_FragColor = color * texture2D(Texture, uv);
}"#;
}
