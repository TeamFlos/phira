use macroquad::prelude::{vec2, Color, Rect, Vec2};
use once_cell::sync::Lazy;
use std::{any::Any, ops::Range, rc::Rc};

pub type TweenId = u8;

const PI: f32 = std::f32::consts::PI;

macro_rules! f1 {
    ($fn:ident) => {
        $fn
    };
}

macro_rules! f2 {
    ($fn:ident) => {
        |x| (1. - $fn(1. - x))
    };
}

macro_rules! f3 {
    ($fn:ident) => {
        |x| {
            let x = x * 2.;
            if x < 1. {
                $fn(x) / 2.
            } else {
                1. - $fn(2. - x) / 2.
            }
        }
    };
}

#[inline]
fn sine(x: f32) -> f32 {
    1. - ((x * PI) / 2.).cos()
}

#[inline]
fn quad(x: f32) -> f32 {
    x * x
}

#[inline]
fn cubic(x: f32) -> f32 {
    x * x * x
}

#[inline]
fn quart(x: f32) -> f32 {
    x * x * x * x
}

#[inline]
fn quint(x: f32) -> f32 {
    x * x * x * x * x
}

#[inline]
fn expo(x: f32) -> f32 {
    (2.0_f32).powf(10. * (x - 1.))
}

#[inline]
fn circ(x: f32) -> f32 {
    1. - (1. - x * x).sqrt()
}

#[inline]
fn back(x: f32) -> f32 {
    const C1: f32 = 1.70158;
    const C3: f32 = C1 + 1.;
    (C3 * x - C1) * x * x
}

#[inline]
fn elastic(x: f32) -> f32 {
    const C4: f32 = (2. * PI) / 3.;
    -((2.0_f32).powf(10. * x - 10.) * ((x * 10. - 10.75) * C4).sin())
}

#[inline]
fn bounce(x: f32) -> f32 {
    const N1: f32 = 7.5625;
    const D1: f32 = 2.75;

    let x = 1. - x;
    1. - (if x < 1. / D1 {
        N1 * x.powi(2)
    } else if x < 2. / D1 {
        N1 * (x - 1.5 / D1).powi(2) + 0.75
    } else if x < 2.5 / D1 {
        N1 * (x - 2.25 / D1).powi(2) + 0.9375
    } else {
        N1 * (x - 2.625 / D1).powi(2) + 0.984375
    })
}

#[rustfmt::skip]
pub static TWEEN_FUNCTIONS: [fn(f32) -> f32; 33] = [
	|_| 0.,			|_| 1.,			|x| x,
	/* In */		/* Out */		/* InOut */
	f1!(sine),		f2!(sine),		f3!(sine),
	f1!(quad),		f2!(quad),		f3!(quad),
	f1!(cubic),		f2!(cubic),		f3!(cubic),
	f1!(quart),		f2!(quart),		f3!(quart),
	f1!(quint),		f2!(quint),		f3!(quint),
	f1!(expo),		f2!(expo),		f3!(expo),
	f1!(circ),		f2!(circ),		f3!(circ),
	f1!(back),		f2!(back),		f3!(back),
	f1!(elastic),	f2!(elastic),	f3!(elastic),
	f1!(bounce),	f2!(bounce),	f3!(bounce),
];

thread_local! {
    static TWEEN_FUNCTION_RCS: Lazy<Vec<Rc<dyn TweenFunction>>> = Lazy::new(|| {
        (0..33)
            .map(|it| -> Rc<dyn TweenFunction> { Rc::new(StaticTween(it)) })
            .collect()
    });
}

pub trait TweenFunction {
    fn y(&self, x: f32) -> f32;
    fn as_any(&self) -> &dyn Any;
}

