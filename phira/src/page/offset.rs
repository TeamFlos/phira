prpr_l10n::tl_file!("cali");

use std::borrow::Cow;

use super::{Page, SharedState};
use crate::{get_data, get_data_mut, save_data};
use anyhow::{Context, Result};
use macroquad::prelude::*;
use prpr::{
    core::{ParticleEmitter, ResourcePack, NOTE_WIDTH_RATIO_BASE},
    ext::{create_audio_manger, semi_black, RectExt, SafeTexture, ScaleType},
    time::TimeManager,
    ui::{Slider, Ui},
};
use sasa::{AudioClip, AudioManager, Music, MusicParams, PlaySfxParams, Sfx};

pub struct OffsetPage {
    _audio: AudioManager,
    cali: Music,
    cali_hit: Sfx,

    tm: TimeManager,
    cali_last: bool,

    click: SafeTexture,
    _hit_fx: SafeTexture,
    emitter: ParticleEmitter,
    color: Color,

    slider: Slider,

    touched: bool,
    touch: Option<(f32, f32)>,
}

impl OffsetPage {
    const FADE_TIME: f32 = 0.8;

    pub async fn new() -> Result<Self> {
        let mut audio = create_audio_manger(&get_data().config)?;
        let cali = audio.create_music(
            AudioClip::new(load_file("cali.ogg").await?)?,
            MusicParams {
                loop_mix_time: 0.,
                ..Default::default()
            },
        )?;
        let cali_hit = audio.create_sfx(AudioClip::new(load_file("cali_hit.ogg").await?)?, None)?;

        let mut tm = TimeManager::new(1., true);
        tm.force = 3e-2;

        let respack = ResourcePack::from_path(get_data().config.res_pack_path.as_ref())
            .await
            .context("Failed to load resource pack")?;
        let click = respack.note_style.click.clone();
        let emitter = ParticleEmitter::new(&respack, get_data().config.note_scale, respack.info.hide_particles)?;
        Ok(Self {
            _audio: audio,
            cali,
            cali_hit,

            tm,
            cali_last: false,

            click,
            _hit_fx: respack.hit_fx,
            emitter,
            color: respack.info.fx_perfect(),

            slider: Slider::new(-500.0..500.0, 5.),

            touched: false,
            touch: None,
        })
    }
}

impl Page for OffsetPage {
    fn can_play_bgm(&self) -> bool {
        false
    }

    fn label(&self) -> Cow<'static, str> {
        tl!("label")
    }

    fn exit(&mut self) -> Result<()> {
        save_data()?;
        Ok(())
    }

    fn enter(&mut self, _s: &mut SharedState) -> Result<()> {
        self.cali.seek_to(0.)?;
        self.cali.play()?;
        self.tm.reset();
        Ok(())
    }

    fn pause(&mut self) -> Result<()> {
        save_data()?;
        self.tm.pause();
        self.cali.pause()?;
        Ok(())
    }

    fn resume(&mut self) -> Result<()> {
        self.tm.resume();
        self.cali.play()?;
        Ok(())
    }

    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        let t = s.t;
        let config = &mut get_data_mut().config;
        let mut offset = config.offset * 1000.;
        if self.slider.touch(touch, t, &mut offset).is_some() {
            config.offset = offset / 1000.;
            return Ok(true);
        }
        if touch.phase == TouchPhase::Started && touch.position.x < 0. {
            self.touched = true;
        }
        Ok(false)
    }

    fn update(&mut self, _s: &mut SharedState) -> Result<()> {
        if !self.cali.paused() {
            let pos = self.cali.position() as f64;
            let now = self.tm.now();
            if now > 2. {
                self.tm.seek_to(now - 2.);
                self.tm.dont_wait();
            }
            let now = self.tm.now();
            if now - pos >= -1. {
                self.tm.update(pos);
            }
        }
        Ok(())
    }

    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        s.render_fader(ui, |ui| {
            let lf = -0.92;
            let mut r = ui.content_rect();
            r.w += r.x - lf;
            r.x = lf;
            ui.fill_path(&r.rounded(0.02), semi_black(0.4));

            let ct = (-0.4, r.bottom() - 0.12);
            let hw = 0.4;
            let hh = 0.005;
            ui.fill_rect(Rect::new(ct.0 - hw, ct.1 - hh, hw * 2., hh * 2.), WHITE);

            let ot = t;

            let config = &get_data().config;
            let mut t = self.tm.now() as f32 - config.offset;
            if t < 0. {
                t += 2.;
            }
            if t >= 2. {
                t -= 2.;
            }
            let ny = ct.1 + (t - 1.) * 0.6;
            if self.touched {
                self.touch = Some((ot, ny));
                self.touched = false;
            }
            if t <= 1. {
                let w = NOTE_WIDTH_RATIO_BASE * config.note_scale * 2.;
                let h = w * self.click.height() / self.click.width();
                let r = Rect::new(ct.0 - w / 2., ny, w, h);
                ui.fill_rect(r, (*self.click, r, ScaleType::Fit));
                self.cali_last = true;
            } else {
                if self.cali_last {
                    let g = ui.to_global(ct);
                    self.emitter.emit_at(vec2(g.0, g.1), 0., self.color);
                    let _ = self.cali_hit.play(PlaySfxParams::default());
                }
                self.cali_last = false;
            }

            if let Some((time, pos)) = &self.touch {
                let p = (ot - time) / Self::FADE_TIME;
                if p > 1. {
                    self.touch = None;
                } else {
                    let p = p.max(0.);
                    let c = Color {
                        a: (if p <= 0.5 { 1. } else { (1. - p) * 2. }) * self.color.a,
                        ..self.color
                    };
                    ui.fill_rect(Rect::new(ct.0 - hw, pos - hh, hw * 2., hh * 2.), c);
                }
            }

            let offset = config.offset * 1000.;
            self.slider
                .render(ui, Rect::new(0.46, -0.1, 0.45, 0.2), ot, offset, format!("{offset:.0}ms"));
        });

        self.emitter.draw(get_frame_time());

        Ok(())
    }
}
