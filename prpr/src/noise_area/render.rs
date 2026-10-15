//! GLES2 adaptation of the extracted BlockRender passes.
use super::{perf::Profiler, BlockArea, NoiseAreaState};
use crate::core::Resource;
use anyhow::{ensure, Result};
use macroquad::prelude::*;
use miniquad::{BlendFactor, BlendState, BlendValue, Equation, PipelineParams, UniformType};
use std::collections::{HashMap, HashSet};
const VERT: &str = r#"#version 100
attribute vec3 position; attribute vec2 texcoord; attribute vec4 color0;
uniform mat4 Model; uniform mat4 Projection;
varying vec2 uv; varying vec4 col;
void main(){gl_Position=Projection*Model*vec4(position,1.);uv=texcoord;col=color0/255.;}"#;
const SPRITE: &str = r#"#version 100
precision mediump float; varying vec2 uv; varying vec4 col; uniform sampler2D Texture;
void main(){gl_FragColor=texture2D(Texture,uv)*col;}"#;
const PACK_MASK: &str = r#"#version 100
precision highp float; varying vec2 uv; uniform sampler2D Active; uniform sampler2D Ready; uniform sampler2D Normal; uniform sampler2D Subtract;
void main(){gl_FragColor=vec4(texture2D(Active,uv).r,texture2D(Ready,uv).r,texture2D(Normal,uv).r,texture2D(Subtract,uv).g);}"#;
const PACK_EFFECT: &str = r#"#version 100
precision highp float; varying vec2 uv; uniform sampler2D Edge; uniform sampler2D Glow; uniform sampler2D Hover;
void main(){gl_FragColor=vec4(texture2D(Edge,uv).r,texture2D(Glow,uv).g,texture2D(Hover,uv).r,1.);}"#;
const PRECISE: &str = r#"#version 100
precision highp float; varying vec2 uv; uniform sampler2D Normal; uniform sampler2D Subtract; uniform vec2 SourceSize; uniform vec2 TargetSize;
void main(){float v=0.; for(int y=0;y<4;y++){for(int x=0;x<4;x++){vec2 p=uv+(vec2(float(x),float(y))/4.+0.125-0.5)/TargetSize;float a=texture2D(Normal,p).r;float b=texture2D(Subtract,p).r;v+=abs(a-(step(0.09,b)-step(0.12,b)));}}gl_FragColor=vec4(v/16.,0.,0.,1.);}"#;
fn precise_variant(samples: u32) -> String {
    PRECISE
        .replace("y<4", &format!("y<{samples}"))
        .replace("x<4", &format!("x<{samples}"))
        .replace("/4.+0.125", &format!("/{samples}.+{}", 0.5 / samples as f32))
        .replace("v/16.", &format!("v/{}.", samples * samples))
}
fn supersampling(w: u32, h: u32) -> u32 {
    if w.max(h) <= 1024 && w as u64 * h as u64 * 16 <= 8_388_608 {
        4
    } else if w.max(h) <= 2048 && w as u64 * h as u64 * 4 <= 8_388_608 {
        2
    } else {
        1
    }
}
// Ready/disabled composition has no displacement sampler. Keep its
// binding list distinct from the active composition shader.
fn compose_textures<T: Copy>(normal: T, subtract: T, displacement: Option<T>) -> ([(&'static str, T); 3], usize) {
    if let Some(displacement) = displacement {
        ([("_DisplaceMap", displacement), ("_NormalBlockRT", normal), ("_SubtractBlockRT", subtract)], 3)
    } else {
        ([("_DisabledNormalBlockRT", normal), ("_DisabledSubtractBlockRT", subtract), ("", normal)], 2)
    }
}
// Specialize only uniform conditions; coordinates, noise and blend math remain unchanged.
fn active_variant(no_touch: bool, no_distortion: bool, low: bool, ready_only: bool) -> String {
    // Keep the embedded source newline agnostic: the repository stores LF, but a
    // Windows checkout with `core.autocrlf` hands us CRLF.
    let shader = include_str!("shaders/ActiveBlock_program_0.glsl").replace("\r\n", "\n");
    let (declarations, body) = shader.split_once("void main()").unwrap();
    let mut body = body.to_owned();
    if no_touch {
        body = body
            .replace("_TouchPosCount", "0")
            .replace("texture2D(_EffectRT, (floor(vs_TEXCOORD0.xy * _HoverSize) + 0.5) / _HoverSize).b", "0.0");
    }
    if no_distortion {
        body = body.replace("mix(u_xlat16.xy, vs_TEXCOORD0.xy, _RemoveDistortion)", "vs_TEXCOORD0.xy");
    }
    if ready_only {
        body = body
            .replace("maskChannels.r", "0.0")
            .replace("texture2D(_EffectRT, u_xlat16.xy).x", "0.0")
            .replace("texture2D(_EffectRT, vs_TEXCOORD0.xy).y", "0.0");
    }
    if low && !no_touch {
        let begin = body.find("    if(u_xlatb0.x){\n        u_xlat0.x = dot(_TouchDisplaceDirection").unwrap();
        let end = body[begin..].find("    } else {\n        u_xlat16_8.x = float(0.0);").unwrap() + begin;
        body.replace_range(begin..end, "    if(u_xlatb0.x){\n        u_xlat16_8.xyz = clamp(u_xlat48, 0.0, 1.0) * _TouchGlowColor.xyz;\n");
    }
    format!("{declarations}void main(){body}")
}

/// Conservative UV bounds. Margin covers mask displacement, five glow rings,
/// filtering and touch displacement. Nonfinite geometry uses the full screen.
#[derive(Clone, Copy, Debug)]
struct Coverage {
    min: Vec2,
    max: Vec2,
}
impl Coverage {
    fn new() -> Self {
        Self {
            min: Vec2::splat(f32::INFINITY),
            max: Vec2::splat(f32::NEG_INFINITY),
        }
    }
    fn add(&mut self, point: Vec2) {
        if !point.is_finite() {
            self.min = Vec2::ZERO;
            self.max = Vec2::ONE;
            return;
        }
        self.min = self.min.min(point);
        self.max = self.max.max(point);
    }
    fn clip(self, w: u32, h: u32, margin: Vec2) -> Option<(i32, i32, i32, i32)> {
        if self.min.x > self.max.x || self.min.y > self.max.y {
            return None;
        }
        let min = (self.min - margin).clamp(Vec2::ZERO, Vec2::ONE);
        let max = (self.max + margin).clamp(Vec2::ZERO, Vec2::ONE);
        let x = (min.x * w as f32).floor() as i32;
        let y = ((1. - max.y) * h as f32).floor() as i32;
        let right = (max.x * w as f32).ceil() as i32;
        let bottom = ((1. - min.y) * h as f32).ceil() as i32;
        Some((x, y, (right - x).max(0), (bottom - y).max(0)))
    }
}
#[derive(Clone, Copy, PartialEq)]
struct ScreenQuad {
    points: [Vec2; 4],
    color: Color,
}
const TOUCH_UNIFORMS: [&str; 10] = [
    "_TouchPos0",
    "_TouchPos1",
    "_TouchPos2",
    "_TouchPos3",
    "_TouchPos4",
    "_TouchPos5",
    "_TouchPos6",
    "_TouchPos7",
    "_TouchPos8",
    "_TouchPos9",
];
/// `_BlockTime` drives every scrolling, shimmering and edge-jittering effect.
/// Freezing it keeps the block edges perfectly still while the geometry and the
/// appear/enable/disable fades keep working.
fn block_time(t: f32, no_jitter: bool) -> Vec4 {
    if no_jitter {
        Vec4::ZERO
    } else {
        vec4(t / 20., t, t * 2., t * 3.)
    }
}
struct Program {
    material: Material,
    uniforms: HashMap<String, UniformType>,
    textures: HashSet<String>,
}
impl Program {
    fn new(vert: &str, frag: &str, blend: Option<BlendState>) -> Result<Self> {
        let regex = regex::Regex::new(r"uniform\s+(?:(?:highp|mediump|lowp)\s+)?(float|int|vec2|vec3|vec4|sampler2D)\s+(\w+)\s*;")?;
        let mut uniforms = HashMap::new();
        let mut textures = HashSet::new();
        for caps in regex.captures_iter(&format!("{vert}\n{frag}")) {
            let n = caps[2].to_owned();
            if &caps[1] == "sampler2D" {
                if n != "Texture" {
                    textures.insert(n);
                }
                continue;
            }
            let ty = match &caps[1] {
                "float" => UniformType::Float1,
                "int" => UniformType::Int1,
                "vec2" => UniformType::Float2,
                "vec3" => UniformType::Float3,
                _ => UniformType::Float4,
            };
            uniforms.insert(n, ty);
        }
        let label = frag
            .lines()
            .find(|line| line.contains("uniform") && line.contains("sampler2D"))
            .unwrap_or("sprite");
        let started = std::time::Instant::now();
        tracing::info!(stage = "shader_begin", shader = label, "noise-area initialization");
        let material = load_material(
            vert,
            frag,
            MaterialParams {
                uniforms: uniforms.iter().map(|(n, t)| (n.clone(), *t)).collect(),
                textures: textures.iter().cloned().collect(),
                pipeline_params: PipelineParams {
                    color_blend: blend,
                    ..Default::default()
                },
            },
        )?;
        tracing::info!(stage = "shader_ready", shader = label, elapsed_ms = started.elapsed().as_millis(), "noise-area initialization");
        Ok(Self {
            material,
            uniforms,
            textures,
        })
    }
    fn f(&self, n: &str, v: f32) {
        if self.uniforms.contains_key(n) {
            self.material.set_uniform(n, v)
        }
    }
    fn v2(&self, n: &str, v: Vec2) {
        if self.uniforms.contains_key(n) {
            self.material.set_uniform(n, v)
        }
    }
    fn v4(&self, n: &str, v: Vec4) {
        if self.uniforms.contains_key(n) {
            self.material.set_uniform(n, v)
        }
    }
    fn props(&self, values: &serde_json::Value) {
        if let Some(map) = values.as_object() {
            for (n, v) in map {
                if let Some(t) = self.uniforms.get(n) {
                    if let Some(f) = v.as_f64() {
                        self.f(n, f as f32)
                    } else if let Some(a) = v.as_array() {
                        let a: Vec<_> = a.iter().map(|v| v.as_f64().unwrap_or(0.) as f32).collect();
                        match t {
                            UniformType::Float2 => self.v2(n, vec2(a[0], a[1])),
                            UniformType::Float3 => self.material.set_uniform(n, vec3(a[0], a[1], a[2])),
                            UniformType::Float4 => self.v4(n, vec4(a[0], a[1], a[2], a[3])),
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}
impl Drop for Program {
    fn drop(&mut self) {
        self.material.delete();
    }
}
struct Targets {
    w: u32,
    h: u32,
    targets: Vec<RenderTarget>,
}
impl Targets {
    fn new(w: u32, h: u32, precise: bool, low: bool) -> Self {
        let divisor = if low { 12 } else { 8 };
        let (bw, bh) = ((w / divisor).max(1), (h / divisor).max(1));
        let (ew, eh) = if precise { (w, h) } else { (bw * 2, bh * 2) };
        let ss = supersampling(w, h);
        // 0/1 active masks, 2/3 ready, 4/5 disabled, 6 subtract active,
        // 7 subtract ready, 8 compose active, 9 compose ready, 10 disabled,
        // 11 edge, 12/13 glow ping-pong, 14 hover, 15 packed masks,
        // 16 packed effects, 17 scene color, 18/19 supersample geometry.
        let sizes = [
            (bw, bh),
            (bw, bh),
            (bw, bh),
            (bw, bh),
            (bw, bh),
            (bw, bh),
            (bw, bh),
            (bw, bh),
            if precise { (w, h) } else { (bw, bh) },
            (bw, bh),
            (bw, bh),
            (ew, eh),
            if low && precise { ((w / 2).max(1), (h / 2).max(1)) } else { (ew, eh) },
            if low && precise { ((w / 2).max(1), (h / 2).max(1)) } else { (ew, eh) },
            (bw, bh),
            if precise { (w, h) } else { (bw, bh) },
            (ew, eh),
            ((w / if low { 10 } else { 6 }).max(1), (h / if low { 10 } else { 6 }).max(1)),
            if precise { (w * ss, h * ss) } else { (1, 1) },
            if precise { (w * ss, h * ss) } else { (1, 1) },
            (bw, bh),
        ];
        let targets = sizes
            .into_iter()
            .enumerate()
            .map(|(i, (x, y))| {
                let r = render_target(x, y);
                r.texture.set_filter(if i == 16 { FilterMode::Linear } else { FilterMode::Nearest });
                r
            })
            .collect();
        Self { w, h, targets }
    }
}
impl Drop for Targets {
    fn drop(&mut self) {
        let mut gl = unsafe { get_internal_gl() };
        gl.flush();
        for t in &self.targets {
            t.render_pass.delete(gl.quad_context);
        }
    }
}
/// Simple mode uses only the engine's ordinary colored-triangle renderer.
/// Settings are selected before loading a chart, so its effect pipeline is optional.
pub struct NoiseRenderer {
    effects: Option<EffectRenderer>,
    basic_profiler: Option<Profiler>,
}
impl NoiseRenderer {
    pub async fn new(low: bool) -> Result<Self> {
        Ok(Self {
            effects: if low { None } else { Some(EffectRenderer::new().await?) },
            basic_profiler: if low { Some(Profiler::new()) } else { None },
        })
    }
    pub fn render(&mut self, res: &mut Resource, areas: &[BlockArea], state: &NoiseAreaState, full_vp: (i32, i32, i32, i32)) -> Result<()> {
        if !res.config.noise_area.enabled {
            return Ok(());
        }
        if !res.config.noise_area.low_performance {
            if let Some(effects) = &mut self.effects {
                return effects.render(res, areas, state, full_vp);
            }
        }
        if !areas.iter().any(|a| a.visible(res.time as f32)) {
            return Ok(());
        }
        let Some(target) = res.chart_target.as_ref() else {
            return Ok(());
        };
        let output = target.output();
        let (w, h) = (output.texture.width(), output.texture.height());
        let cv = res.camera.viewport.unwrap_or(full_vp);
        let vp = (cv.0 - full_vp.0, cv.1 - full_vp.1, cv.2, cv.3);
        let profiler = self.basic_profiler.get_or_insert_with(Profiler::new);
        profiler.frame(true, false, w as u32, h as u32);
        let timing = profiler.begin(0);
        let t = res.time as f32;
        let mut normal: [Vec<([Vec2; 4], f32)>; 3] = std::array::from_fn(|_| Vec::new());
        let mut holes: [Vec<[Vec2; 4]>; 3] = std::array::from_fn(|_| Vec::new());
        let mut adjusted_normal = Vec::new();
        let mut adjusted_holes = Vec::new();
        for area in areas.iter().filter(|a| a.visible(t)) {
            let phase = if area.active(t) {
                0
            } else if area.ready(t) {
                1
            } else {
                2
            };
            let pose = area.pose(t, res.aspect_ratio);
            // Pose::contains rejects both axes if either is degenerate.
            if pose.size.x.abs() < 0.0001 || pose.size.y.abs() < 0.0001 {
                continue;
            }
            let points = pose.corners().map(|p| EffectRenderer::screen_uv(p, res, vp, w, h));
            if !points.iter().all(|p| p.is_finite()) {
                continue;
            }
            let fade = if phase == 2 { area.fade(t) } else { 1. };
            if phase == 0 {
                let adjusted = adjusted_pose(pose, area.is_subtract);
                let points = adjusted.corners().map(|p| EffectRenderer::screen_uv(p, res, vp, w, h));
                if points.iter().all(|p| p.is_finite()) {
                    if area.is_subtract {
                        adjusted_holes.push(points);
                    } else {
                        adjusted_normal.push(points);
                    }
                }
            }
            if area.is_subtract {
                holes[phase].push(points);
            } else {
                normal[phase].push((points, fade));
            }
        }
        let mut pieces = Vec::new();
        let screen = [vec2(0., 0.), vec2(1., 0.), vec2(1., 1.), vec2(0., 1.)];
        // Assign overlapping pixels once, prioritizing real blocking over previews.
        let mut painted = Vec::new();
        let mut blocking_shape = Vec::new();
        for phase in [0, 1, 2] {
            let normals: Vec<_> = normal[phase].iter().map(|(p, _)| *p).collect();
            let mut fragments = mask_polygons(&normals, &holes[phase], &screen);
            let mut boundaries: Vec<_> = normals.iter().chain(&holes[phase]).copied().collect();
            if phase == 0 {
                // Exactly (normal union XOR subtract parity) AND its adjusted
                // counterpart, matching blocked() rather than ordinary subtraction.
                let adjusted = mask_polygons(&adjusted_normal, &adjusted_holes, &screen);
                fragments = intersect_sets(&fragments, &adjusted);
                blocking_shape.clone_from(&fragments);
                boundaries.extend(adjusted_normal.iter().chain(&adjusted_holes).copied());
            }
            fragments = difference(fragments, &painted);
            painted.extend(fragments.iter().cloned());
            for poly in fragments {
                let center = poly.iter().copied().sum::<Vec2>() / poly.len() as f32;
                let fade = if phase == 0 {
                    1.
                } else {
                    normal[phase]
                        .iter()
                        .filter(|(p, _)| inside_convex(center, p))
                        .map(|(_, f)| *f)
                        .reduce(f32::max)
                        .unwrap_or(1.)
                };
                pieces.push((poly, fade, phase == 0, boundaries.clone()));
            }
        }
        profiler.end(timing);
        unsafe { get_internal_gl() }.flush();
        push_camera_state();
        EffectRenderer::camera(output);
        unsafe { get_internal_gl() }.quad_gl.scissor(None);
        gl_use_default_material();
        let timing = profiler.begin(11);
        for (poly, fade, blocking, boundaries) in pieces {
            let vertices: Vec<_> = poly
                .iter()
                .map(|p| macroquad::models::Vertex {
                    position: vec3(p.x * 2. - 1., p.y * 2. - 1., 0.),
                    uv: Vec2::ZERO,
                    color: Color::new(1., 0., 0., (if blocking { 0.40 } else { 0.05 }) * fade),
                })
                .collect();
            let indices: Vec<u16> = (1..poly.len().saturating_sub(1)).flat_map(|i| [0, i as u16, (i + 1) as u16]).collect();
            let gl = unsafe { get_internal_gl() }.quad_gl;
            gl.texture(None);
            gl.draw_mode(DrawMode::Triangles);
            gl.geometry(&vertices, &indices);
            if !blocking {
                continue;
            }
            for i in 0..poly.len() {
                let a = poly[i];
                let b = poly[(i + 1) % poly.len()];
                if !boundaries
                    .iter()
                    .any(|quad| (0..4).any(|i| on_segment(a, quad[i], quad[(i + 1) % 4]) && on_segment(b, quad[i], quad[(i + 1) % 4])))
                {
                    continue;
                }
                let delta = (b - a) * vec2(w, h);
                if delta.length_squared() < 1e-8 {
                    continue;
                }
                let n = vec2(-delta.y, delta.x).normalize() / vec2(w, h);
                let midpoint = (a + b) * 0.5;
                let side_a = blocking_shape.iter().any(|p| inside_convex(midpoint + n * 0.5, p));
                let side_b = blocking_shape.iter().any(|p| inside_convex(midpoint - n * 0.5, p));
                if side_a == side_b {
                    continue;
                } // Never outline internal partitions/overlaps.
                EffectRenderer::mesh([a - n, b - n, b + n, a + n], Color::new(1., 0., 0., 0.8 * fade), None);
            }
        }
        unsafe { get_internal_gl() }.flush();
        profiler.end(timing);
        pop_camera_state();
        let gl = unsafe { get_internal_gl() };
        gl.quad_gl.render_pass(Some(output.render_pass));
        gl.quad_gl.viewport(Some(vp));
        Ok(())
    }
}
fn adjusted_pose(pose: super::Pose, subtract: bool) -> super::Pose {
    let inset = vec2((0.3 / pose.size.x.abs()).min(0.25), (0.3 / pose.size.y.abs()).min(0.25));
    let sign = if subtract { 1. } else { -1. };
    super::Pose {
        size: pose.size * (Vec2::ONE + inset * (2. * sign)),
        ..pose
    }
}
fn on_segment(p: Vec2, a: Vec2, b: Vec2) -> bool {
    let d = b - a;
    let len2 = d.length_squared();
    len2 > 1e-12 && d.perp_dot(p - a).abs() <= 1e-6 * d.length() && (p - a).dot(d) >= -1e-6 && (p - a).dot(d) <= len2 + 1e-6
}
/// Partition a convex polygon outside a convex hole, preserving real holes
/// without stencil buffers, texture masks or fragment effects.
fn subtract_polygon(mut inside: Vec<Vec2>, hole: &[Vec2]) -> Vec<Vec<Vec2>> {
    if !bounds_overlap(&inside, hole) {
        return vec![inside];
    }
    let signed_area = polygon_area(hole) * 2.;
    if signed_area.abs() < 1e-10 {
        return vec![inside];
    }
    let sign = signed_area.signum();
    let mut outside = Vec::new();
    for i in 0..hole.len() {
        if inside.len() < 3 {
            break;
        }
        let a = hole[i];
        let edge = hole[(i + 1) % hole.len()] - a;
        let mut kept = Vec::new();
        let mut removed = Vec::new();
        for j in 0..inside.len() {
            let p = inside[j];
            let q = inside[(j + 1) % inside.len()];
            let dp = sign * edge.perp_dot(p - a);
            let dq = sign * edge.perp_dot(q - a);
            if dp >= 0. {
                kept.push(p);
            } else {
                removed.push(p);
            }
            if (dp >= 0.) != (dq >= 0.) {
                let cross = p + (q - p) * (dp / (dp - dq));
                kept.push(cross);
                removed.push(cross);
            }
        }
        if removed.len() >= 3 {
            let removed = clean_polygon(removed);
            if removed.len() >= 3 {
                outside.push(removed);
            }
        }
        inside = clean_polygon(kept);
    }
    outside
}
/// All set members are convex and have disjoint interiors. This also prevents
/// repeated alpha blending where normal areas overlap.
fn inside_convex(p: Vec2, poly: &[Vec2]) -> bool {
    if poly.len() < 3 {
        return false;
    }
    let sign = polygon_area(poly).signum();
    sign != 0.
        && (0..poly.len())
            .all(|i| sign * (poly[(i + 1) % poly.len()] - poly[i]).perp_dot(p - poly[i]) >= -1e-7 * (poly[(i + 1) % poly.len()] - poly[i]).length())
}
fn bounds_overlap(a: &[Vec2], b: &[Vec2]) -> bool {
    let bounds = |p: &[Vec2]| {
        p.iter()
            .fold((Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)), |(min, max), p| (min.min(*p), max.max(*p)))
    };
    let (amin, amax) = bounds(a);
    let (bmin, bmax) = bounds(b);
    amin.x < bmax.x && bmin.x < amax.x && amin.y < bmax.y && bmin.y < amax.y
}
fn clean_polygon(poly: Vec<Vec2>) -> Vec<Vec2> {
    // Repeated clipping can create almost-identical vertices with a reversed
    // microscopic edge. Remove them before they become another clipping plane.
    let mut result: Vec<Vec2> = Vec::new();
    for p in poly {
        if result.last().is_none_or(|q| (*q - p).length_squared() > 1e-12) {
            result.push(p);
        }
    }
    if result.len() > 1 && (result[0] - result[result.len() - 1]).length_squared() <= 1e-12 {
        result.pop();
    }
    result
}
fn polygon_area(poly: &[Vec2]) -> f32 {
    ((0..poly.len())
        .map(|i| {
            let a = poly[i];
            let b = poly[(i + 1) % poly.len()];
            a.x as f64 * b.y as f64 - a.y as f64 * b.x as f64
        })
        .sum::<f64>()
        * 0.5) as f32
}
fn intersection(mut poly: Vec<Vec2>, clip: &[Vec2]) -> Vec<Vec2> {
    if clip.len() < 3 || polygon_area(clip).abs() < 1e-10 || !bounds_overlap(&poly, clip) {
        return Vec::new();
    }
    let sign = polygon_area(clip).signum();
    for i in 0..clip.len() {
        if poly.len() < 3 {
            return Vec::new();
        }
        let a = clip[i];
        let edge = clip[(i + 1) % clip.len()] - a;
        let mut next = Vec::new();
        for j in 0..poly.len() {
            let p = poly[j];
            let q = poly[(j + 1) % poly.len()];
            let dp = sign * edge.perp_dot(p - a);
            let dq = sign * edge.perp_dot(q - a);
            if dp >= 0. {
                next.push(p);
            }
            if (dp >= 0.) != (dq >= 0.) {
                next.push(p + (q - p) * (dp / (dp - dq)));
            }
        }
        poly = clean_polygon(next);
    }
    if polygon_area(&poly).abs() > 1e-10 {
        poly
    } else {
        Vec::new()
    }
}
fn difference(mut polys: Vec<Vec<Vec2>>, cuts: &[Vec<Vec2>]) -> Vec<Vec<Vec2>> {
    for cut in cuts {
        polys = polys
            .into_iter()
            .flat_map(|p| subtract_polygon(p, cut))
            .filter(|p| polygon_area(p).abs() > 1e-10)
            .collect();
    }
    polys
}
fn intersect_sets(a: &[Vec<Vec2>], b: &[Vec<Vec2>]) -> Vec<Vec<Vec2>> {
    a.iter()
        .flat_map(|p| b.iter().map(move |q| intersection(p.clone(), q)))
        .filter(|p| p.len() >= 3)
        .collect()
}
fn mask_polygons(normals: &[[Vec2; 4]], subtracts: &[[Vec2; 4]], screen: &[Vec2; 4]) -> Vec<Vec<Vec2>> {
    let mut union = Vec::new();
    for quad in normals {
        let p = intersection(quad.to_vec(), screen);
        if p.is_empty() {
            continue;
        }
        union.extend(difference(vec![p], &union));
    }
    let mut parity = Vec::new();
    for quad in subtracts {
        let p = intersection(quad.to_vec(), screen);
        if p.is_empty() {
            continue;
        }
        let added = difference(vec![p.clone()], &parity);
        parity = difference(parity, &[p]);
        parity.extend(added);
    }
    let mut xor = difference(union.clone(), &parity);
    xor.extend(difference(parity, &union));
    xor
}
struct EffectRenderer {
    programs: Vec<Program>,
    sprite: Program,
    hover_sprite: Program,
    mask: Program,
    effect: Program,
    precise: Vec<Program>,
    targets: Option<Targets>,
    precise_enabled: bool,
    precise_cache_valid: bool,
    precise_geometry: [Vec<ScreenQuad>; 2],
    low_enabled: bool,
    variants: Vec<Program>,
    fused_effect: Program,
    profiler: Profiler,
    noise: Texture2D,
    spark: Texture2D,
    grain: Texture2D,
    touch: Texture2D,
    blank: Texture2D,
    geometry: [Vec<ScreenQuad>; 6],
}
impl EffectRenderer {
    pub async fn new() -> Result<Self> {
        let blend = Some(BlendState::new(Equation::Add, BlendFactor::One, BlendFactor::OneMinusValue(BlendValue::SourceAlpha)));
        macro_rules! program {
            ($n:literal) => {{
                // LoadingScene polls once per visible frame; do not compile every
                // effect in one poll and leave the loading image unresponsive.
                let mut yielded = false;
                std::future::poll_fn(|cx| {
                    if std::mem::replace(&mut yielded, true) {
                        std::task::Poll::Ready(())
                    } else {
                        cx.waker().wake_by_ref();
                        std::task::Poll::Pending
                    }
                })
                .await;
                tracing::info!(stage = "program_begin", program = $n, "noise-area initialization");
                Program::new(include_str!(concat!("shaders/", $n, ".vert")), include_str!(concat!("shaders/", $n, ".glsl")), blend)?
            }};
        }
        tracing::info!(stage = "textures_begin", "noise-area initialization");
        let noise = Texture2D::from_file_with_format(include_bytes!("assets/texture_30.png"), Some(ImageFormat::Png));
        let spark = Texture2D::from_file_with_format(include_bytes!("assets/texture_17.png"), Some(ImageFormat::Png));
        let grain = Texture2D::from_file_with_format(include_bytes!("assets/texture_15.png"), Some(ImageFormat::Png));
        let touch = Texture2D::from_file_with_format(include_bytes!("assets/touch.png"), Some(ImageFormat::Png));
        for tex in [noise, spark, grain] {
            tex.set_filter(FilterMode::Linear);
            tex.raw_miniquad_texture_handle()
                .set_wrap(unsafe { get_internal_gl() }.quad_context, miniquad::TextureWrap::Repeat);
        }
        touch.set_filter(FilterMode::Linear);
        tracing::info!(stage = "textures_ready", "noise-area initialization");
        let mut renderer = Self {
            programs: vec![
                program!("SubtractBlockBlender_program_0"),
                program!("SubtractBlockBlender_program_1"),
                program!("BlockCompose_program_0"),
                program!("BlockCompose_program_1"),
                program!("EdgeMask_program_1"),
                program!("GlowMask_program_0"),
                // Unity DisabledBlock uses additive blending (One, One).
                // Premultiplied-over would erase the chart because its alpha is 1.
                Program::new(
                    include_str!("shaders/DisabledBlock_program_0.vert"),
                    include_str!("shaders/DisabledBlock_program_0.glsl"),
                    Some(BlendState::new(Equation::Add, BlendFactor::One, BlendFactor::One)),
                )?,
                program!("ActiveBlock_program_0"),
            ],
            sprite: Program::new(VERT, SPRITE, Some(BlendState::new(Equation::Add, BlendFactor::Value(BlendValue::SourceAlpha), BlendFactor::One)))?,
            hover_sprite: Program::new(VERT, &SPRITE.replace("texture2D(Texture,uv)*col", "vec4(texture2D(Texture,uv).a) * col"), blend)?,
            mask: Program::new(VERT, PACK_MASK, None)?,
            effect: Program::new(VERT, PACK_EFFECT, None)?,
            precise: [1, 2, 4]
                .into_iter()
                .map(|n| Program::new(VERT, &precise_variant(n), None))
                .collect::<Result<_>>()?,
            targets: None,
            precise_enabled: false,
            precise_cache_valid: false,
            precise_geometry: std::array::from_fn(|_| Vec::new()),
            low_enabled: false,
            variants: Vec::new(),
            fused_effect: Program::new(VERT, &PACK_EFFECT.replace("texture2D(Edge,uv).r", "texture2D(Glow,uv).b"), None)?,
            profiler: Profiler::new(),
            noise,
            spark,
            grain,
            touch,
            blank: Texture2D::from_rgba8(1, 1, &[0, 0, 0, 0]),
            geometry: std::array::from_fn(|_| Vec::new()),
        };
        // Material properties do not vary during a play session.
        let props: serde_json::Value = serde_json::from_str(include_str!("assets/materials.json"))?;
        renderer.programs[2].props(&props["6"]);
        renderer.programs[3].props(&props["6"]);
        renderer.programs[6].props(&props["8"]);
        renderer.programs[7].props(&props["5"]);
        for (no_touch, no_distortion, low, ready_only) in [
            (true, false, false, false),
            (false, true, false, false),
            (true, true, false, false),
            (true, true, false, true),
        ] {
            let mut yielded = false;
            std::future::poll_fn(|cx| {
                if std::mem::replace(&mut yielded, true) {
                    std::task::Poll::Ready(())
                } else {
                    cx.waker().wake_by_ref();
                    std::task::Poll::Pending
                }
            })
            .await;
            let shader = active_variant(no_touch, no_distortion, low, ready_only);
            let pr = Program::new(include_str!("shaders/ActiveBlock_program_0.vert"), &shader, blend)?;
            pr.props(&props["5"]);
            renderer.variants.push(pr);
        }
        Ok(renderer)
    }
    fn camera(target: RenderTarget) {
        set_camera(&Camera2D {
            zoom: vec2(1., 1.),
            render_target: Some(target),
            ..Default::default()
        });
    }
    fn quad() {
        Self::mesh([vec2(0., 0.), vec2(1., 0.), vec2(1., 1.), vec2(0., 1.)], WHITE, None);
    }
    fn pass(
        program: &Program,
        target: RenderTarget,
        textures: &[(&str, Texture2D)],
        clear: bool,
        profiler: &mut Profiler,
        channel: usize,
        clip: Option<(i32, i32, i32, i32)>,
    ) -> Result<()> {
        for (name, _) in textures {
            ensure!(program.textures.contains(*name), "Noise-area shader has no texture sampler {name}");
        }
        let timing = profiler.begin(channel);
        Self::camera(target);
        if clear {
            clear_background(BLANK);
        }
        for (n, t) in textures {
            program.material.set_texture(n, *t)
        }
        unsafe { get_internal_gl() }.quad_gl.scissor(clip);
        gl_use_material(program.material);
        Self::quad();
        gl_use_default_material();
        unsafe { get_internal_gl() }.flush();
        unsafe { get_internal_gl() }.quad_gl.scissor(None);
        profiler.end(timing);
        Ok(())
    }
    fn screen_uv(p: Vec2, res: &Resource, vp: (i32, i32, i32, i32), w: f32, h: f32) -> Vec2 {
        let x = p.x / (5. * res.aspect_ratio) * if res.config.flip_x() { -1. } else { 1. };
        let y = -p.y / (5. * res.aspect_ratio);
        vec2((vp.0 as f32 + (x + 1.) * 0.5 * vp.2 as f32) / w, (vp.1 as f32 + (1. - y * res.aspect_ratio) * 0.5 * vp.3 as f32) / h)
    }
    fn mesh(points: [Vec2; 4], color: Color, tex: Option<Texture2D>) {
        let uv = [vec2(0., 0.), vec2(1., 0.), vec2(1., 1.), vec2(0., 1.)];
        let vertices = std::array::from_fn::<_, 4, _>(|i| macroquad::models::Vertex {
            position: vec3(points[i].x * 2. - 1., points[i].y * 2. - 1., 0.),
            uv: uv[i],
            color,
        });
        let gl = unsafe { get_internal_gl() }.quad_gl;
        gl.texture(tex);
        gl.draw_mode(DrawMode::Triangles);
        gl.geometry(&vertices, &[0, 1, 2, 0, 2, 3]);
    }

    pub fn render(&mut self, res: &mut Resource, areas: &[BlockArea], state: &NoiseAreaState, full_vp: (i32, i32, i32, i32)) -> Result<()> {
        if !res.config.noise_area.enabled {
            return Ok(());
        }
        if !areas.iter().any(|area| area.visible(res.time as f32)) && !state.hovers.iter().any(|hover| hover.scale > 0.) {
            return Ok(());
        }
        let Some(target) = res.chart_target.as_mut() else { return Ok(()) };
        unsafe { get_internal_gl() }.flush();
        target.swap();
        let source = target.old().texture;
        let output = target.output();
        let (w, h) = (source.width() as u32, source.height() as u32);
        let precise = res.config.noise_area.precise_edges;
        let low = res.config.noise_area.low_performance;
        self.profiler.frame(low, precise, w, h);
        if self.targets.as_ref().is_none_or(|t| t.w != w || t.h != h) || self.precise_enabled != precise || self.low_enabled != low {
            tracing::info!(stage = "targets_begin", width = w, height = h, precise, "noise-area initialization");
            self.targets = Some(Targets::new(w, h, precise, low));
            self.precise_cache_valid = false;
            tracing::info!(stage = "targets_ready", "noise-area initialization");
            self.precise_enabled = precise;
            self.low_enabled = low;
        }
        let rt = &self.targets.as_ref().unwrap().targets;
        let t = res.time as f32;
        let cv = res.camera.viewport.unwrap_or(full_vp);
        let vp = (cv.0 - full_vp.0, cv.1 - full_vp.1, cv.2, cv.3);
        // Resolve each visible area's event chains and screen transform once,
        // then reuse the same vertices for low-resolution / precise masks.
        let geometry_timing = self.profiler.begin(0);
        for group in &mut self.geometry {
            group.clear();
        }
        for area in areas.iter().filter(|area| area.visible(t)) {
            let phase = if area.active(t) {
                0
            } else if area.ready(t) {
                2
            } else {
                4
            };
            let fade = if phase == 4 && t < area.enable_time {
                ((t - area.appear_time) / 0.5).clamp(0., 1.)
            } else {
                1.
            };
            self.geometry[phase + usize::from(area.is_subtract)].push(ScreenQuad {
                points: area
                    .pose(t, res.aspect_ratio)
                    .corners()
                    .map(|p| Self::screen_uv(p, res, vp, w as f32, h as f32)),
                color: Color::new(1., if area.is_subtract { fade } else { 1. }, 1., if area.is_subtract { 0.1 } else { fade }),
            });
        }
        self.profiler.end(geometry_timing);
        let active = !self.geometry[0].is_empty() || !self.geometry[1].is_empty();
        let reuse_precise = precise
            && active
            && self.precise_cache_valid
            && self.precise_geometry[0] == self.geometry[0]
            && self.precise_geometry[1] == self.geometry[1];
        let ready = !self.geometry[2].is_empty() || !self.geometry[3].is_empty();
        let disabled = !self.geometry[4].is_empty() || !self.geometry[5].is_empty();
        let hover_visible = state.hovers.iter().any(|hover| hover.scale > 0.);
        let mut active_coverage = Coverage::new();
        let mut disabled_coverage = Coverage::new();
        for (i, group) in self.geometry.iter().enumerate() {
            for quad in group {
                for point in quad.points {
                    if i < 4 {
                        active_coverage.add(point);
                    } else {
                        disabled_coverage.add(point);
                    }
                }
            }
        }
        for hover in state.hovers.iter().filter(|hover| hover.scale > 0.) {
            // Sprite fits in this enclosing square; includes hiding/reused slots.
            for delta in [vec2(-0.22, -0.22), vec2(-0.22, 0.22), vec2(0.22, -0.22), vec2(0.22, 0.22)] {
                active_coverage.add(Self::screen_uv(hover.position + delta * hover.scale, res, vp, w as f32, h as f32));
            }
        }
        // Mask displacement is <= 0.1 * sqrt(0.5) per UV axis. The largest
        // accumulated ring/filter footprint is bounded by eight edge texels.
        let active_clip = active_coverage.clip(
            w,
            h,
            vec2(
                0.11 + 8. * (if precise { 4. } else { 1. }) / rt[11].texture.width(),
                0.11 + 8. * (if precise { 4. } else { 1. }) / rt[11].texture.height(),
            ),
        );
        let disabled_clip = disabled_coverage.clip(w, h, vec2(2. / rt[4].texture.width(), 2. / rt[4].texture.height()));
        // Empty channels always sample a persistent zero texture. Never read an
        // old target after skipping its pass (phase changes, seeks, restarts).
        let mut tex = [self.blank; 21];
        push_camera_state();
        let result = (|| -> Result<()> {
            for i in [0, 1, 2, 3, 4, 5, 14, 18, 19] {
                let group = if i >= 18 { i - 18 } else { i };
                if i == 14 {
                    if !hover_visible {
                        continue;
                    }
                } else if (i >= 18 && (!precise || reuse_precise)) || (precise && i < 2) || self.geometry[group].is_empty() {
                    continue;
                }
                tex[i] = rt[i].texture;
                let timing = self.profiler.begin(1);
                Self::camera(rt[i]);
                clear_background(BLANK);
                gl_use_material(if i == 14 { self.hover_sprite.material } else { self.sprite.material });
                if i == 14 {
                    for hover in &state.hovers {
                        if hover.scale <= 0. {
                            continue;
                        }
                        let center = hover.position;
                        let local: [Vec2; 6] = [
                            vec2(-0.14, 0.22),
                            vec2(0.22, -0.22),
                            vec2(0.22, 0.22),
                            vec2(-0.16, -0.22),
                            vec2(-0.22, 0.15),
                            vec2(-0.22, -0.11),
                        ];
                        let vertices = std::array::from_fn::<_, 6, _>(|i| {
                            let p = local[i];
                            let pos = Self::screen_uv(center + p * hover.scale, res, vp, w as f32, h as f32);
                            macroquad::models::Vertex {
                                position: vec3(pos.x * 2. - 1., pos.y * 2. - 1., 0.),
                                uv: p / 0.44 + Vec2::splat(0.5),
                                color: WHITE,
                            }
                        });
                        let gl = unsafe { get_internal_gl() }.quad_gl;
                        gl.texture(Some(self.touch));
                        gl.draw_mode(DrawMode::Triangles);
                        gl.geometry(&vertices, &[5, 4, 3, 0, 3, 4, 1, 3, 0, 2, 1, 0]);
                    }
                } else {
                    for area in &self.geometry[group] {
                        Self::mesh(area.points, area.color, None);
                    }
                }
                gl_use_default_material();
                unsafe { get_internal_gl() }.flush();
                self.profiler.end(timing);
            }
            for (p, target, input) in [(0, 6, 1), (1, 7, 3), (1, 10, 5)] {
                if self.geometry[input].is_empty() || (precise && target == 6) {
                    continue;
                }
                tex[target] = rt[target].texture;
                self.programs[p].f("_ClampThresholdLow", 0.09);
                self.programs[p].f("_ClampThresholdHigh", 0.12);
                Self::pass(&self.programs[p], rt[target], &[("_MainTex", tex[input])], true, &mut self.profiler, 2, None)?;
            }
            for (p, target, n, s) in [(2, 8, 0, 6), (3, 9, 2, 7)] {
                if (p == 2 && !active) || (p == 3 && !ready) || (p == 2 && precise) {
                    continue;
                }
                if p == 3 && self.geometry[3].is_empty() {
                    // Ready composition with no subtraction is exactly the existing mask.
                    tex[target] = tex[n];
                    continue;
                }
                tex[target] = rt[target].texture;
                let pr = &self.programs[p];
                pr.v4("_BlockTime", block_time(t, res.config.noise_area.no_jitter));
                let (bindings, len) = compose_textures(tex[n], tex[s], (p == 2).then_some(self.noise));
                Self::pass(pr, rt[target], &bindings[..len], true, &mut self.profiler, 3, None)?;
            }
            if precise && active {
                tex[8] = rt[8].texture;
            }
            if precise && active && !reuse_precise {
                let pr = &self.precise[supersampling(w, h).trailing_zeros() as usize];
                pr.v2("SourceSize", vec2(rt[18].texture.width(), rt[18].texture.height()));
                pr.v2("TargetSize", vec2(w as f32, h as f32));
                Self::pass(pr, rt[8], &[("Normal", tex[18]), ("Subtract", tex[19])], true, &mut self.profiler, 4, None)?;
            }
            let ew = rt[11].texture.width();
            let eh = rt[11].texture.height();
            let dilation = if precise { 4. } else { 1. };
            let texel = vec4(dilation / ew, dilation / eh, ew, eh);
            let mut glow = 8;
            if reuse_precise {
                glow = 12; // Five ping-pong passes always finish in target 12.
                tex[glow] = rt[glow].texture;
            }
            if active && !reuse_precise {
                // Full-resolution precise edge is kept when low-mode glow is downsampled.
                if low && precise {
                    tex[11] = rt[11].texture;
                    self.programs[4].v4("_DilateTexelSize", texel);
                    Self::pass(&self.programs[4], rt[11], &[("_MainTex", tex[8]), ("_ComposeRT", tex[8])], true, &mut self.profiler, 5, None)?;
                }
                let weights = [0.45856798, 0.28286123, 0.15658922, 0.07305908, 0.02494781];
                for (i, weight) in weights[..if low { 3 } else { 5 }].iter().copied().enumerate() {
                    let out = 12 + i % 2;
                    tex[out] = rt[out].texture;
                    let pr = &self.programs[5];
                    pr.v4("_DilateTexelSize", texel);
                    pr.f("_PassWeight", weight);
                    pr.f("_GlowFirstPass", if i == 0 { 1. } else { 0. });
                    // Full target clear keeps skipped texels zero across moves/seeks.
                    // Bound all five dilation rings plus mask displacement and nearest filtering.
                    let margin = vec2(0.11 + 8. * dilation / ew, 0.11 + 8. * dilation / eh);
                    let clip = active_coverage.clip(rt[out].texture.width() as u32, rt[out].texture.height() as u32, margin);
                    Self::pass(pr, rt[out], &[("_MainTex", tex[glow]), ("_ComposeRT", tex[8])], true, &mut self.profiler, 6, clip)?;
                    glow = out;
                }
            }
            if active || ready || hover_visible {
                Self::pass(
                    &self.mask,
                    rt[15],
                    &[("Active", tex[8]), ("Ready", tex[9]), ("Normal", tex[2]), ("Subtract", tex[7])],
                    true,
                    &mut self.profiler,
                    7,
                    None,
                )?;
                Self::pass(
                    if low && precise { &self.effect } else { &self.fused_effect },
                    rt[16],
                    &[("Edge", tex[11]), ("Glow", tex[glow]), ("Hover", tex[14])],
                    true,
                    &mut self.profiler,
                    7,
                    None,
                )?;
            }
            if disabled {
                if self.geometry[5].is_empty() {
                    tex[20] = tex[4];
                } else {
                    tex[20] = rt[20].texture;
                    Self::pass(
                        &self.programs[3],
                        rt[20],
                        &[("_DisabledNormalBlockRT", tex[4]), ("_DisabledSubtractBlockRT", tex[10])],
                        true,
                        &mut self.profiler,
                        3,
                        None,
                    )?;
                }
            }
            if (active || hover_visible) && !res.config.noise_area.remove_distortion {
                let timing = self.profiler.begin(8);
                Self::camera(rt[17]);
                clear_background(BLANK);
                Self::mesh([vec2(0., 0.), vec2(1., 0.), vec2(1., 1.), vec2(0., 1.)], WHITE, Some(source));
                unsafe { get_internal_gl() }.flush();
                self.profiler.end(timing);
            }
            // Preserve the original scene outside the block overlay.
            let timing = self.profiler.begin(9);
            Self::camera(output);
            clear_background(BLANK);
            Self::mesh([vec2(0., 0.), vec2(1., 0.), vec2(1., 1.), vec2(0., 1.)], WHITE, Some(source));
            unsafe { get_internal_gl() }.flush();
            self.profiler.end(timing);
            if disabled {
                let dp = &self.programs[6];
                dp.v4("_BlockTime", block_time(t, res.config.noise_area.no_jitter));
                Self::pass(
                    dp,
                    output,
                    &[("_ComposeRT", tex[20]), ("_DisplaceMap", self.noise), ("_SparkMap", self.spark)],
                    false,
                    &mut self.profiler,
                    10,
                    disabled_clip,
                )?;
            }
            if active || ready || hover_visible {
                let no_touch = state.positions.is_empty() && !hover_visible;
                let no_distortion = res.config.noise_area.remove_distortion;
                let variant = if !active && !hover_visible {
                    Some(3)
                } else if no_distortion {
                    Some(1 + usize::from(no_touch))
                } else if no_touch {
                    Some(0)
                } else {
                    None
                };
                let pr = variant.map_or(&self.programs[7], |i| &self.variants[i]);
                pr.v4("_BlockTime", block_time(t, res.config.noise_area.no_jitter));
                pr.v4("_ScreenParams", vec4(w as f32, h as f32, 1. + 1. / w as f32, 1. + 1. / h as f32));
                pr.v4("_ProjectionParams", vec4(1., 0.1, 100., 0.01));
                pr.v4("_EffectRT_TexelSize", vec4(1. / ew, 1. / eh, ew, eh));
                pr.v2("_HoverSize", vec2(rt[14].texture.width(), rt[14].texture.height()));
                pr.f("_RemoveDistortion", if res.config.noise_area.remove_distortion { 1. } else { 0. });
                pr.f("_TouchPosShine", 1.);
                pr.material
                    .set_uniform("_TouchPosCount", state.positions.len().min(TOUCH_UNIFORMS.len()) as i32);
                for (i, name) in TOUCH_UNIFORMS.iter().enumerate() {
                    let p = state
                        .positions
                        .get(i)
                        .map(|p| Self::screen_uv(*p, res, vp, w as f32, h as f32))
                        .unwrap_or(Vec2::ZERO);
                    pr.v2(name, vec2(p.x * w as f32 / h as f32, p.y));
                }
                Self::pass(
                    pr,
                    output,
                    &[
                        ("_MaskRT", rt[15].texture),
                        ("_EffectRT", rt[16].texture),
                        ("_DisplaceMap", self.noise),
                        ("_SparkMap", self.spark),
                        ("_NoiseMap", self.grain),
                        (
                            "_SceneColor",
                            if res.config.noise_area.remove_distortion || !(active || hover_visible) {
                                source
                            } else {
                                rt[17].texture
                            },
                        ),
                    ],
                    false,
                    &mut self.profiler,
                    11,
                    active_clip,
                )?;
            }
            Ok(())
        })();
        unsafe { get_internal_gl() }.quad_gl.scissor(None);
        pop_camera_state();
        let gl = unsafe { get_internal_gl() };
        gl.quad_gl.render_pass(Some(output.render_pass));
        gl.quad_gl.viewport(Some(vp));
        self.precise_cache_valid = result.is_ok() && precise && active;
        if self.precise_cache_valid && !reuse_precise {
            for i in 0..2 {
                self.precise_geometry[i].clone_from(&self.geometry[i]);
            }
        }
        result
    }
}
impl Drop for EffectRenderer {
    fn drop(&mut self) {
        for tex in [self.noise, self.spark, self.grain, self.touch, self.blank] {
            tex.delete();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn no_jitter_freezes_the_block_time_axis() {
        assert_eq!(block_time(7.5, true), Vec4::ZERO);
        assert_eq!(block_time(-3., true), Vec4::ZERO);
        assert_eq!(block_time(20., false), vec4(1., 20., 40., 60.));
    }
    #[test]
    fn microscopic_duplicate_edges_do_not_cut_away_valid_polygons() {
        let poly = vec![vec2(0., 0.), vec2(1., 0.), vec2(1., 1.), vec2(1. - 1e-7, 1.), vec2(0., 1.)];
        let clean = clean_polygon(poly);
        assert_eq!(clean.len(), 4);
        assert!(inside_convex(vec2(0.8, 0.6), &clean));
        assert!(subtract_polygon(clean, &[vec2(2., 2.), vec2(3., 2.), vec2(3., 3.), vec2(2., 3.)]).len() == 1);
    }
    #[test]
    fn subtract_parity_restores_double_holes_and_draws_standalone_regions() {
        let screen = [vec2(0., 0.), vec2(1., 0.), vec2(1., 1.), vec2(0., 1.)];
        let hole = [vec2(0.25, 0.25), vec2(0.75, 0.25), vec2(0.75, 0.75), vec2(0.25, 0.75)];
        let doubled = mask_polygons(&[screen], &[hole, hole], &screen);
        assert!((doubled.iter().map(|p| polygon_area(p).abs()).sum::<f32>() - 1.).abs() < 1e-6);
        let solo = mask_polygons(&[], &[hole], &screen);
        assert!((solo.iter().map(|p| polygon_area(p).abs()).sum::<f32>() - 0.25).abs() < 1e-6);
        assert!(mask_polygons(&[screen], &[screen], &screen).is_empty());
        // Overlapping normals are a union, not independent translucent layers.
        let overlap = mask_polygons(&[hole, hole], &[], &screen);
        assert!((overlap.iter().map(|p| polygon_area(p).abs()).sum::<f32>() - 0.25).abs() < 1e-6);
    }
    #[test]
    fn simple_render_matches_actual_blocked_function_for_mixed_rotated_areas() {
        use ::rand::{Rng, SeedableRng};
        let mut rng = ::rand::rngs::StdRng::seed_from_u64(150015);
        let screen = [vec2(0., 0.), vec2(1., 0.), vec2(1., 1.), vec2(0., 1.)];
        for case in 0..24 {
            let mut areas = Vec::new();
            for i in 0..6 {
                let center = vec2(rng.gen_range(0.1..0.9), rng.gen_range(0.1..0.9));
                let size = vec2(rng.gen_range(0.08..0.8), rng.gen_range(0.08..0.8));
                areas.push(BlockArea {
                    bottom_left_percentage: super::super::Point {
                        x: center.x - size.x / 2.,
                        y: center.y - size.y / 2.,
                    },
                    top_right_percentage: super::super::Point {
                        x: center.x + size.x / 2.,
                        y: center.y + size.y / 2.,
                    },
                    enable_time: 0.,
                    disable_time: 2.,
                    disappear_time: 3.,
                    is_subtract: i < case % 5,
                    rotate_events: vec![super::super::BlockRotateEvent {
                        time: 0.,
                        rotation: rng.gen_range(-180.0..180.0),
                        anchor: super::super::Point { x: center.x, y: center.y },
                        ..Default::default()
                    }],
                    ..Default::default()
                });
            }
            if case % 3 == 0 {
                areas.push(areas[0].clone());
            }
            for flip in [false, true] {
                let project = |p: Vec2| vec2(if flip { 0.5 - p.x / 10. } else { 0.5 + p.x / 10. }, 0.5 + p.y / 10.);
                let mut normal = Vec::new();
                let mut holes = Vec::new();
                let mut adj_normal = Vec::new();
                let mut adj_holes = Vec::new();
                for a in &areas {
                    let pose = a.pose(1., 1.);
                    let p = pose.corners().map(project);
                    let q = adjusted_pose(pose, a.is_subtract).corners().map(project);
                    if a.is_subtract {
                        holes.push(p);
                        adj_holes.push(q);
                    } else {
                        normal.push(p);
                        adj_normal.push(q);
                    }
                }
                let raw = mask_polygons(&normal, &holes, &screen);
                let adj = mask_polygons(&adj_normal, &adj_holes, &screen);
                let polys = intersect_sets(&raw, &adj);
                for y in 0..23 {
                    for x in 0..29 {
                        let uv = vec2((x as f32 + 0.37) / 29., (y as f32 + 0.61) / 23.);
                        let world = vec2((uv.x - 0.5) * 10. * if flip { -1. } else { 1. }, (uv.y - 0.5) * 10.);
                        let rendered = polys.iter().any(|p| inside_convex(uv, p));
                        assert_eq!(rendered, super::super::blocked(&areas, world, 1., 1.), "case={case}, flip={flip}, point={uv:?}");
                    }
                }
            }
        }
    }
    #[test]
    fn low_geometry_preserves_holes_and_flipped_winding() {
        let quad = [vec2(0., 0.), vec2(1., 0.), vec2(1., 1.), vec2(0., 1.)];
        let mut hole = [vec2(0.25, 0.25), vec2(0.75, 0.25), vec2(0.75, 0.75), vec2(0.25, 0.75)];
        for _ in 0..2 {
            let pieces = subtract_polygon(quad.to_vec(), &hole);
            let area: f32 = pieces
                .iter()
                .map(|p| (0..p.len()).map(|i| p[i].perp_dot(p[(i + 1) % p.len()])).sum::<f32>().abs() * 0.5)
                .sum();
            assert!((area - 0.75).abs() < 1e-6);
            hole.reverse();
        }
        assert!(subtract_polygon(quad.to_vec(), &quad).is_empty());
        assert!(!on_segment(vec2(0.1, 0.25), hole[0], hole[1]));
    }
    #[test]
    fn precise_samples_follow_allocated_supersampling() {
        assert_eq!(supersampling(960, 540), 4);
        assert_eq!(supersampling(1920, 1080), 2);
        assert_eq!(supersampling(3840, 2160), 1);
        for n in [1, 2, 4] {
            let shader = precise_variant(n);
            assert!(shader.contains(&format!("y<{n}")));
            assert!(shader.contains(&format!("v/{}.", n * n)));
        }
    }
    #[test]
    fn coverage_includes_margin_and_uses_top_origin_scissor() {
        let mut c = Coverage::new();
        assert_eq!(c.clip(100, 100, Vec2::ZERO), None);
        c.add(vec2(0.2, 0.3));
        c.add(vec2(0.4, 0.6));
        let (x, y, w, h) = c.clip(100, 100, Vec2::splat(0.1)).unwrap();
        assert!(x <= 10 && y <= 30 && x + w >= 50 && y + h >= 80);
        c.add(vec2(f32::NAN, 0.));
        assert_eq!(c.clip(100, 100, Vec2::ZERO), Some((0, 0, 100, 100)));
    }
    #[test]
    fn specialized_shaders_remove_only_requested_paths() {
        let plain = active_variant(true, true, false, false);
        let body = plain.split_once("void main()").unwrap().1;
        assert!(!body.contains("_TouchPosCount"));
        assert!(!body.contains("mix(u_xlat16.xy, vs_TEXCOORD0.xy, _RemoveDistortion)"));
        assert!(body.contains("_SparkDisplaceIntensity"));
        let low = active_variant(false, false, true, false);
        assert!(!low.contains("u_xlat48 = _BlockTime.y * _NoiseDirChangeSpeed"));
        assert!(low.contains("clamp(u_xlat48, 0.0, 1.0) * _TouchGlowColor.xyz"));
        let ready = active_variant(true, true, false, true);
        let body = ready.split_once("void main()").unwrap().1;
        assert!(!body.contains("maskChannels.r"));
        assert!(body.contains("maskChannels.g"));
    }
    #[test]
    fn compose_bindings_match_actual_shader_sampler_declarations() {
        let regex = regex::Regex::new(r"uniform\s+(?:(?:highp|mediump|lowp)\s+)?sampler2D\s+(\w+)\s*;").unwrap();
        for (shader, displacement) in [
            (include_str!("shaders/BlockCompose_program_0.glsl"), Some(3)),
            (include_str!("shaders/BlockCompose_program_1.glsl"), None),
        ] {
            let declared: HashSet<String> = regex.captures_iter(shader).map(|cap| cap[1].to_owned()).collect();
            let (bindings, len) = compose_textures(1, 2, displacement);
            let bound: HashSet<String> = bindings[..len].iter().map(|(name, _)| name.to_string()).collect();
            assert_eq!(bound, declared);
            assert!(displacement.is_some() || !bound.contains("_DisplaceMap"));
        }
    }
}
