prpr::tl_file!("chapter");

use crate::{
    anim::Anim,
    data::BriefChartInfo,
    dir,
    icons::Icons,
    load_res_tex,
    page::{ChartItem, ChartType, Illustration, SFader},
    resource::rtl,
};
use anyhow::Result;
use macroquad::prelude::*;
use prpr::{
    config::Mods,
    core::BOLD_FONT,
    ext::{semi_black, semi_white, RectExt, SafeTexture},
    info::ChartInfo,
    scene::{NextScene, Scene},
    time::TimeManager,
    ui::{button_hit, DRectButton, RectButton, Scroll, Ui},
};
use serde::Deserialize;
use std::{borrow::Cow, sync::Arc};
use tap::Tap;

use super::{SongScene, ASSET_CHART_INFO};

#[repr(usize)]
#[derive(Clone, Copy)]
enum Difficulty {
    Easy,
    Hard,
    Extreme,
}
impl Difficulty {
    pub fn name(&self) -> Cow<'static, str> {
        match self {
            Self::Easy => tl!("diff-easy"),
            Self::Hard => tl!("diff-hard"),
            Self::Extreme => tl!("diff-extreme"),
        }
    }

    pub fn color(&self) -> Color {
        Color::from_hex(match self {
            Self::Easy => 0xff16a34a,
            Self::Hard => 0xfff97316,
            Self::Extreme => 0xffdc2626,
        })
    }
}

#[derive(Deserialize)]
struct LevelInfo {
    level: String,
    charter: String,
    difficulty: f32,
}

#[derive(Deserialize)]
struct SongInfo {
    name: String,
    intro: String,
    composer: String,
    illustrator: String,
    levels: Vec<LevelInfo>,
}

struct ChartInstance {
    id: String,
    info: SongInfo,
    illu: SafeTexture,
    btn: DRectButton,
}

pub struct ChapterScene {
    id: String,

    icons: Arc<Icons>,
    rank_icons: [SafeTexture; 8],
    cover: SafeTexture,

    btn_back: RectButton,

    next_scene: Option<NextScene>,

    first_in: bool,

    diff: Difficulty,
    diff_btn: DRectButton,
    diff_btn_color: Anim<Color>,

    sf: SFader,

    scroll: Scroll,
    charts: Vec<ChartInstance>,
}

impl ChapterScene {
    const WIDTH: f32 = 0.5;
    const HEIGHT: f32 = 0.3;
    const PAD: f32 = 0.05;

    pub async fn new(id: String, icons: Arc<Icons>, rank_icons: [SafeTexture; 8], cover: SafeTexture) -> Result<Self> {
        let songs = match id.as_str() {
            "c1" => vec!["snow", "jumping23"],
            _ => vec![],
        };
        let mut charts = Vec::with_capacity(songs.capacity());
        for song in songs {
            let info = serde_yaml::from_slice(&load_file(&format!("res/song/{song}/info.yml")).await?)?;
            let illu = load_res_tex(&format!("res/song/{song}/cover")).await;
            charts.push(ChartInstance {
                id: song.to_owned(),
                info,
                illu,
                btn: DRectButton::new(),
            });
        }
        Ok(Self {
            id,

            icons,
            rank_icons,
            cover,
            btn_back: RectButton::new(),

            next_scene: None,

            first_in: true,

            diff: Difficulty::Hard,
            diff_btn: DRectButton::new(),
            diff_btn_color: Anim::new(Difficulty::Hard.color()),

            sf: SFader::new(),

            scroll: Scroll::new().tap_mut(|it| it.y_scroller.step = Self::HEIGHT + Self::PAD),
            charts,
        })
    }
}

impl Scene for ChapterScene {
    fn enter(&mut self, tm: &mut TimeManager, _target: Option<RenderTarget>) -> Result<()> {
        if self.first_in {
            self.first_in = false;
            tm.reset();
        }
        Ok(())
    }

