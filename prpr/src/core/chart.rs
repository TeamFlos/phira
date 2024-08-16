use super::{BpmList, Effect, JudgeLine, JudgeLineKind, Matrix, Resource, UIElement, Vector};
use crate::{fs::FileSystem, judge::JudgeStatus, ui::Ui};
use anyhow::{Context, Result};
use macroquad::prelude::*;
use sasa::AudioClip;
use std::{cell::RefCell, collections::HashMap};
use tracing::warn;

#[derive(Default)]
pub struct ChartExtra {
    pub effects: Vec<Effect>,
    pub global_effects: Vec<Effect>,
    #[cfg(feature = "video")]
    pub videos: Vec<super::Video>,
}

#[derive(Default)]
pub struct ChartSettings {
    pub pe_alpha_extension: bool,
    pub hold_partial_cover: bool,
}

pub type HitSoundMap = HashMap<String, AudioClip>;

pub struct Chart {
    pub offset: f32,
    pub lines: Vec<JudgeLine>,
    pub bpm_list: RefCell<BpmList>,

    pub settings: ChartSettings,
    pub extra: ChartExtra,

    /// Line order according to z-index, lines with attach_ui will be removed from this list
    ///
    /// Store the index of the line in z-index ascending order
    pub order: Vec<usize>,
    /// TODO: docs from RPE
    pub attach_ui: [Option<usize>; 7],

    pub hitsounds: HitSoundMap,
}

impl Chart {
    pub fn new(offset: f32, lines: Vec<JudgeLine>, bpm_list: BpmList, settings: ChartSettings, extra: ChartExtra, hitsounds: HitSoundMap) -> Self {
        let mut attach_ui = [None; 7];
        let mut order = (0..lines.len())
            .filter(|it| {
                if let Some(element) = lines[*it].attach_ui {
                    attach_ui[element as usize - 1] = Some(*it);
                    false
                } else {
                    true
                }
            })
            .collect::<Vec<_>>();
        order.sort_by_key(|it| (lines[*it].z_index, *it));
        Self {
            offset,
            lines,
            bpm_list: RefCell::new(bpm_list),
            settings,
            extra,

            order,
            attach_ui,

            hitsounds,
        }
    }

    #[inline]
    pub fn with_element<R>(&self, ui: &mut Ui, res: &Resource, element: UIElement, ct: Option<(f32, f32)>, f: impl FnOnce(&mut Ui, Color) -> R) -> R {
        if let Some(id) = self.attach_ui[element as usize - 1] {
            let obj = &self.lines[id].object;
            let mut tr = obj.now_translation(res);
            tr.y = -tr.y;
            let color = self.lines[id].color.now_opt().unwrap_or(WHITE);
            let scale = obj.now_scale(ct.map_or_else(|| Vector::default(), |(x, y)| Vector::new(x, y)));
            ui.with(obj.now_rotation().append_translation(&tr) * scale, |ui| ui.alpha(obj.now_alpha().max(0.), |ui| f(ui, color)))
        } else {
            f(ui, WHITE)
        }
    }

    pub async fn load_textures(&mut self, fs: &mut dyn FileSystem) -> Result<()> {
        for line in &mut self.lines {
            if let JudgeLineKind::Texture(tex, path) = &mut line.kind {
                *tex = image::load_from_memory(&fs.load_file(path).await.with_context(|| format!("failed to load illustration {path}"))?)?.into();
            }
        }
        Ok(())
    }

    pub fn reset(&mut self) {
        self.lines
            .iter_mut()
            .flat_map(|it| it.notes.iter_mut())
            .for_each(|note| note.judge = JudgeStatus::NotJudged);
        for line in &mut self.lines {
            line.cache.reset(&mut line.notes);
        }
    }

    pub fn update(&mut self, res: &mut Resource) {
        for line in &mut self.lines {
            line.object.set_time(res.time);
        }
        // TODO optimize
        let trs = self.lines.iter().map(|it| it.now_transform(res, &self.lines)).collect::<Vec<_>>();
        for (line, tr) in self.lines.iter_mut().zip(trs) {
            line.update(res, tr);
        }
        for effect in &mut self.extra.effects {
            effect.update(res);
        }
        #[cfg(feature = "video")]
        for video in &mut self.extra.videos {
            if let Err(err) = video.update(res.time) {
                warn!("video error: {err:?}");
            }
        }
    }

    pub fn render(&self, ui: &mut Ui, res: &mut Resource) {
        #[cfg(feature = "video")]
        for video in &self.extra.videos {
            video.render(res.time, res.aspect_ratio);
        }
        res.apply_model_of(&Matrix::identity().append_nonuniform_scaling(&Vector::new(if res.config.flip_x() { -1. } else { 1. }, -1.)), |res| {
            let mut guard = self.bpm_list.borrow_mut();
            for id in &self.order {
                self.lines[*id].render(ui, res, &self.lines, &mut guard, &self.settings, *id);
            }
            drop(guard);
            res.note_buffer.borrow_mut().draw_all();
            if res.config.sample_count > 1 {
                unsafe { get_internal_gl() }.flush();
                if let Some(target) = &res.chart_target {
                    target.blit();
                }
            }
            if !res.no_effect {
                for effect in &self.extra.effects {
                    effect.render(res);
                }
            }
        });
    }
}
