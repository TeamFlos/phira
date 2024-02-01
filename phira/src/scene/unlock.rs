use anyhow::{anyhow, Result};
use macroquad::prelude::*;
use prpr::{
    config::Config,
    core::{Anim, Keyframe, Video},
    ext::{create_audio_manger, semi_white, ScaleType, BLACK_TEXTURE},
    fs::FileSystem,
    info::ChartInfo,
    scene::{BasicPlayer, GameMode, LoadingScene, NextScene, Scene, UpdateFn, UploadFn},
    time::TimeManager,
    ui::LoadingParams
};
use sasa::{AudioClip, AudioManager, Music, MusicParams};

enum State {
    Blank1,
    Playing,
    Blank2,
    Loading,
    Blank3
}

pub struct UnlockScene {
    loading_scene: Box<LoadingScene>,
    game_scene: Option<Box<dyn Scene>>,
    next_scene: Option<NextScene>,

    render_target: Option<RenderTarget>,
    video: Video,
    audio_manager: AudioManager,
    music: Music,
    music_length: f32,

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
    ) -> Result<UnlockScene> {
        let video = Video::new(
            fs.load_file("unlock.mp4").await?,
            0.,
            ScaleType::CropCenter,
            Anim::new(vec![Keyframe::new(0., 1., 0)]),
            Anim::default(),
        )?;
        let clip = AudioClip::new(fs.load_file("unlock.mp3").await?)?;
        let music_length = clip.length();
        let mut audio_manager = create_audio_manger(&config)?;
        let music = audio_manager.create_music(
            clip,
            MusicParams {
                amplifier: config.volume_music,
                ..Default::default()
            }
        )?;
        let loading_scene = Box::new(LoadingScene::new(mode, info, config, fs, player, upload_fn, update_fn,
            Some((BLACK_TEXTURE.clone(), BLACK_TEXTURE.clone(), WHITE))
        ).await?);

        Ok(UnlockScene {
            loading_scene,
            next_scene: None,
            game_scene: None,

            render_target: None,
            video,
            audio_manager,
            music,
            music_length,

            state: State::Blank1,
        })
    }
}

impl Scene for UnlockScene {
    fn enter(&mut self, tm: &mut TimeManager, target: Option<RenderTarget>) -> Result<()> {
        self.render_target = target;
        tm.reset(); // TODO: useless?
        Ok(())
    }

    fn pause(&mut self, tm: &mut TimeManager) -> Result<()> {
        tm.pause();
        self.music.pause()?;
        Ok(())
    }

    fn resume(&mut self, tm: &mut TimeManager) -> Result<()> {
        tm.resume();
        self.music.play()?;
        Ok(())
    }

    fn update(&mut self, tm: &mut TimeManager) -> Result<()> {
        self.audio_manager.recover_if_needed()?;

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
            State::Blank1 => {
                if t > 0.5 {
                    self.state = State::Playing;
                    tm.reset();
                    self.music.seek_to(0.)?;
                    self.music.play()?;
                }
            },
            State::Playing => {
                if self.video.ended && t > self.music_length {
                    self.state = State::Blank2;
                    tm.reset();
                } else {
                    tm.seek_to(self.music.position() as _);
                    self.video.update(t)?;
                }
            },
            State::Blank2 => {
                if t > 1. && self.game_scene.is_some() {
                    self.next_scene = self.game_scene.take().map(|it| NextScene::Replace(it));
                } else {
                    self.state = State::Loading;
                    tm.reset();
                }
            },
            State::Loading => {
                if t > 1. && self.game_scene.is_some() {
                    self.state = State::Blank3;
                }
            },
            State::Blank3 => {
                if t > 1. {
                    if self.game_scene.is_none() {
                        return Err(anyhow!("UnlockScene exited at State::Blank3 without GameScene"));
                    }
                    self.next_scene = self.game_scene
                        .take()
                        .map(|it| NextScene::Replace(it));
                }
            },
        }

        Ok(())
    }

    fn render(&mut self, tm: &mut TimeManager, ui: &mut prpr::ui::Ui) -> Result<()> {
        clear_background(BLACK);
        let mut cam = ui.camera();
        let asp = -cam.zoom.y;
        let t = tm.now() as f32;
        cam.render_target = self.render_target;
        set_camera(&cam);

        match self.state {
            State::Playing => {
                self.video.render(t, asp);
            },
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
                    }
                );
            },
            State::Blank3 => {
                let top = 1. / asp;
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
                    }
                );
            }
            _ => (),
        }

        Ok(())
    }

    fn next_scene(&mut self, _tm: &mut TimeManager) -> NextScene {
        self.next_scene.take().unwrap_or(NextScene::None)
    }
}
