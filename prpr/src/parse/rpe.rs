use anyhow::{Context, Result};
use image::{codecs::gif, AnimationDecoder, DynamicImage, ImageError};
use macroquad::prelude::{Color, WHITE};
use sasa::AudioClip;
use serde::{Deserialize, Deserializer};
use std::{any::Any, cell::RefCell, collections::HashMap, future::IntoFuture, io::Cursor, rc::Rc, str::FromStr, time::Duration};
use tracing::debug;

use super::{process_lines, L10N_LOCAL, RPE_TWEEN_MAP};
use crate::{
    core::{
        Anim, AnimFloat, AnimVector, BezierTween, BpmList, Chart, ChartExtra, ChartSettings, ClampedTween, CtrlObject, GifFrames, HitSoundMap,
        JudgeLine, JudgeLineCache, JudgeLineKind, Keyframe, Note, NoteKind, Object, StaticTween, Triple, TweenFunction, Tweenable, UIElement, EPS,
        HEIGHT_RATIO,
    },
    ext::{NotNanExt, SafeTexture},
    fs::FileSystem,
    judge::{HitSound, JudgeStatus},
};

pub const RPE_WIDTH: f32 = 1350.;
pub const RPE_HEIGHT: f32 = 900.;
const SPEED_RATIO: f32 = 10. / 45. / HEIGHT_RATIO;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RPEBpmItem {
    bpm: f32,
    start_time: Triple,
}

// serde is weird...
fn f32_zero() -> f32 {
    0.
}

fn f32_one() -> f32 {
    1.
}

fn rpe_version_default() -> i32 {
    160
}

