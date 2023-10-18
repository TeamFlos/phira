use crate::{dir, scene::PlayerView};
use anyhow::Result;
use phira_mp_common::UserInfo;
use prpr::{
    config::Config,
    core::ParticleEmitter,
    ext::LocalTask,
    fs,
    scene::{GameMode, GameScene, LoadingScene},
};
use std::path::Path;

#[must_use]
pub fn launch_task(id: i32, players: Vec<UserInfo>) -> Result<LocalTask<Result<(GameScene, Vec<PlayerView>)>>> {
    let mut fs = fs::fs_from_file(Path::new(&format!("{}/{id}", dir::downloaded_charts()?)))?;
    Ok(Some(Box::pin(async move {
        let mut info = fs::load_info(fs.as_mut()).await?;
        info.id = Some(id);
        let mut config = Config::default();
        config.sample_count = 4;
        config.player_name = "LIVE".to_owned();

        let mut charts = Vec::with_capacity(players.len());
        for _ in 0..players.len() {
            charts.push(GameScene::load_chart(fs.as_mut(), &info).await?.0);
        }
        let loading_scene = LoadingScene::new(GameMode::View, info, config, fs, None, None, None, None).await?;
        let game_scene = loading_scene.load_task.unwrap().await?;

        let views = players
            .into_iter()
            .zip(charts)
            .map(|(player, chart)| {
                Ok(PlayerView::new(
                    player,
                    chart,
                    ParticleEmitter::new(&game_scene.res.res_pack, game_scene.res.config.note_scale, game_scene.res.res_pack.info.hide_particles)?,
                ))
            })
            .collect::<Result<_>>()?;

        Ok((game_scene, views))
    })))
}
