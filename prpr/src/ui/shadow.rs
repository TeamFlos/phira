use super::Ui;
use macroquad::prelude::*;
use miniquad::{BlendFactor, BlendState, BlendValue, Equation};
use once_cell::sync::Lazy;

fn alpha_blend_material_params(uniforms: Vec<(String, UniformType)>) -> MaterialParams {
    MaterialParams {
        pipeline_params: PipelineParams {
            color_blend: Some(BlendState::new(
                Equation::Add,
                BlendFactor::Value(BlendValue::SourceAlpha),
                BlendFactor::OneMinusValue(BlendValue::SourceAlpha),
            )),
            ..Default::default()
        },
        uniforms,
        textures: Vec::new(),
    }
}

static SHADOW_MATERIAL: Lazy<Material> =
    Lazy::new(|| load_material(shader::VERTEX, shader::SHADOW_FRAGMENT, alpha_blend_material_params(ShadowConfig::uniforms())).unwrap());

static RR_MATERIAL: Lazy<Material> = Lazy::new(|| {
    load_material(
        shader::VERTEX,
        shader::RR_FRAGMENT,
        alpha_blend_material_params(vec![("rect".to_owned(), UniformType::Float4), ("radius".to_owned(), UniformType::Float1)]),
    )
    .unwrap()
});

static SECTOR_MATERIAL: Lazy<Material> = Lazy::new(|| {
    load_material(
        shader::VERTEX,
        shader::SECTOR_FRAGMENT,
        alpha_blend_material_params(vec![
            ("center".to_owned(), UniformType::Float2),
            ("angle".to_owned(), UniformType::Float2),
            ("blur".to_owned(), UniformType::Float2),
        ]),
    )
    .unwrap()
});

#[derive(Clone, Copy)]
pub struct ShadowConfig {
    pub elevation: f32,
    pub radius: f32,
    pub base: f32,
}
impl Default for ShadowConfig {
    fn default() -> Self {
        Self {
            elevation: 0.005,
            radius: 0.005,
            base: 0.7,
        }
    }
}

impl ShadowConfig {
    pub fn uniforms() -> Vec<(String, UniformType)> {
        vec![
            ("rect".to_owned(), UniformType::Float4),
            ("elevation".to_owned(), UniformType::Float1),
            ("radius".to_owned(), UniformType::Float1),
            ("base".to_owned(), UniformType::Float1),
        ]
    }

    pub fn apply(&self, mat: &Material) {
        mat.set_uniform("elevation", self.elevation);
        mat.set_uniform("radius", self.radius);
        mat.set_uniform("base", self.base);
    }
}

pub fn rounded_rect_shadow(ui: &mut Ui, r: Rect, config: &ShadowConfig) {
    // r.y += elevation * 0.5;
    let mat = *SHADOW_MATERIAL;
    let gr = ui.rect_to_global(r);
    mat.set_uniform("rect", vec4(gr.x, gr.y, gr.right(), gr.bottom()));
    ShadowConfig {
        base: config.base * ui.alpha,
        ..*config
    }
    .apply(&mat);
    gl_use_material(mat);
    let r3 = config.elevation * 3.0;
    draw_rectangle(gr.x - r3, gr.y - r3, gr.w + r3 * 2., gr.h + r3 * 2., WHITE);
    gl_use_default_material();
}

pub fn clip_rounded_rect<R>(ui: &mut Ui, r: Rect, radius: f32, f: impl FnOnce(&mut Ui) -> R) -> R {
    let mat = *RR_MATERIAL;
    let gr = ui.rect_to_global(r);
    mat.set_uniform("rect", vec4(gr.x, gr.y, gr.right(), gr.bottom()));
    mat.set_uniform("radius", radius);
    gl_use_material(mat);
    let res = f(ui);
    gl_use_default_material();
    res
}

pub fn clip_sector<R>(ui: &mut Ui, ct: Vec2, start: f32, end: f32, f: impl FnOnce(&mut Ui) -> R) -> R {
    let mat = *SECTOR_MATERIAL;
    mat.set_uniform("center", ui.to_global((ct.x, ct.y)));
    mat.set_uniform("angle", vec2(start, end));
    let t = -end.sin();
    mat.set_uniform("blur", vec2((ct.y - ui.top) / t, (ct.y + ui.top) / t));
    gl_use_material(mat);
    let res = f(ui);
    gl_use_default_material();
    res
}

mod shader {
    pub const VERTEX: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;

varying lowp vec4 color;
varying highp vec2 pos0;
varying lowp vec2 uv;

uniform mat4 Model;
uniform mat4 Projection;

void main() {
    gl_Position = Projection * Model * vec4(position, 1);
    color = color0 / 255.0;
    pos0 = position.xy;
    uv = texcoord;
}"#;

