prpr_l10n::tl_file!("parser" ptl);

use super::RPE_TWEEN_MAP;
#[cfg(feature = "video")]
use crate::core::Video;
use crate::{
    core::{Anim, BpmList, ChartExtra, ClampedTween, Effect, Keyframe, StaticTween, Triple, Tweenable, Uniform, EPS},
    ext::ScaleType,
    fs::FileSystem,
};
use anyhow::{Context, Result};
use macroquad::prelude::{Color, Vec2};
use serde::Deserialize;
use std::{collections::HashMap, rc::Rc};

// serde is weird...
fn f32_zero() -> f32 {
    0.
}

fn f32_one() -> f32 {
    1.
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExtKeyframe<T> {
    #[serde(default = "f32_zero")]
    easing_left: f32,
    #[serde(default = "f32_one")]
    easing_right: f32,
    easing_type: i32,
    start: T,
    end: T,
    start_time: Triple,
    end_time: Triple,
}

#[derive(Default, Deserialize)]
#[serde(untagged)]
enum ExtAnim<V> {
    #[default]
    Default,
    Fixed(V),
    Keyframes(Vec<ExtKeyframe<V>>),
}

impl<V> ExtAnim<V> {
    fn into<T: Tweenable>(self, r: &mut BpmList, default: Option<T>) -> Anim<T>
    where
        V: Into<T>,
    {
        match self {
            ExtAnim::Default => Anim::default(),
            ExtAnim::Fixed(value) => Anim::fixed(value.into()),
            ExtAnim::Keyframes(events) => {
                let mut kfs = Vec::new();
                if let Some(default) = default {
                    if events[0].start_time.beats() != 0.0 {
                        kfs.push(Keyframe::new(0.0, default, 0));
                    }
                }
                for e in events {
                    kfs.push(Keyframe {
                        time: r.time(&e.start_time),
                        value: e.start.into(),
                        tween: {
                            let tween = RPE_TWEEN_MAP.get(e.easing_type.max(1) as usize).copied().unwrap_or(RPE_TWEEN_MAP[0]);
                            if e.easing_left.abs() < EPS && (e.easing_right - 1.0).abs() < EPS {
                                StaticTween::get_rc(tween)
                            } else {
                                Rc::new(ClampedTween::new(tween, e.easing_left..e.easing_right))
                            }
                        },
                    });
                    kfs.push(Keyframe::new(r.time(&e.end_time), e.end.into(), 0));
                }
                Anim::new(kfs)
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExtBpmItem {
    time: Triple,
    bpm: f32,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum BpmForm {
    Single(f32),
    List(Vec<ExtBpmItem>),
}

impl From<BpmForm> for BpmList {
    fn from(value: BpmForm) -> Self {
        match value {
            BpmForm::Single(value) => BpmList::new(vec![(0., value)]),
            BpmForm::List(list) => BpmList::new(list.into_iter().map(|it| (it.time.beats(), it.bpm)).collect()),
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Variable {
    Float(ExtAnim<f32>),
    Vec2(ExtAnim<(f32, f32)>),
    Color(ExtAnim<[u8; 4]>),
}

#[derive(Deserialize)]
struct ExtEffect {
    start: Triple,
    end: Triple,
    shader: String,
    #[serde(default)]
    vars: HashMap<String, Variable>,
    #[serde(default)]
    global: bool,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct ExtVideo {
    path: String,
    #[serde(default)]
    time: Triple,
    #[serde(default)]
    scale: ScaleType,
    #[serde(default)]
    alpha: ExtAnim<f32>,
    #[serde(default)]
    dim: ExtAnim<f32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Extra {
    bpm: BpmForm,
    #[serde(default)]
    effects: Vec<ExtEffect>,
    #[serde(default)]
    videos: Vec<ExtVideo>,
}

async fn parse_effect(r: &mut BpmList, rpe: ExtEffect, fs: &mut dyn FileSystem) -> Result<Effect> {
    let range = r.time(&rpe.start)..r.time(&rpe.end);
    let vars = rpe
        .vars
        .into_iter()
        .map(|(name, var)| -> Result<Box<dyn Uniform>> {
            Ok(match var {
                Variable::Float(events) => Box::new((name, events.into::<f32>(r, None))),
                Variable::Vec2(events) => Box::new((name, events.into::<Vec2>(r, None))),
                Variable::Color(events) => Box::new((name, events.into::<Color>(r, None))),
            })
        })
        .collect::<Result<_>>()?;
    let string;
    Effect::new(
        range,
        if let Some(path) = rpe.shader.strip_prefix('/') {
            string = String::from_utf8(fs.load_file(path).await?).with_context(|| ptl!("shader-load-failed", "path" => path))?;
            &string
        } else {
            Effect::get_preset(&rpe.shader).ok_or_else(|| ptl!(err "shader-not-found", "shader" => rpe.shader))?
        },
        vars,
        rpe.global,
    )
}

pub async fn parse_extra(source: &str, fs: &mut dyn FileSystem) -> Result<ChartExtra> {
    let ext: Extra = serde_json::from_str(source).with_context(|| ptl!("json-parse-failed"))?;
    let mut r: BpmList = ext.bpm.into();
    let mut effects = Vec::new();
    let mut global_effects = Vec::new();
    for (id, effect) in ext.effects.into_iter().enumerate() {
        (if effect.global { &mut global_effects } else { &mut effects }).push(
            parse_effect(&mut r, effect, fs)
                .await
                .with_context(|| ptl!("effect-location", "id" => id))?,
        );
    }
    #[cfg(feature = "video")]
    let mut videos = Vec::new();
    #[cfg(feature = "video")]
    for video in ext.videos {
        videos.push(
            Video::new(
                fs.load_file(&video.path)
                    .await
                    .with_context(|| ptl!("video-load-failed", "path" => video.path.clone()))?,
                r.time(&video.time),
                video.scale,
                video.alpha.into(&mut r, Some(1.)),
                video.dim.into(&mut r, Some(0.)),
            )
            .with_context(|| ptl!("video-load-failed", "path" => video.path))?,
        );
    }
    #[cfg(not(feature = "video"))]
    if !ext.videos.is_empty() {
        tracing::warn!("video is disabled in this build");
    }
    Ok(ChartExtra {
        effects,
        global_effects,
        #[cfg(feature = "video")]
        videos,
    })
}
