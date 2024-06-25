use anyhow::{bail, Result};
use macroquad::prelude::*;
use prpr::{
    config::Config,
    core::{demux_audio, Anim, Keyframe, Video},
    ext::{create_audio_manger, semi_black, semi_white, SafeTexture, ScaleType},
    fs::FileSystem,
    info::ChartInfo,
    scene::{BasicPlayer, GameMode, LoadingScene, NextScene, Scene, UpdateFn, UploadFn},
    time::TimeManager,
    ui::LoadingParams,
};
use sasa::{AudioClip, AudioManager, Music, MusicParams};

enum State {
    Before,
    Playing,
    Blanking,
    Loading,
    Transforming,
}

struct Bgm {
    audio_manager: AudioManager,
    music: Music,
}

pub struct UnlockScene {
    loading_scene: Box<LoadingScene>,
    game_scene: Option<Box<dyn Scene>>,
    next_scene: Option<NextScene>,

    render_target: Option<RenderTarget>,
    video: Video,
    bgm: Option<Bgm>,
    music_length: f32,

    background: SafeTexture,

    state: State,
}

impl UnlockScene {
    pub async fn new(
        mode: GameMode,
        info: ChartInfo,
        config: Config,
        mut fs: Box<dyn FileSystem>,
        player: Option<BasicPlayer>,
        upload_fn: Option<UploadFn>,
        update_fn: Option<UpdateFn>,
        preloaded: Option<(prpr::ext::SafeTexture, prpr::ext::SafeTexture, Color)>,
    ) -> Result<UnlockScene> {
        let bytes = fs
            .load_file(&info.unlock_video.clone().unwrap_or_else(|| "unlock.mp4".to_owned()))
            .await?;
        let video = Video::new(bytes, 0., ScaleType::Inside, Anim::new(vec![Keyframe::new(0., 1., 0)]), Anim::default())?;
        let clip = demux_audio(video.video_file().path().to_str().unwrap())?;
        let music_length = clip.as_ref().map_or(0., AudioClip::length);

        let bgm = match clip {
            Some(clip) => {
                let mut audio_manager = create_audio_manger(&config)?;
                let music = audio_manager.create_music(
                    clip,
                    MusicParams {
                        amplifier: config.volume_music,
                        ..Default::default()
                    },
                )?;
                Some(Bgm { audio_manager, music })
            }
            None => None,
        };

        let (_, background, _) = preloaded.clone().unwrap_or(LoadingScene::load(&mut *fs, &info.illustration).await?);
        let loading_scene = Box::new(LoadingScene::new(mode, info, config, fs, player, upload_fn, update_fn, preloaded).await?);

        Ok(UnlockScene {
            loading_scene,
            next_scene: None,
            game_scene: None,

            render_target: None,
            video,
            bgm,
            music_length,

            background,

            state: State::Before,
        })
    }
}

impl Scene for UnlockScene {
    fn enter(&mut self, tm: &mut TimeManager, target: Option<RenderTarget>) -> Result<()> {
        self.render_target = target;
        tm.reset();
        Ok(())
    }

    fn pause(&mut self, tm: &mut TimeManager) -> Result<()> {
        tm.pause();
        if let Some(bgm) = &mut self.bgm {
            bgm.music.pause()?;
        }
        Ok(())
    }

    fn resume(&mut self, tm: &mut TimeManager) -> Result<()> {
        tm.resume();
        if let Some(bgm) = &mut self.bgm {
            bgm.music.play()?;
        }
        Ok(())
    }

    fn update(&mut self, tm: &mut TimeManager) -> Result<()> {
        if let Some(bgm) = &mut self.bgm {
            bgm.audio_manager.recover_if_needed()?;
        }

        if self.game_scene.is_none() {
            self.loading_scene.update(tm)?;
            let loading_next_scene = self.loading_scene.next_scene(tm);
            match loading_next_scene {
                NextScene::PopWithResult(_) => self.next_scene = Some(loading_next_scene),
                NextScene::Replace(game_scene) => self.game_scene = Some(game_scene),
                _ => (),
            }
        }

        let t = tm.now() as f32;
        match self.state {
            State::Before => {
                if t > 0.5 {
                    self.state = State::Playing;
                    tm.reset();
                    if let Some(bgm) = &mut self.bgm {
                        bgm.music.seek_to(0.)?;
                        bgm.music.play()?;
                    }
                }
            }
            State::Playing => {
                if self.video.ended && t > self.music_length {
                    self.state = State::Blanking;
                    tm.reset();
                } else {
                    if let Some(bgm) = &mut self.bgm {
                        tm.update(bgm.music.position() as _);
                    }
                    self.video.update(t)?;
                }
            }
            State::Blanking => {
                if t > 1. && self.game_scene.is_some() {
                    self.next_scene = self.game_scene.take().map(|it| NextScene::Replace(it));
                } else {
                    self.state = State::Loading;
                    tm.reset();
                }
            }
            State::Loading => {
                if t > 1. && self.game_scene.is_some() {
                    self.state = State::Transforming;
                }
            }
            State::Transforming => {
                if t > 1. {
                    if self.game_scene.is_none() {
                        bail!("UnlockScene exited at State::Blank3 without GameScene");
                    }
                    self.next_scene = self.game_scene.take().map(|it| NextScene::Replace(it));
                }
            }
        }

        Ok(())
    }

    fn render(&mut self, tm: &mut TimeManager, ui: &mut prpr::ui::Ui) -> Result<()> {
        let mut cam = ui.camera();
        let asp = -cam.zoom.y;
        let t = tm.now() as f32;
        cam.render_target = self.render_target;
        set_camera(&cam);
        clear_background(BLACK);

        match self.state {
            State::Playing => {
                if t > 0.05 {
                    self.video.render(t, asp);
                }
            }
            State::Loading => {
                let pad = 0.07;
                let top = 1. / asp;
                ui.loading(
                    1. - pad,
                    top - pad,
                    t,
                    WHITE,
                    LoadingParams {
                        width: 0.01,
                        radius: 0.04,
                        ..Default::default()
                    },
                );
            }
            State::Transforming => {
                let top = 1. / asp;
                if t < 0.5 {
                    let pad = 0.07;
                    let alpha = if t < 0.5 { 1. - t / 0.5 } else { 0. }; // TODO: more smoothly
                    ui.loading(
                        1. - pad,
                        top - pad,
                        t,
                        semi_white(alpha),
                        LoadingParams {
                            width: 0.01,
                            radius: 0.04,
                            ..Default::default()
                        },
                    );
                } else {
                    let alpha = if t < 0.5 { t / 0.5 * 0.3 } else { 0.3 };
                    let r = ui.screen_rect();
                    ui.fill_rect(r, (*self.background, r));
                    ui.fill_rect(r, semi_black(alpha));
                }
            }
            _ => (),
        }

        Ok(())
    }

    fn next_scene(&mut self, _tm: &mut TimeManager) -> NextScene {
        self.next_scene.take().unwrap_or_default()
    }
}