    fn touch(&mut self, tm: &mut TimeManager, touch: &Touch) -> Result<bool> {
        let t = tm.now() as f32;
        if self.btn_back.touch(touch) {
            button_hit();
            self.next_scene = Some(NextScene::Pop);
            return Ok(true);
        }
        if self.diff_btn.touch(touch, t) {
            button_hit();
            self.diff = match self.diff {
                Difficulty::Easy => Difficulty::Hard,
                Difficulty::Hard => Difficulty::Extreme,
                Difficulty::Extreme => Difficulty::Easy,
            };
            self.diff_btn_color.goto(self.diff.color(), t, 0.4);
            return Ok(true);
        }
        if self.scroll.touch(touch, t) {
            return Ok(true);
        }
        for chart in &mut self.charts {
            if chart.btn.touch(touch, t) {
                button_hit();
                let info = &chart.info;
                let level = &info.levels[self.diff as usize];
                let local_path = format!(
                    ":{}:{}",
                    chart.id,
                    match self.diff {
                        Difficulty::Easy => "ez",
                        Difficulty::Hard => "hd",
                        Difficulty::Extreme => "ex",
                    }
                );
                let item = ChartItem {
                    info: BriefChartInfo {
                        id: None,
                        uploader: None,
                        name: info.name.clone(),
                        level: level.level.clone(),
                        difficulty: level.difficulty,
                        intro: info.intro.clone(),
                        charter: level.charter.clone(),
                        composer: info.composer.clone(),
                        illustrator: info.illustrator.clone(),
                        created: None,
                        updated: None,
                        chart_updated: None,
                        has_unlock: false,
                    },
                    illu: Illustration::from_done(chart.illu.clone()),
                    local_path: Some(local_path.clone()),
                    chart_type: ChartType::Integrated,
                };
                let info = &item.info;
                let dir = format!("{}/{}", dir::charts()?, item.local_path.as_ref().unwrap().replace(':', "_"));
                let path = std::path::Path::new(&dir);
                if !path.exists() {
                    std::fs::create_dir_all(path)?;
                }
                let dir = prpr::dir::Dir::new(dir)?;
                *ASSET_CHART_INFO.lock().unwrap() = Some(ChartInfo {
                    id: None,
                    uploader: None,

                    name: info.name.clone(),
                    difficulty: info.difficulty,
                    level: info.level.clone(),
                    charter: info.charter.clone(),
                    composer: info.composer.clone(),
                    illustrator: info.illustrator.clone(),

                    chart: ":chart".to_owned(),
                    format: None,
                    music: ":music".to_owned(),
                    illustration: ":illu".to_owned(),
                    unlock_video: None,

                    preview_start: 0.,
                    preview_end: None,
                    aspect_ratio: 16. / 9.,
                    background_dim: 0.6,
                    line_length: 6.,
                    offset: dir
                        .read("offset")
                        .map(|d| {
                            f32::from_be_bytes(
                                d.get(0..4)
                                    .map(|first4| {
                                        let mut result = <[u8; 4]>::default();
                                        result.copy_from_slice(first4);
                                        result
                                    })
                                    .unwrap_or_default(),
                            )
                        })
                        .unwrap_or_default(),
                    tip: None,
                    tags: Vec::new(),

                    intro: info.intro.clone(),

                    hold_partial_cover: true,
                    created: None,
                    updated: None,
                    chart_updated: None,
                });
                self.sf
                    .goto(t, SongScene::new(item, Some(local_path), Arc::clone(&self.icons), self.rank_icons.clone(), Mods::empty()));
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn update(&mut self, tm: &mut TimeManager) -> Result<()> {
        let t = tm.now() as f32;
        self.scroll.update(t);
        Ok(())
    }

    fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        set_camera(&ui.camera());
        let t = tm.now() as f32;

        let r = ui.screen_rect();
        ui.fill_rect(r, (*self.cover, r));
        ui.fill_rect(r, semi_black(0.3));
        let r = ui.back_rect();
        ui.fill_rect(r, (*self.icons.back, r));
        self.btn_back.set(ui, r);

        use crate::resource::L10N_LOCAL;

        let title = rtl!(format!("chap-{}", self.id)).into_owned();
        let intro = rtl!(format!("chap-{}-intro", self.id));

        let p = (t / 0.4).min(1.);
        let p = 1. - (1. - p).powi(3);

        let r = Rect::new(-0.83, -0.35, 0.6, 0.12);
        ui.scissor(r, |ui| {
            ui.text(title).pos(r.x, r.y + (1. - p) * 0.1).size(1.4).draw_using(&BOLD_FONT);
        });
        ui.text(intro)
            .pos(r.x, r.bottom() + 0.02)
            .size(0.44)
            .max_width(0.74)
            .multiline()
            .color(semi_white(p))
            .draw();

        let r = Rect::new(r.x, 0.3, 0.24, 0.1);
        self.diff_btn.render_shadow(ui, r, t, |ui, path| {
            let ct = r.center();
            ui.fill_path(&path, self.diff_btn_color.now(t));
            ui.text(self.diff.name())
                .pos(ct.x, ct.y)
                .anchor(0.5, 0.5)
                .no_baseline()
                .size(0.6)
                .draw_using(&BOLD_FONT);
        });

        let r = Rect::new(0.2, -ui.top, 0.6, ui.top * 2.);
        self.scroll.size((r.w, r.h));
        ui.scope(|ui| {
            ui.dx(r.x);
            ui.dy(r.y);
            self.scroll.render(ui, |ui| {
                ui.dx(r.w / 2.);
                ui.dy(ui.top);
                let mut y = 0.;
                let step = Self::HEIGHT + Self::PAD;
                for chart in &mut self.charts {
                    let r = Rect::new(-Self::WIDTH / 2., y - Self::HEIGHT / 2., Self::WIDTH, Self::HEIGHT);
                    chart.btn.render_shadow(ui, r, t, |ui, path| {
                        ui.fill_path(&path, (*chart.illu, r));
                        ui.fill_path(&path, semi_black(0.4));
                        let mut t = ui
                            .text(&chart.info.levels[self.diff as usize].level)
                            .pos(r.right() - 0.016, r.y + 0.016)
                            .max_width(r.w * 2. / 3.)
                            .anchor(1., 0.)
                            .size(0.52)
                            .color(WHITE);
                        let ms = t.measure();
                        t.ui.fill_path(&ms.feather(0.008).rounded(0.01), Color { a: 0.7, ..t.ui.background() });
                        t.draw();

                        ui.text(&chart.info.name)
                            .pos(r.x + 0.01, r.bottom() - 0.02)
                            .max_width(r.w)
                            .anchor(0., 1.)
                            .size(0.6)
                            .color(WHITE)
                            .draw();
                    });
                    y += step;
                }

                (Self::WIDTH, step * (self.charts.len() - 1) as f32 + ui.top * 2.)
            });
        });

        self.sf.render(ui, t);

        Ok(())
    }

    fn next_scene(&mut self, tm: &mut TimeManager) -> NextScene {
        self.next_scene.take().or_else(|| self.sf.next_scene(tm.now() as f32)).unwrap_or_default()
    }
}
