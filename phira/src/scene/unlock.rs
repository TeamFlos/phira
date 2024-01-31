use anyhow::Result;
use macroquad::prelude::*;
use prpr::{
    config::Config,
    core::{Anim, Keyframe, Video},
    ext::{create_audio_manger, ScaleType, BLACK_TEXTURE},
    fs::FileSystem,
    info::ChartInfo,
    scene::{BasicPlayer, GameMode, LoadingScene, NextScene, Scene, UpdateFn, UploadFn},
    time::TimeManager,
    ui::LoadingParams
};
use sasa::{AudioClip, Music, MusicParams};

pub struct UnlockScene {
    loading_scene: Box<LoadingScene>,
    game_scene: Option<Box<dyn Scene>>,
    next_scene: Option<NextScene>,

    video: Video,
    music: Music,

    end_time: f32,

    aspect_ratio: f32,
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
        let music = create_audio_manger(&config)?.create_music(
            AudioClip::new(fs.load_file("unlock.mp3").await?)?,
            MusicParams {
                amplifier: config.volume_music,
                ..Default::default()
            }
        )?;
        let aspect_ratio = config.aspect_ratio.unwrap_or(info.aspect_ratio);
        let loading_scene = Box::new(LoadingScene::new(mode, info, config, fs, player, upload_fn, update_fn,
            Some((BLACK_TEXTURE.clone(), BLACK_TEXTURE.clone(), WHITE))
        ).await?);

        Ok(UnlockScene {
            loading_scene,
            next_scene: None,
            game_scene: None,

            video,
            music,

            end_time: -1.,

            aspect_ratio,
        })
    }
}

const BLANKING_TIME: f32 = 1.;

impl Scene for UnlockScene {
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
        tm.update(self.music.position() as _);

        if self.game_scene.is_none() {
            self.loading_scene.update(tm);
            let loading_next_scene = self.loading_scene.next_scene(tm);
            match loading_next_scene {
                NextScene::PopWithResult(_) => self.next_scene = Some(loading_next_scene),
                NextScene::Replace(game_scene) => self.game_scene = Some(game_scene),
                _ => (),
            }
        }

        if self.video.ended && self.end_time < 0. {
            self.end_time = tm.now() as _;
        }

        if self.video.ended && self.game_scene.is_some() {
            self.next_scene = self.game_scene.take().map(|it| NextScene::Replace(it));
        } else {
            self.video.update(tm.now() as _)?;
        }

        Ok(())
    }

    fn render(&mut self, tm: &mut TimeManager, ui: &mut prpr::ui::Ui) -> Result<()> {
        let t = tm.now() as _;
        if self.end_time > 0. && t > self.end_time + BLANKING_TIME && self.next_scene.is_none() {
            let top = 1. - self.aspect_ratio;
            let padding = 0.07;
            ui.loading(
                1. - padding,
                top - padding,
                t,
                WHITE, // TODO: fade in && fade out
                LoadingParams {
                    radius: 0.04,
                    width: 0.01,
                    ..Default::default()
                }
            )
        } else {
            self.video.render(t, self.aspect_ratio);
        }

        Ok(())
    }

    fn next_scene(&mut self, _tm: &mut TimeManager) -> NextScene {
        self.next_scene.take().unwrap_or(NextScene::None)
    }
}