fn deserialize_rpe_version<'de, D>(deserializer: D) -> std::result::Result<i32, D::Error>
where
    D: Deserializer<'de>,
{
    let value: Option<serde_json::Value> = Option::deserialize(deserializer)?;
    let parsed = match value {
        Some(serde_json::Value::Number(v)) => v.as_i64().map(|it| it as i32),
        Some(serde_json::Value::String(s)) => s.parse::<i32>().ok(),
        _ => None,
    };
    Ok(parsed.unwrap_or(rpe_version_default()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RPEEvent<T = f32> {
    #[serde(default = "f32_zero")]
    easing_left: f32,
    #[serde(default = "f32_one")]
    easing_right: f32,
    #[serde(default)]
    bezier: u8,
    #[serde(default)]
    bezier_points: [f32; 4],
    easing_type: i32,
    start: T,
    end: T,
    start_time: Triple,
    end_time: Triple,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RPECtrlEvent {
    easing: u8,
    x: f32,
    #[serde(flatten)]
    value: HashMap<String, f32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RPESpeedEvent {
    start_time: Triple,
    end_time: Triple,
    start: f32,
    end: f32,
    easing_type: i32,
    #[serde(default = "f32_zero")]
    easing_left: f32,
    #[serde(default = "f32_one")]
    easing_right: f32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RPEEventLayer {
    alpha_events: Option<Vec<RPEEvent>>,
    move_x_events: Option<Vec<RPEEvent>>,
    move_y_events: Option<Vec<RPEEvent>>,
    rotate_events: Option<Vec<RPEEvent>>,
    speed_events: Option<Vec<RPESpeedEvent>>,
}

#[derive(Clone, Deserialize)]
struct RGBColor(u8, u8, u8);
impl From<RGBColor> for Color {
    fn from(RGBColor(r, g, b): RGBColor) -> Self {
        Self::from_rgba(r, g, b, 255)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RPEExtendedEvents {
    color_events: Option<Vec<RPEEvent<RGBColor>>>,
    text_events: Option<Vec<RPEEvent<String>>>,
    scale_x_events: Option<Vec<RPEEvent>>,
    scale_y_events: Option<Vec<RPEEvent>>,
    incline_events: Option<Vec<RPEEvent>>,
    paint_events: Option<Vec<RPEEvent>>,
    gif_events: Option<Vec<RPEEvent>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RPENote {
    // TODO above == 0? what does that even mean?
    #[serde(rename = "type")]
    kind: u8,
    above: u8,
    start_time: Triple,
    end_time: Triple,
    position_x: f32,
    y_offset: f32,
    alpha: u16,               // some alpha has 256...
    hitsound: Option<String>, // TODO implement this feature
    size: f32,
    speed: f32,
    is_fake: u8,
    visible_time: f32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RPEJudgeLine {
    // TODO group
    // TODO bpmfactor
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Texture")]
    texture: String,
    #[serde(rename = "father")]
    parent: Option<isize>,
    rotate_with_father: Option<bool>,
    event_layers: Vec<Option<RPEEventLayer>>,
    extended: Option<RPEExtendedEvents>,
    notes: Option<Vec<RPENote>>,
    is_cover: u8,
    #[serde(default)]
    z_order: i32,
    #[serde(rename = "attachUI")]
    attach_ui: Option<UIElement>,

    #[serde(default)]
    pos_control: Vec<RPECtrlEvent>,
    #[serde(default)]
    size_control: Vec<RPECtrlEvent>,
    #[serde(default)]
    alpha_control: Vec<RPECtrlEvent>,
    #[serde(default)]
    y_control: Vec<RPECtrlEvent>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RPEMetadata {
    offset: i32,
    #[serde(rename = "RPEVersion", default = "rpe_version_default", deserialize_with = "deserialize_rpe_version")]
    rpe_version: i32,
}

#[derive(Copy, Clone)]
enum SpeedEasingMode {
    Legacy,
    Modern,
}

fn speed_easing_type(type_id: i32) -> i32 {
    if (1..=28).contains(&type_id) {
        type_id
    } else {
        1
    }
}

fn sanitize_speed_easing_params(easing_type: i32, x: f32, easing_left: f32, easing_right: f32) -> (i32, f32, f32, f32) {
    let t = speed_easing_type(easing_type);
    let x = x.clamp(0., 1.);
    let left = easing_left.clamp(0., 1.);
    let right = easing_right.clamp(0., 1.);
    if left >= right {
        (t, x, 0., 1.)
    } else {
        (t, x, left, right)
    }
}

fn speed_bounce_out_value(mut x: f32) -> f32 {
    if x < 1. / 2.75 {
        7.5625 * x * x
    } else if x < 2. / 2.75 {
        x -= 1.5 / 2.75;
        7.5625 * x * x + 0.75
    } else if x < 2.5 / 2.75 {
        x -= 2.25 / 2.75;
        7.5625 * x * x + 0.9375
    } else {
        x -= 2.625 / 2.75;
        7.5625 * x * x + 0.984375
    }
}

fn speed_raw_easing_value(easing_type: i32, x: f32) -> f32 {
    let x = x.clamp(0., 1.);
    match speed_easing_type(easing_type) {
        1 => x,
        2 => ((x * std::f32::consts::PI) / 2.).sin(),
        3 => 1. - ((x * std::f32::consts::PI) / 2.).cos(),
        4 => 1. - (1. - x) * (1. - x),
        5 => x * x,
        6 => -((std::f32::consts::PI * x).cos() - 1.) / 2.,
        7 => {
            if x < 0.5 {
                2. * x * x
            } else {
                1. - (-2. * x + 2.).powi(2) / 2.
            }
        }
        8 => 1. - (1. - x).powi(3),
        9 => x.powi(3),
        10 => 1. - (1. - x).powi(4),
        11 => x.powi(4),
        12 => {
            if x < 0.5 {
                4. * x.powi(3)
            } else {
                1. - (-2. * x + 2.).powi(3) / 2.
            }
        }
        13 => {
            if x < 0.5 {
                8. * x.powi(4)
            } else {
                1. - (-2. * x + 2.).powi(4) / 2.
            }
        }
        14 => 1. - (1. - x).powi(5),
        15 => x.powi(5),
        16 => {
            if (x - 1.).abs() < EPS {
                1.
            } else {
                1. - 2f32.powf(-10. * x)
            }
        }
        17 => {
            if x.abs() < EPS {
                0.
            } else {
                2f32.powf(10. * x - 10.)
            }
        }
        18 => (1. - (x - 1.).powi(2)).sqrt(),
        19 => 1. - (1. - x * x).sqrt(),
        20 => 1. + 2.70158 * (x - 1.).powi(3) + 1.70158 * (x - 1.).powi(2),
        21 => 2.70158 * x.powi(3) - 1.70158 * x.powi(2),
        22 => {
            if x < 0.5 {
                (1. - (1. - (2. * x).powi(2)).sqrt()) / 2.
            } else {
                ((1. - (-2. * x + 2.).powi(2)).sqrt() + 1.) / 2.
            }
        }
        23 => {
            if x < 0.5 {
                ((2. * x).powi(2) * ((2.59491 + 1.) * 2. * x - 2.59491)) / 2.
            } else {
                ((2. * x - 2.).powi(2) * ((2.59491 + 1.) * (x * 2. - 2.) + 2.59491) + 2.) / 2.
            }
        }
        24 => {
            if x.abs() < EPS {
                0.
            } else if (x - 1.).abs() < EPS {
                1.
            } else {
                2f32.powf(-10. * x) * ((x * 10. - 0.75) * ((2. * std::f32::consts::PI) / 3.)).sin() + 1.
            }
        }
        25 => {
            if x.abs() < EPS {
                0.
            } else if (x - 1.).abs() < EPS {
                1.
            } else {
                -2f32.powf(10. * x - 10.) * ((x * 10. - 10.75) * ((2. * std::f32::consts::PI) / 3.)).sin()
            }
        }
        26 => speed_bounce_out_value(x),
        27 => 1. - speed_bounce_out_value(1. - x),
        28 => {
            if x < 0.5 {
                (1. - speed_bounce_out_value(1. - 2. * x)) / 2.
            } else {
                (1. + speed_bounce_out_value(2. * x - 1.)) / 2.
            }
        }
        _ => x,
    }
}

fn speed_bounce_out_integral(x: f32) -> f32 {
    let x = x.clamp(0., 1.);
    let a: f32 = 7.5625;
    let b1: f32 = 1. / 2.75;
    let b2: f32 = 2. / 2.75;
    let b3: f32 = 2.5 / 2.75;
    let c1: f32 = 1.5 / 2.75;
    let c2: f32 = 2.25 / 2.75;
    let c3: f32 = 2.625 / 2.75;
    let i_b1 = (a * b1.powi(3)) / 3.;
    let i_b2 = i_b1 + (a / 3.) * ((b2 - c1).powi(3) - (b1 - c1).powi(3)) + 0.75 * (b2 - b1);
    let i_b3 = i_b2 + (a / 3.) * ((b3 - c2).powi(3) - (b2 - c2).powi(3)) + 0.9375 * (b3 - b2);
    if x < b1 {
        (a * x.powi(3)) / 3.
    } else if x < b2 {
        i_b1 + (a / 3.) * ((x - c1).powi(3) - (b1 - c1).powi(3)) + 0.75 * (x - b1)
    } else if x < b3 {
        i_b2 + (a / 3.) * ((x - c2).powi(3) - (b2 - c2).powi(3)) + 0.9375 * (x - b2)
    } else {
        i_b3 + (a / 3.) * ((x - c3).powi(3) - (b3 - c3).powi(3)) + 0.984375 * (x - b3)
    }
}

fn speed_raw_easing_integral(easing_type: i32, x: f32) -> f32 {
    let x = x.clamp(0., 1.);
    match speed_easing_type(easing_type) {
        1 => x * x / 2.,
        2 => (2. / std::f32::consts::PI) * (1. - ((x * std::f32::consts::PI) / 2.).cos()),
        3 => x - (2. / std::f32::consts::PI) * ((x * std::f32::consts::PI) / 2.).sin(),
        4 => x * x - x.powi(3) / 3.,
        5 => x.powi(3) / 3.,
        6 => x / 2. - (std::f32::consts::PI * x).sin() / (2. * std::f32::consts::PI),
        7 => {
            if x < 0.5 {
                (2. / 3.) * x.powi(3)
            } else {
                2. * x * x - (2. / 3.) * x.powi(3) - x + 1. / 6.
            }
        }
        8 => (3. / 2.) * x * x - x.powi(3) + x.powi(4) / 4.,
        9 => x.powi(4) / 4.,
        10 => 2. * x * x - 2. * x.powi(3) + x.powi(4) - x.powi(5) / 5.,
        11 => x.powi(5) / 5.,
        12 => {
            if x < 0.5 {
                x.powi(4)
            } else {
                -3. * x + 6. * x * x - 4. * x.powi(3) + x.powi(4) + 0.5
            }
        }
        13 => {
            if x < 0.5 {
                (8. / 5.) * x.powi(5)
            } else {
                -7. * x + 16. * x * x - 16. * x.powi(3) + 8. * x.powi(4) - (8. / 5.) * x.powi(5) + 11. / 10.
            }
        }
        14 => (5. / 2.) * x * x - (10. / 3.) * x.powi(3) + (5. / 2.) * x.powi(4) - x.powi(5) + x.powi(6) / 6.,
        15 => x.powi(6) / 6.,
        16 => x - (1. - 2f32.powf(-10. * x)) / (10. * std::f32::consts::LN_2),
        17 => (2f32.powf(10. * x - 10.) - 2f32.powf(-10.)) / (10. * std::f32::consts::LN_2),
        18 => 0.5 * ((x - 1.) * (0f32.max(1. - (x - 1.).powi(2))).sqrt() + (x - 1.).asin()) + std::f32::consts::PI / 4.,
        19 => x - 0.5 * (x * (0f32.max(1. - x * x)).sqrt() + x.clamp(-1., 1.).asin()),
        20 => {
            let a = 2.70158;
            let b = 1.70158;
            (1. - a + b) * x + ((3. * a - 2. * b) / 2.) * x * x + ((-3. * a + b) / 3.) * x.powi(3) + (a / 4.) * x.powi(4)
        }
        21 => (2.70158 / 4.) * x.powi(4) - (1.70158 / 3.) * x.powi(3),
        22 => {
            if x < 0.5 {
                0.5 * x - 0.25 * x * (0f32.max(1. - 4. * x * x)).sqrt() - 0.125 * (2. * x).clamp(-1., 1.).asin()
            } else {
                0.5 * x - 0.25 * (1. - x) * (0f32.max(1. - 4. * (1. - x) * (1. - x))).sqrt() - 0.125 * (2. * (1. - x)).clamp(-1., 1.).asin()
            }
        }
        23 => {
            let s = 2.59491;
            if x <= 0.5 {
                (s + 1.) * x.powi(4) - (2. * s * x.powi(3)) / 3.
            } else {
                let i_half = (s + 1.) * 0.5f32.powi(4) - (2. * s * 0.5f32.powi(3)) / 3.;
                let f = |t: f32| (s + 1.) * t.powi(4) - ((10. * s + 12.) / 3.) * t.powi(3) + ((8. * s + 12.) / 2.) * t * t - (2. * s + 3.) * t;
                i_half + (f(x) - f(0.5))
            }
        }
        24 => {
            let k = 10. * std::f32::consts::LN_2;
            let a = ((2. * std::f32::consts::PI) / 3.) * 10.;
            let b = -0.75 * ((2. * std::f32::consts::PI) / 3.);
            let h = |t: f32| (f32::exp(-k * t) * (-k * (a * t + b).sin() - a * (a * t + b).cos())) / (a * a + k * k);
            x + (h(x) - h(0.))
        }
        25 => {
            let k = 10. * std::f32::consts::LN_2;
            let a = ((2. * std::f32::consts::PI) / 3.) * 10.;
            let b = -10.75 * ((2. * std::f32::consts::PI) / 3.);
            let c = 2f32.powf(-10.);
            let g = |t: f32| (-c * f32::exp(k * t) * (k * (a * t + b).sin() - a * (a * t + b).cos())) / (a * a + k * k);
            g(x) - g(0.)
        }
        26 => speed_bounce_out_integral(x),
        27 => {
            let i1 = speed_bounce_out_integral(1.);
            x - (i1 - speed_bounce_out_integral(1. - x))
        }
        28 => {
            let i1 = speed_bounce_out_integral(1.);
            if x <= 0.5 {
                x / 2. + (i1 - speed_bounce_out_integral(1. - 2. * x)) / 4.
            } else {
                x / 2. + (i1 + speed_bounce_out_integral(2. * x - 1.)) / 4.
            }
        }
        _ => x * x / 2.,
    }
}

fn speed_easing_value(easing_type: i32, x: f32, easing_left: f32, easing_right: f32) -> f32 {
    let (easing_type, x, easing_left, easing_right) = sanitize_speed_easing_params(easing_type, x, easing_left, easing_right);
    let scaled = easing_left + (easing_right - easing_left) * x;
    let start = speed_raw_easing_value(easing_type, easing_left);
    let end = speed_raw_easing_value(easing_type, easing_right);
    let denom = end - start;
    if !denom.is_finite() || denom.abs() < 1e-8 {
        return x;
    }
    (speed_raw_easing_value(easing_type, scaled) - start) / denom
}

fn speed_easing_derivative(easing_type: i32, x: f32, easing_left: f32, easing_right: f32) -> f32 {
    let eps = 1e-6;
    let l = (x - eps).max(1e-7);
    let r = (x + eps).min(1. - 1e-7);
    if r <= l {
        return 0.;
    }
    let yl = speed_easing_value(easing_type, l, easing_left, easing_right);
    let yr = speed_easing_value(easing_type, r, easing_left, easing_right);
    (yr - yl) / (r - l)
}

fn speed_easing_integral(easing_type: i32, x: f32, easing_left: f32, easing_right: f32) -> f32 {
    let (easing_type, x, easing_left, easing_right) = sanitize_speed_easing_params(easing_type, x, easing_left, easing_right);
    let l = easing_left;
    let r = easing_right;
    let scaled_x = l + (r - l) * x;
    let f_l = speed_raw_easing_value(easing_type, l);
    let f_r = speed_raw_easing_value(easing_type, r);
    let denom = f_r - f_l;
    if !denom.is_finite() || denom.abs() < 1e-8 {
        return x * x / 2.;
    }
    let i_scaled = speed_raw_easing_integral(easing_type, scaled_x);
    let i_l = speed_raw_easing_integral(easing_type, l);
    (i_scaled - i_l - f_l * (scaled_x - l)) / ((r - l) * denom)
}

enum SpeedIntegralKind {
    Legacy {
        easing_type: i32,
        easing_left: f32,
        easing_right: f32,
        k: f32,
        b: f32,
    },
    Modern {
        easing_type: i32,
        easing_left: f32,
        easing_right: f32,
        start: f32,
        delta: f32,
    },
}

impl SpeedIntegralKind {
    fn partial_factor(&self, x: f32) -> f32 {
        match *self {
            Self::Legacy {
                easing_type,
                easing_left,
                easing_right,
                k,
                b,
            } => k * speed_easing_value(easing_type, x, easing_left, easing_right) + b * x,
            Self::Modern {
                easing_type,
                easing_left,
                easing_right,
                start,
                delta,
            } => start * x + delta * speed_easing_integral(easing_type, x, easing_left, easing_right),
        }
    }
}

struct SpeedIntegralTween {
    kind: SpeedIntegralKind,
    total: f32,
}

impl SpeedIntegralTween {
    fn try_create(kind: SpeedIntegralKind) -> Option<(Rc<dyn TweenFunction>, f32)> {
        let total = kind.partial_factor(1.);
        if !total.is_finite() || total.abs() < EPS {
            return None;
        }
        Some((Rc::new(Self { kind, total }), total))
    }
}

impl TweenFunction for SpeedIntegralTween {
    fn y(&self, x: f32) -> f32 {
        if x <= 0. {
            return 0.;
        }
        if x >= 1. {
            return 1.;
        }
        let y = self.kind.partial_factor(x) / self.total;
        if y.is_finite() {
            y
        } else {
            x
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn speed_linear_tween(start_speed: f32, end_speed: f32) -> Rc<dyn TweenFunction> {
    if (start_speed - end_speed).abs() < EPS {
        StaticTween::get_rc(2)
    } else if start_speed.abs() > end_speed.abs() {
        Rc::new(ClampedTween::new(7 /*quadOut*/, 0.0..(1. - end_speed / start_speed)))
    } else {
        Rc::new(ClampedTween::new(6 /*quadIn*/, (start_speed / end_speed)..1.))
    }
}

fn speed_segment_tween(
    mode: SpeedEasingMode,
    start_speed: f32,
    end_speed: f32,
    easing_type: i32,
    easing_left: f32,
    easing_right: f32,
) -> (Rc<dyn TweenFunction>, f32) {
    if easing_type <= 1 {
        return (speed_linear_tween(start_speed, end_speed), (start_speed + end_speed) / 2.);
    }
    let (tween, total) = match mode {
        SpeedEasingMode::Legacy => {
            let df0 = speed_easing_derivative(easing_type, 0., easing_left, easing_right);
            let df1 = speed_easing_derivative(easing_type, 1., easing_left, easing_right);
            let denom = df1 - df0;
            if !denom.is_finite() || denom.abs() < 1e-8 {
                return (speed_linear_tween(start_speed, end_speed), (start_speed + end_speed) / 2.);
            }
            let k = (end_speed - start_speed) / denom;
            let b = start_speed - k * df0;
            SpeedIntegralTween::try_create(SpeedIntegralKind::Legacy {
                easing_type,
                easing_left,
                easing_right,
                k,
                b,
            })
        }
        SpeedEasingMode::Modern => SpeedIntegralTween::try_create(SpeedIntegralKind::Modern {
            easing_type,
            easing_left,
            easing_right,
            start: start_speed,
            delta: end_speed - start_speed,
        }),
    }
    .unwrap_or_else(|| (speed_linear_tween(start_speed, end_speed), (start_speed + end_speed) / 2.));
    (tween, total)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RPEChart {
    #[serde(rename = "META")]
    meta: RPEMetadata,
    #[serde(rename = "BPMList")]
    bpm_list: Vec<RPEBpmItem>,
    judge_line_list: Vec<RPEJudgeLine>,
}

type BezierMap = HashMap<(u16, i16, i16), Rc<dyn TweenFunction>>;

fn bezier_key<T>(event: &RPEEvent<T>) -> (u16, i16, i16) {
    let p = &event.bezier_points;
    let int = |p: f32| (p * 100.).round() as i16;
    ((int(p[0]) * 100 + int(p[1])) as u16, int(p[2]), int(p[3]))
}

fn parse_events<T: Tweenable, V: Clone + Into<T>>(
    r: &mut BpmList,
    rpe: &[RPEEvent<V>],
    default: Option<T>,
    bezier_map: &BezierMap,
) -> Result<Anim<T>> {
    if rpe.is_empty() {
        return Ok(Anim::default());
    }
    let mut kfs = Vec::new();
    if let Some(default) = default {
        if rpe[0].start_time.beats() != 0.0 {
            kfs.push(Keyframe::new(0.0, default, 0));
        }
    }
    for e in rpe {
        kfs.push(Keyframe {
            time: r.time(&e.start_time),
            value: e.start.clone().into(),
            tween: {
                let tween = RPE_TWEEN_MAP.get(e.easing_type.max(1) as usize).copied().unwrap_or(RPE_TWEEN_MAP[0]);
                if e.bezier != 0 {
                    Rc::clone(&bezier_map[&bezier_key(e)])
                } else if e.easing_left.abs() < EPS && (e.easing_right - 1.0).abs() < EPS {
                    StaticTween::get_rc(tween)
                } else {
                    Rc::new(ClampedTween::new(tween, e.easing_left..e.easing_right))
                }
            },
        });
        kfs.push(Keyframe::new(r.time(&e.end_time), e.end.clone().into(), 0));
    }
    Ok(Anim::new(kfs))
}

fn parse_speed_events(r: &mut BpmList, rpe: &[RPEEventLayer], max_time: f32, mode: SpeedEasingMode) -> Result<AnimFloat> {
    let layers: Vec<_> = rpe.iter().filter_map(|it| it.speed_events.as_ref()).collect();
    if layers.is_empty() {
        return Ok(AnimFloat::default());
    }
    let mut anis = Vec::new();
    for layer in layers {
        if layer.is_empty() {
            continue;
        }
        let mut events: Vec<_> = layer
            .iter()
            .map(|it| {
                (
                    r.time(&it.start_time),
                    r.time(&it.end_time),
                    it.start * SPEED_RATIO,
                    it.end * SPEED_RATIO,
                    it.easing_type,
                    it.easing_left,
                    it.easing_right,
                )
            })
            .collect();
        events.sort_by_key(|it| it.0.not_nan());
        let mut kfs = vec![Keyframe::new(0.0, 0.0, 2)];
        let mut height = 0.0;
        let mut cursor = 0.0;
        let mut last_speed = 0.0;
        let push_segment = |start_time: f32,
                            end_time: f32,
                            start_speed: f32,
                            end_speed: f32,
                            easing_type: i32,
                            easing_left: f32,
                            easing_right: f32,
                            mode: SpeedEasingMode,
                            kfs: &mut Vec<Keyframe<f32>>,
                            height: &mut f32| {
            if end_time - start_time <= EPS {
                return;
            }
            let (tween, integral_factor) = speed_segment_tween(mode, start_speed, end_speed, easing_type, easing_left, easing_right);
            if let Some(last) = kfs.last_mut() {
                if (last.time - start_time).abs() < EPS {
                    last.value = *height;
                    last.tween = tween;
                } else {
                    kfs.push(Keyframe {
                        time: start_time,
                        value: *height,
                        tween,
                    });
                }
            }
            *height += integral_factor * (end_time - start_time);
        };
        for (start_time, end_time, start_speed, end_speed, easing_type, easing_left, easing_right) in events {
            let start_time = start_time.max(cursor);
            let end_time = end_time.max(start_time);
            if start_time > cursor + EPS {
                push_segment(cursor, start_time, last_speed, last_speed, 1, 0., 1., mode, &mut kfs, &mut height);
            }
            if end_time > start_time + EPS {
                if easing_type <= 1 && start_speed.signum() * end_speed.signum() < 0. {
                    let x = start_speed / (start_speed - end_speed);
                    let mid = f32::tween(&start_time, &end_time, x);
                    push_segment(start_time, mid, start_speed, 0., easing_type, easing_left, easing_right, mode, &mut kfs, &mut height);
                    push_segment(mid, end_time, 0., end_speed, easing_type, easing_left, easing_right, mode, &mut kfs, &mut height);
                } else {
                    push_segment(start_time, end_time, start_speed, end_speed, easing_type, easing_left, easing_right, mode, &mut kfs, &mut height);
                }
            }
            cursor = end_time;
            last_speed = end_speed;
        }
        if max_time > cursor + EPS {
            push_segment(cursor, max_time, last_speed, last_speed, 1, 0., 1., mode, &mut kfs, &mut height);
        }
        if let Some(last) = kfs.last() {
            if (last.time - max_time).abs() > EPS {
                kfs.push(Keyframe::new(max_time, height, 0));
            }
        }
        anis.push(AnimFloat::new(kfs));
    }
    if anis.is_empty() {
        return Ok(AnimFloat::default());
    }
    Ok(AnimFloat::chain(anis))
}

fn parse_gif_events<V: Clone + Into<f32>>(r: &mut BpmList, rpe: &[RPEEvent<V>], bezier_map: &BezierMap, gif: &GifFrames) -> Result<Anim<f32>> {
    let mut kfs = Vec::new();
    kfs.push(Keyframe::new(0.0, 0.0, 2));
    let mut next_rep_time: u128 = 0;
    for e in rpe {
        while r.time(&e.start_time) > next_rep_time as f32 / 1000. {
            kfs.push(Keyframe::new(next_rep_time as f32 / 1000., 1.0, 0));
            kfs.push(Keyframe::new(next_rep_time as f32 / 1000., 0.0, 2));
            next_rep_time += gif.total_time();
        }
        let stop_prog = 1. - (next_rep_time as f32 - r.time(&e.start_time) * 1000.) / gif.total_time() as f32;
        kfs.push(Keyframe::new(r.time(&e.start_time), stop_prog, 0));
        kfs.push(Keyframe {
            time: r.time(&e.start_time),
            value: e.start.clone().into(),
            tween: {
                let tween = RPE_TWEEN_MAP.get(e.easing_type.max(1) as usize).copied().unwrap_or(RPE_TWEEN_MAP[0]);
                if e.bezier != 0 {
                    Rc::clone(&bezier_map[&bezier_key(e)])
                } else if e.easing_left.abs() < EPS && (e.easing_right - 1.0).abs() < EPS {
                    StaticTween::get_rc(tween)
                } else {
                    Rc::new(ClampedTween::new(tween, e.easing_left..e.easing_right))
                }
            },
        });
        kfs.push(Keyframe::new(r.time(&e.end_time), e.end.clone().into(), 2));
        next_rep_time = (r.time(&e.end_time) * 1000. + gif.total_time() as f32 * (1. - e.end.clone().into())).round() as u128;
    }

    // TODO maybe a better approach?
    const GIF_MAX_TIME: f32 = 2000.;
    while GIF_MAX_TIME > next_rep_time as f32 / 1000. {
        kfs.push(Keyframe::new(next_rep_time as f32 / 1000., 1.0, 0));
        kfs.push(Keyframe::new(next_rep_time as f32 / 1000., 0.0, 2));
        next_rep_time += gif.total_time();
    }
    if kfs.is_empty() {
        return Ok(Anim::default());
    }
    Ok(Anim::new(kfs))
}

async fn parse_notes(
    r: &mut BpmList,
    rpe: Vec<RPENote>,
    fs: &mut dyn FileSystem,
    height: &mut AnimFloat,
    hitsounds: &mut HitSoundMap,
) -> Result<Vec<Note>> {
    let mut notes = Vec::new();
    for note in rpe {
        let time: f32 = r.time(&note.start_time);
        height.set_time(time);
        let note_height = height.now();
        let y_offset = note.y_offset * 2. / RPE_HEIGHT * note.speed;
        let kind = match note.kind {
            1 => NoteKind::Click,
            2 => {
                let end_time = r.time(&note.end_time);
                height.set_time(end_time);
                NoteKind::Hold {
                    end_time,
                    end_height: height.now(),
                }
            }
            3 => NoteKind::Flick,
            4 => NoteKind::Drag,
            _ => ptl!(bail "unknown-note-type", "type" => note.kind),
        };
        let hitsound = match note.hitsound {
            Some(s) => {
                // TODO: RPE doc needed...
                if s == "flick.mp3" {
                    HitSound::Flick
                } else if s == "tap.mp3" {
                    HitSound::Click
                } else if s == "drag.mp3" {
                    HitSound::Drag
                } else {
                    if hitsounds.get(&s).is_none() {
                        let data = fs.load_file(&s).await;
                        if let Ok(data) = data {
                            hitsounds.insert(s.clone(), AudioClip::new(data)?);
                        } else {
                            ptl!(bail "hitsound-missing", "name" => s);
                        }
                    }
                    HitSound::Custom(String::from_str(&s)?)
                }
            }
            None => HitSound::default_from_kind(&kind),
        };
        notes.push(Note {
            object: Object {
                alpha: if note.visible_time >= time {
                    if note.alpha >= 255 {
                        AnimFloat::default()
                    } else {
                        AnimFloat::fixed(note.alpha as f32 / 255.)
                    }
                } else {
                    let alpha = note.alpha.min(255) as f32 / 255.;
                    AnimFloat::new(vec![Keyframe::new(0.0, 0.0, 0), Keyframe::new(time - note.visible_time, alpha, 0)])
                },
                translation: AnimVector(AnimFloat::fixed(note.position_x / (RPE_WIDTH / 2.)), AnimFloat::fixed(y_offset)),
                scale: AnimVector(AnimFloat::fixed(note.size), AnimFloat::fixed(note.size)),
                ..Default::default()
            },
            kind,
            hitsound,
            time,
            height: note_height,
            speed: note.speed,

            above: note.above == 1,
            multiple_hint: false,
            fake: note.is_fake != 0,
            judge: JudgeStatus::NotJudged,
        })
    }
    Ok(notes)
}

fn parse_ctrl_events(rpe: &[RPECtrlEvent], key: &str) -> AnimFloat {
    let vals: Vec<_> = rpe.iter().map(|it| it.value[key]).collect();
    if rpe.is_empty() || (rpe.len() == 2 && rpe[0].easing == 1 && (vals[0] - 1.).abs() < 1e-4) {
        return AnimFloat::default();
    }
    // In RPE, each control event's easing governs the interval ending at that
    // event's x, not starting from it. The Anim system uses kf[i].tween for
    // the interval [kf[i], kf[i+1]], so we shift the tween assignment: each
    // keyframe gets the tween from the next event.
    let tweens: Vec<Rc<dyn TweenFunction>> = rpe
        .iter()
        .skip(1)
        .map(|it| StaticTween::get_rc(RPE_TWEEN_MAP.get(it.easing.max(1) as usize).copied().unwrap_or(RPE_TWEEN_MAP[0])))
        .chain(std::iter::once(StaticTween::get_rc(0)))
        .collect();
    AnimFloat::new(
        rpe.iter()
            .zip(vals)
            .zip(tweens)
            .map(|((it, val), tween)| Keyframe {
                time: it.x,
                value: val,
                tween,
            })
            .collect(),
    )
}

#[allow(clippy::too_many_arguments)]
async fn parse_judge_line(
    r: &mut BpmList,
    rpe: RPEJudgeLine,
    max_time: f32,
    speed_mode: SpeedEasingMode,
    fs: &mut dyn FileSystem,
    bezier_map: &BezierMap,
    hitsounds: &mut HitSoundMap,
    line_texture_map: &mut HashMap<String, SafeTexture>,
) -> Result<JudgeLine> {
    let event_layers: Vec<_> = rpe.event_layers.into_iter().flatten().collect();
    fn events_with_factor(
        r: &mut BpmList,
        event_layers: &[RPEEventLayer],
        get: impl Fn(&RPEEventLayer) -> &Option<Vec<RPEEvent>>,
        factor: f32,
        desc: &str,
        bezier_map: &BezierMap,
    ) -> Result<AnimFloat> {
        let anis: Vec<_> = event_layers
            .iter()
            .filter_map(|it| get(it).as_ref().map(|es| parse_events(r, es, None, bezier_map)))
            .collect::<Result<_>>()
            .with_context(|| ptl!("type-events-parse-failed", "type" => desc))?;
        let mut res = AnimFloat::chain(anis);
        if res.is_default() {
            return Ok(AnimFloat::fixed(0.0));
        }
        res.map_value(|v| v * factor);
        Ok(res)
    }
    let mut height = parse_speed_events(r, &event_layers, max_time, speed_mode)?;
    let mut notes = parse_notes(r, rpe.notes.unwrap_or_default(), fs, &mut height, hitsounds).await?;
    let cache = JudgeLineCache::new(&mut notes);
    Ok(JudgeLine {
        object: Object {
            alpha: events_with_factor(r, &event_layers, |it| &it.alpha_events, 1. / 255., "alpha", bezier_map)?,
            rotation: events_with_factor(r, &event_layers, |it| &it.rotate_events, -1., "rotate", bezier_map)?,
            translation: AnimVector(
                events_with_factor(r, &event_layers, |it| &it.move_x_events, 2. / RPE_WIDTH, "move X", bezier_map)?,
                events_with_factor(r, &event_layers, |it| &it.move_y_events, 2. / RPE_HEIGHT, "move Y", bezier_map)?,
            ),
            scale: {
                fn parse(r: &mut BpmList, opt: &Option<Vec<RPEEvent>>, factor: f32, bezier_map: &BezierMap) -> Result<AnimFloat> {
                    let mut res = opt
                        .as_ref()
                        .map(|it| parse_events(r, it, None, bezier_map))
                        .transpose()?
                        .unwrap_or_default();
                    res.map_value(|v| v * factor);
                    Ok(res)
                }
                let factor = if rpe.texture == "line.png" { 1. } else { 2. / RPE_WIDTH };
                rpe.extended
                    .as_ref()
                    .map(|e| -> Result<_> {
                        Ok(AnimVector(
                            parse(
                                r,
                                &e.scale_x_events,
                                factor
                                    * if rpe.texture == "line.png"
                                        && rpe
                                            .extended
                                            .as_ref()
                                            .and_then(|it| it.text_events.as_ref())
                                            .is_none_or(|it| it.is_empty())
                                        && rpe.attach_ui.is_none()
                                    {
                                        0.5
                                    } else {
                                        1.
                                    },
                                bezier_map,
                            )?,
                            parse(r, &e.scale_y_events, factor, bezier_map)?,
                        ))
                    })
                    .transpose()?
                    .unwrap_or_default()
            },
        },
        ctrl_obj: RefCell::new(CtrlObject {
            alpha: parse_ctrl_events(&rpe.alpha_control, "alpha"),
            size: parse_ctrl_events(&rpe.size_control, "size"),
            pos: parse_ctrl_events(&rpe.pos_control, "pos"),
            y: parse_ctrl_events(&rpe.y_control, "y"),
        }),
        height,
        incline: if let Some(events) = rpe.extended.as_ref().and_then(|e| e.incline_events.as_ref()) {
            parse_events(r, events, Some(0.), bezier_map).with_context(|| ptl!("incline-events-parse-failed"))?
        } else {
            AnimFloat::default()
        },
        notes,
        kind: if rpe.texture == "line.png" {
            if let Some(events) = rpe.extended.as_ref().and_then(|e| e.paint_events.as_ref()) {
                JudgeLineKind::Paint(
                    parse_events(r, events, Some(-1.), bezier_map).with_context(|| ptl!("paint-events-parse-failed"))?,
                    RefCell::default(),
                )
            } else if let Some(extended) = rpe.extended.as_ref() {
                if let Some(events) = extended.text_events.as_ref() {
                    JudgeLineKind::Text(parse_events(r, events, Some(String::new()), bezier_map).with_context(|| ptl!("text-events-parse-failed"))?)
                } else {
                    JudgeLineKind::Normal
                }
            } else {
                JudgeLineKind::Normal
            }
        } else if let Some(extended) = rpe.extended.as_ref() {
            if let Some(events) = extended.gif_events.as_ref() {
                let data = fs
                    .load_file(&rpe.texture)
                    .await
                    .with_context(|| ptl!("gif-load-failed", "path" => rpe.texture.clone()))?;
                let frames = GifFrames::new(
                    tokio::spawn(async move {
                        let decoder = gif::GifDecoder::new(Cursor::new(data))?;
                        debug!("decoding gif");
                        Ok::<std::vec::Vec<_>, ImageError>(decoder.into_frames().collect())
                    })
                    .into_future()
                    .await??
                    .into_iter()
                    .map(|frame| -> (u128, SafeTexture) {
                        let frame = frame.unwrap();
                        let delay: Duration = frame.delay().into();
                        (delay.as_millis(), SafeTexture::from(DynamicImage::ImageRgba8(frame.into_buffer())))
                    })
                    .collect(),
                );
                debug!("gif decoded");
                let events = parse_gif_events(r, events, bezier_map, &frames).with_context(|| ptl!("gif-events-parse-failed"))?;
                JudgeLineKind::TextureGif(events, frames, rpe.texture.clone())
            } else if let Some(texture) = line_texture_map.get(&rpe.texture) {
                debug!("texture {} reused, id: {}", rpe.texture.clone(), texture.clone().into_inner().raw_miniquad_texture_handle().gl_internal_id());
                JudgeLineKind::Texture(texture.clone(), rpe.texture.clone())
            } else {
                let texture = SafeTexture::from(image::load_from_memory(
                    &fs.load_file(&rpe.texture)
                        .await
                        .with_context(|| ptl!("illustration-load-failed", "path" => rpe.texture.clone()))?,
                )?)
                .with_mipmap();
                line_texture_map.insert(rpe.texture.clone(), texture.clone());
                JudgeLineKind::Texture(texture, rpe.texture.clone())
            }
        } else if let Some(texture) = line_texture_map.get(&rpe.texture) {
            debug!("texture {} reused, id: {}", rpe.texture.clone(), texture.clone().into_inner().raw_miniquad_texture_handle().gl_internal_id());
            JudgeLineKind::Texture(texture.clone(), rpe.texture.clone())
        } else {
            let texture = SafeTexture::from(image::load_from_memory(
                &fs.load_file(&rpe.texture)
                    .await
                    .with_context(|| ptl!("illustration-load-failed", "path" => rpe.texture.clone()))?,
            )?)
            .with_mipmap();
            line_texture_map.insert(rpe.texture.clone(), texture.clone());
            JudgeLineKind::Texture(texture, rpe.texture.clone())
        },
        color: if let Some(events) = rpe.extended.as_ref().and_then(|e| e.color_events.as_ref()) {
            parse_events(r, events, Some(WHITE), bezier_map).with_context(|| ptl!("color-events-parse-failed"))?
        } else {
            Anim::default()
        },
        parent: {
            let parent = rpe.parent.unwrap_or(-1);
            if parent == -1 {
                None
            } else {
                Some(parent as usize)
            }
        },
        rot_with_parent: rpe.rotate_with_father.unwrap_or(false),
        z_index: rpe.z_order,
        show_below: rpe.is_cover != 1,
        attach_ui: rpe.attach_ui,

        cache,
    })
}

fn add_bezier<T>(map: &mut BezierMap, event: &RPEEvent<T>) {
    if event.bezier != 0 {
        let p = &event.bezier_points;
        let int = |p: f32| (p * 100.).round() as i16;
        map.entry(((int(p[0]) * 100 + int(p[1])) as u16, int(p[2]), int(p[3])))
            .or_insert_with(|| Rc::new(BezierTween::new((p[0], p[1]), (p[2], p[3]))));
    }
}

macro_rules! process_bezier {
    ($event_layer:expr, $map:expr, $($field:ident),*) => {
        $(
            for event in $event_layer.$field.iter().flatten() {
                add_bezier($map, event);
            }
        )*
    };
}

fn get_bezier_map(rpe: &RPEChart) -> BezierMap {
    let mut map = HashMap::new();
    for line in &rpe.judge_line_list {
        for event_layer in line.event_layers.iter().flatten() {
            process_bezier!(event_layer, &mut map, alpha_events, move_x_events, move_y_events, rotate_events);
        }
        if let Some(ext_layer) = &line.extended {
            process_bezier!(ext_layer, &mut map, paint_events, scale_x_events, scale_y_events, gif_events, incline_events, text_events, color_events);
        }
    }
    map
}

pub async fn parse_rpe(source: &str, fs: &mut dyn FileSystem, extra: ChartExtra) -> Result<Chart> {
    let rpe: RPEChart = serde_json::from_str(source).with_context(|| ptl!("json-parse-failed"))?;
    let speed_mode = if rpe.meta.rpe_version >= 170 {
        SpeedEasingMode::Modern
    } else {
        SpeedEasingMode::Legacy
    };
    let bezier_map = get_bezier_map(&rpe);
    let mut r = BpmList::new(rpe.bpm_list.into_iter().map(|it| (it.start_time.beats(), it.bpm)).collect());
    fn vec<T>(v: &Option<Vec<T>>) -> impl Iterator<Item = &T> {
        v.iter().flat_map(|it| it.iter())
    }
    let mut hitsounds = HashMap::new();
    #[rustfmt::skip]
    let max_time = *rpe
        .judge_line_list
        .iter()
        .map(|line| {
            line.notes.as_ref().map(|notes| {
                notes
                    .iter()
                    .map(|note| r.time(&note.end_time).not_nan())
                    .max()
                    .unwrap_or_default()
            }).unwrap_or_default().max(
                line.event_layers.iter().filter_map(|it| it.as_ref().map(|layer| {
                    vec(&layer.alpha_events)
                        .chain(vec(&layer.move_x_events))
                        .chain(vec(&layer.move_y_events))
                        .chain(vec(&layer.rotate_events))
                        .map(|it| r.time(&it.end_time).not_nan())
                        .max().unwrap_or_default()
                })).max().unwrap_or_default()
            ).max(
                line.extended.as_ref().map(|e| {
                    vec(&e.scale_x_events)
                        .chain(vec(&e.scale_y_events))
                        .map(|it| r.time(&it.end_time).not_nan())
                        .max().unwrap_or_default()
                        .max(vec(&e.text_events).map(|it| r.time(&it.end_time).not_nan()).max().unwrap_or_default())
                }).unwrap_or_default()
            )
        })
        .max().unwrap_or_default() + 1.;
    // don't want to add a whole crate for a mere join_all...
    let mut lines = Vec::new();
    let mut line_texture_map = HashMap::new();
    for (id, rpe) in rpe.judge_line_list.into_iter().enumerate() {
        let name = rpe.name.clone();
        lines.push(
            parse_judge_line(&mut r, rpe, max_time, speed_mode, fs, &bezier_map, &mut hitsounds, &mut line_texture_map)
                .await
                .with_context(move || ptl!("judge-line-location-name", "jlid" => id, "name" => name))?,
        );
    }
    fn has_cycle(line: &JudgeLine, lines: &[JudgeLine], visited: &mut Vec<usize>) -> Option<usize> {
        if let Some(parent_index) = line.parent {
            if visited.contains(&parent_index) {
                return Some(parent_index);
            }
            visited.push(parent_index);
            return has_cycle(&lines[parent_index], lines, visited);
        }
        None
    }
    for (i, line) in lines.iter().enumerate() {
        let mut vec = Vec::new();
        vec.push(i);
        if let Some(line) = has_cycle(line, &lines, &mut vec) {
            ptl!(bail "found infinite recursive parent relations", "line" => line)
        }
    }
    process_lines(&mut lines);
    Ok(Chart::new(rpe.meta.offset as f32 / 1000.0, lines, r, ChartSettings::default(), extra, hitsounds))
}