pub struct StaticTween(pub TweenId);
impl TweenFunction for StaticTween {
    fn y(&self, x: f32) -> f32 {
        TWEEN_FUNCTIONS[self.0 as usize](x)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl StaticTween {
    pub fn get_rc(tween: TweenId) -> Rc<dyn TweenFunction> {
        TWEEN_FUNCTION_RCS.with(|rcs| Rc::clone(&rcs[tween as usize]))
    }
}

// TODO assuming monotone, but actually they're not (e.g. Back tween)
pub struct ClampedTween(pub TweenId, pub Range<f32>, pub Range<f32>);
impl TweenFunction for ClampedTween {
    fn y(&self, x: f32) -> f32 {
        (TWEEN_FUNCTIONS[self.0 as usize](f32::tween(&self.1.start, &self.1.end, x)) - self.2.start) / (self.2.end - self.2.start)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ClampedTween {
    pub fn new(tween: TweenId, range: Range<f32>) -> Self {
        let f = TWEEN_FUNCTIONS[tween as usize];
        let y_range = f(range.start)..f(range.end);
        Self(tween, range, y_range)
    }
}

// https://github.com/gre/bezier-easing

const SAMPLE_TABLE_SIZE: usize = 21;
const SAMPLE_STEP: f32 = 1. / (SAMPLE_TABLE_SIZE - 1) as f32;
const NEWTON_MIN_STEP: f32 = 1e-3;
const NEWTON_ITERATIONS: usize = 4;
const SUBDIVISION_PRECISION: f32 = 1e-7;
const SUBDIVISION_MAX_ITERATION: usize = 10;
const SLOPE_EPS: f32 = 1e-7;

pub struct BezierTween {
    sample_table: [f32; SAMPLE_TABLE_SIZE],
    pub p1: (f32, f32),
    pub p2: (f32, f32),
}

impl TweenFunction for BezierTween {
    fn y(&self, x: f32) -> f32 {
        Self::sample(self.p1.1, self.p2.1, self.t_for_x(x))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl BezierTween {
    #[inline]
    fn coefficients(x1: f32, x2: f32) -> (f32, f32, f32) {
        ((x1 - x2) * 3. + 1., x2 * 3. - x1 * 6., x1 * 3.)
    }

    #[inline]
    fn sample(x1: f32, x2: f32, t: f32) -> f32 {
        let (a, b, c) = Self::coefficients(x1, x2);
        ((a * t + b) * t + c) * t
    }
    #[inline]
    fn slope(x1: f32, x2: f32, t: f32) -> f32 {
        let (a, b, c) = Self::coefficients(x1, x2);
        (a * 3. * t + b * 2.) * t + c
    }

    fn newton_raphson_iterate(x: f32, mut t: f32, x1: f32, x2: f32) -> f32 {
        for _ in 0..NEWTON_ITERATIONS {
            let slope = Self::slope(x1, x2, t);
            if slope <= SLOPE_EPS {
                return t;
            }
            let diff = Self::sample(x1, x2, t) - x;
            t -= diff / slope;
        }
        t
    }

    fn binary_subdivide(x: f32, mut l: f32, mut r: f32, x1: f32, x2: f32) -> f32 {
        let mut t = (l + r) / 2.;
        for _ in 0..SUBDIVISION_MAX_ITERATION {
            let diff = Self::sample(x1, x2, t) - x;
            if diff.abs() <= SUBDIVISION_PRECISION {
                break;
            }
            if diff > 0. {
                r = t;
            } else {
                l = t;
            }
            t = (l + r) / 2.;
        }
        t
    }

    pub fn t_for_x(&self, x: f32) -> f32 {
        if x == 0. || x == 1. {
            return x;
        }
        let id = (x / SAMPLE_STEP) as usize;
        let id = id.min(SAMPLE_TABLE_SIZE - 1);
        let dist = (x - self.sample_table[id]) / (self.sample_table[id + 1] - self.sample_table[id]);
        let init_t = SAMPLE_STEP * (id as f32 + dist);
        match Self::slope(self.p1.0, self.p2.0, init_t) {
            y if y <= SLOPE_EPS => init_t,
            y if y >= NEWTON_MIN_STEP => Self::newton_raphson_iterate(x, init_t, self.p1.0, self.p2.0),
            _ => Self::binary_subdivide(x, SAMPLE_STEP * id as f32, SAMPLE_STEP * (id + 1) as f32, self.p1.0, self.p2.0),
        }
    }

    pub fn new(p1: (f32, f32), p2: (f32, f32)) -> Self {
        Self {
            sample_table: std::array::from_fn(|i| Self::sample(p1.0, p2.0, i as f32 * SAMPLE_STEP)),
            p1,
            p2,
        }
    }
}

#[repr(u8)]
pub enum TweenMajor {
    Plain,
    Sine,
    Quad,
    Cubic,
    Quart,
    Quint,
    Expo,
    Circ,
    Back,
    Elastic,
    Bounce,
}

#[repr(u8)]
pub enum TweenMinor {
    In,
    Out,
    InOut,
}

pub const fn easing_from(major: TweenMajor, minor: TweenMinor) -> TweenId {
    major as u8 * 3 + minor as u8
}

pub trait Tweenable: Clone {
    fn tween(x: &Self, y: &Self, t: f32) -> Self;
    fn add(_x: &Self, _y: &Self) -> Self {
        unimplemented!()
    }
}

impl Tweenable for f32 {
    fn tween(x: &Self, y: &Self, t: f32) -> Self {
        x + (y - x) * t
    }

    fn add(x: &Self, y: &Self) -> Self {
        x + y
    }
}

impl Tweenable for Vec2 {
    fn tween(x: &Self, y: &Self, t: f32) -> Self {
        vec2(f32::tween(&x.x, &y.x, t), f32::tween(&x.y, &y.y, t))
    }

    fn add(x: &Self, y: &Self) -> Self {
        vec2(x.x + y.x, x.y + y.y)
    }
}

impl Tweenable for Color {
    fn tween(x: &Self, y: &Self, t: f32) -> Self {
        Self::new(f32::tween(&x.r, &y.r, t), f32::tween(&x.g, &y.g, t), f32::tween(&x.b, &y.b, t), f32::tween(&x.a, &y.a, t))
    }
}

impl Tweenable for String {
    fn tween(x: &Self, y: &Self, t: f32) -> Self {
        if x.contains("%P%") && y.contains("%P%") {
            let x = x.replace("%P%", "");
            let y = y.replace("%P%", "");
            if t >= 1. {
                y
            } else if t <= 0. {
                x
            } else {
                let x: f32 = x.parse().unwrap_or(0.0);
                let y: f32 = y.parse().unwrap_or(0.0);
                let value = x + t * (y - x);
                if x.fract() == 0.0 && y.fract() == 0.0 {
                    format!("{:.0}", value)
                } else {
                    format!("{:.3}", value)
                }
            }
        } else {
            if x.is_empty() && y.is_empty() {
                Self::new()
            } else if y.is_empty() {
                let x = if x.contains("%P%") {
                    x.replace("%P%", "")
                } else {
                    x.to_string()
                };
                Self::tween(y, &x, 1. - t)
            } else if x.is_empty() {
                let chars = y.chars().collect::<Vec<_>>();
                chars[..(t * chars.len() as f32).round() as usize].iter().collect()
            } else {
                let x_len = x.chars().count();
                let y_len = y.chars().count();
                if x.chars().zip(y.chars()).take(x_len).all(|(xc, yc)| xc == yc) {
                    let take_num = ((y_len - x_len) as f32 * t).round() as usize + x_len;
                    let mut text = x.clone();
                    text.push_str(&y.chars().skip(x_len).take(take_num - x_len).collect::<String>());
                    text
                } else {
                    if x.contains("%P%") {
                        x.replace("%P%", "")
                    } else {
                        x.clone()
                    }
                }
            }
        }
    }
}

impl Tweenable for Rect {
    fn tween(x: &Self, y: &Self, t: f32) -> Self {
        Self::new(f32::tween(&x.x, &y.x, t), f32::tween(&x.y, &y.y, t), f32::tween(&x.w, &y.w, t), f32::tween(&x.h, &y.h, t))
    }
}
