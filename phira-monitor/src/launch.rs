use crate::{dir, scene::PlayerView};
use anyhow::Result;
use phira_mp_client::Client;
use phira_mp_common::{JudgeEvent, TouchFrame, UserInfo};
use prpr::{
    config::Config,
    core::{BadNote, ParticleEmitter, Vector},
    ext::LocalTask,
    fs,
    judge::JudgeStatus,
    scene::{GameMode, GameScene, LoadingScene, UpdateFn},
};
use std::{collections::VecDeque, path::Path, sync::Arc};

#[must_use]
pub fn launch_task(id: i32, client: Arc<Client>, players: Vec<UserInfo>) -> Result<LocalTask<Result<(GameScene, Vec<PlayerView>)>>> {
    let mut fs = fs::fs_from_file(Path::new(&format!("{}/{id}", dir::downloaded_charts()?)))?;
    let update_fn: UpdateFn = Box::new({
        let mut touches: VecDeque<TouchFrame> = VecDeque::new();
        let mut judges: VecDeque<JudgeEvent> = VecDeque::new();
        move |t, res, chart, judge, touch_points, bad_notes| {
            if let Some(player) = Some(114514) {
                {
                    touches.extend(client.live_player(player).touch_frames.blocking_lock().drain(..));
                    let mut updated = false;
                    while touches.front().map_or(false, |it| it.time < t) {
                        touches.pop_front();
                        updated = true;
                    }
                    if updated {
                        if let Some(frame) = touches.front() {
                            *touch_points = frame.points.iter().map(|it| (it.x(), it.y())).collect();
                        } else {
                            touch_points.clear();
                        }
                    }
                }
                {
                    judges.extend(client.live_player(player).judge_events.blocking_lock().drain(..));
                    while let Some(event) = judges.front() {
                        if event.time > t {
                            break;
                        }
                        let Some(event) = judges.pop_front() else { unreachable!() };
                        use phira_mp_common::Judgement::*;
                        use prpr::judge::Judgement as TJ;
                        let kind = match event.judgement {
                            Perfect => Ok(TJ::Perfect),
                            Good => Ok(TJ::Good),
                            Bad => Ok(TJ::Bad),
                            Miss => Ok(TJ::Miss),
                            HoldPerfect => Err(true),
                            HoldGood => Err(false),
                        };
                        let note = &mut chart.lines[event.line_id as usize].notes[event.note_id as usize];
                        match kind {
                            Ok(tj) => {
                                note.judge = JudgeStatus::Judged;
                                let line = &chart.lines[event.line_id as usize];
                                let line_tr = line.now_transform(res, &chart.lines);
                                let note = &line.notes[event.note_id as usize];
                                judge.commit(t, tj, event.line_id, event.note_id, 0.);
                                match tj {
                                    TJ::Perfect => {
                                        res.with_model(line_tr * note.object.now(res), |res| {
                                            res.emit_at_origin(note.rotation(line), res.res_pack.info.fx_perfect())
                                        });
                                    }
                                    TJ::Good => {
                                        res.with_model(line_tr * note.object.now(res), |res| {
                                            res.emit_at_origin(note.rotation(line), res.res_pack.info.fx_good())
                                        });
                                    }
                                    TJ::Bad => {
                                        bad_notes.push(BadNote {
                                            time: t,
                                            kind: note.kind.clone(),
                                            matrix: {
                                                let mut mat = line_tr;
                                                if !note.above {
                                                    mat.append_nonuniform_scaling_mut(&Vector::new(1., -1.));
                                                }
                                                let incline_sin = line.incline.now_opt().map(|it| it.to_radians().sin()).unwrap_or_default();
                                                mat *= note.now_transform(
                                                    res,
                                                    &line.ctrl_obj.borrow_mut(),
                                                    (note.height - line.height.now()) / res.aspect_ratio * note.speed,
                                                    incline_sin,
                                                );
                                                mat
                                            },
                                        });
                                    }
                                    _ => {}
                                }
                            }
                            Err(perfect) => {
                                note.judge = JudgeStatus::Hold(perfect, t, 0., false, f32::INFINITY);
                            }
                        }
                    }
                }
            }
        }
    });
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
        let loading_scene = LoadingScene::new(GameMode::Normal, info, config, fs, None, None, Some(update_fn)).await?;
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