    pub const SHADOW_FRAGMENT: &str = r#"#version 100
// Adapted from https://madebyevan.com/shaders/fast-rounded-rectangle-shadows/
precision highp float;

varying lowp vec4 color;
varying highp vec2 pos0;

// A standard gaussian function, used for weighting samples
float gaussian(float x, float sigma) {
  const float pi = 3.141592653589793;
  return exp(-(x * x) / (2.0 * sigma * sigma)) / (sqrt(2.0 * pi) * sigma);
}

// This approximates the error function, needed for the gaussian integral
vec2 erf(vec2 x) {
  vec2 s = sign(x), a = abs(x);
  x = 1.0 + (0.278393 + (0.230389 + 0.078108 * (a * a)) * a) * a;
  x *= x;
  return s - s / (x * x);
}

// Return the blurred mask along the x dimension
float roundedBoxShadowX(float x, float y, float sigma, float corner, vec2 halfSize) {
  float delta = min(halfSize.y - corner - abs(y), 0.0);
  float curved = halfSize.x - corner + sqrt(max(0.0, corner * corner - delta * delta));
  vec2 integral = 0.5 + 0.5 * erf((x + vec2(-curved, curved)) * (sqrt(0.5) / sigma));
  return integral.y - integral.x;
}

// Return the mask for the shadow of a box from lower to upper
float roundedBoxShadow(vec2 lower, vec2 upper, vec2 point, float sigma, float corner) {
  vec2 lowerp = lower + vec2(corner);
  vec2 upperp = upper - vec2(corner);
  float lf = step(point.x, lowerp.x);
  float tp = step(point.y, lowerp.y);
  float rt = step(upperp.x, point.x);
  float bt = step(upperp.y, point.y);
  float eps = 0.0003;
  float ein = 0.0007;
  float factor = 1.0 -
      (1.0 - step(corner - eps, distance(lowerp, point)) * lf * tp)
    * (1.0 - step(corner - eps, distance(upperp, point)) * rt * bt)
    * (1.0 - step(corner - eps, distance(vec2(lowerp.x, upperp.y), point)) * lf * bt)
    * (1.0 - step(corner - eps, distance(vec2(upperp.x, lowerp.y), point)) * rt * tp)
    * smoothstep(lower.x, lower.x + ein, point.x)
    * smoothstep(lower.y, lower.y + ein, point.y)
    * smoothstep(point.x, point.x + ein, upper.x)
    * smoothstep(point.y, point.y + ein, upper.y);

  point.y -= sigma * 0.5;
  // Center everything to make the math easier
  vec2 center = (lower + upper) * 0.5;
  vec2 halfSize = (upper - lower) * 0.5;
  point -= center;

  // The signal is only non-zero in a limited range, so don't waste samples
  float low = point.y - halfSize.y;
  float high = point.y + halfSize.y;
  float start = clamp(-3.0 * sigma, low, high);
  float end = clamp(3.0 * sigma, low, high);

  // Accumulate samples (we can get away with surprisingly few samples)
  float s = (end - start) / 4.0;
  float y = start + s * 0.5;
  float value = 0.0;
  for (int i = 0; i < 4; i++) {
    value += roundedBoxShadowX(point.x, point.y - y, sigma, corner, halfSize) * gaussian(y, sigma) * s;
    y += s;
  }

  return value * factor;
}

uniform highp vec4 rect;
uniform highp float elevation;
uniform highp float radius;
uniform highp float base;

void main() {
  gl_FragColor = vec4(0.0, 0.0, 0.0, roundedBoxShadow(rect.xy, rect.zw, pos0, elevation, radius) * base);
}"#;

    pub const RR_FRAGMENT: &str = r#"#version 100
precision highp float;

varying lowp vec4 color;
varying lowp vec2 pos0;
varying lowp vec2 uv;

uniform highp vec4 rect;
uniform highp float radius;

uniform sampler2D Texture;

void main() {
  vec2 lower = rect.xy, upper = rect.zw, point = pos0;
  vec2 lowerp = lower + vec2(radius);
  vec2 upperp = upper - vec2(radius);
  float lf = step(point.x, lowerp.x);
  float tp = step(point.y, lowerp.y);
  float rt = step(upperp.x, point.x);
  float bt = step(upperp.y, point.y);
  float eps = 0.0003;
  float factor =
      (1.0 - step(radius - eps, distance(lowerp, point)) * lf * tp)
    * (1.0 - step(radius - eps, distance(upperp, point)) * rt * bt)
    * (1.0 - step(radius - eps, distance(vec2(lowerp.x, upperp.y), point)) * lf * bt)
    * (1.0 - step(radius - eps, distance(vec2(upperp.x, lowerp.y), point)) * rt * tp)
    * step(lower.x, point.x)
    * step(lower.y, point.y)
    * step(point.x, upper.x)
    * step(point.y, upper.y);
  gl_FragColor = texture2D(Texture, uv) * color;
  gl_FragColor.a *= factor;
}"#;

    pub const SECTOR_FRAGMENT: &str = r#"#version 100
precision highp float;

varying lowp vec4 color;
varying lowp vec2 pos0;
varying lowp vec2 uv;

uniform highp vec2 center;
uniform highp vec2 angle;
uniform highp vec2 blur;

uniform sampler2D Texture;

void main() {
    vec2 delta = pos0.xy - center;
    float cur = atan(delta.y, delta.x);
    float p = clamp((length(delta) - blur.y) / (blur.x - blur.y), 0.0, 1.0);
    p = p * p;
    float blur_range = 0.005 + 0.0 * p;
    float factor = step(angle.x, cur) * smoothstep(angle.y, cur - blur_range * 0.5, cur + blur_range * 0.5) * step(cur, angle.y);
    gl_FragColor = texture2D(Texture, uv);
    gl_FragColor.a *= factor;
}"#;
}
