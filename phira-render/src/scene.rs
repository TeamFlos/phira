use crate::{VideoConfig, INFO_EDIT, VIDEO_CONFIG};
use anyhow::{bail, Result};
use macroquad::prelude::*;
use prpr::{
    config::Config,
    ext::{poll_future, screen_aspect, LocalTask},
    fs::{FileSystem, PatchedFileSystem},
    info::ChartInfo,
    scene::{show_error, show_message, GameMode, LoadingScene, NextScene, Scene},
    time::TimeManager,
    ui::{render_chart_info, ChartInfoEdit, Scroll, Ui},
};

pub struct MainScene {
    target: Option<RenderTarget>,

    scroll: Scroll,
    edit: ChartInfoEdit,
    config: Config,
    fs: Box<dyn FileSystem>,
    next_scene: Option<NextScene>,
    v_config: VideoConfig,

    loading_scene_task: LocalTask<Result<LoadingScene>>,
}

impl MainScene {
    pub fn new(target: Option<RenderTarget>, info: ChartInfo, config: Config, fs: Box<dyn FileSystem>) -> Self {
        Self {
            target,

            scroll: Scroll::new(),
            edit: ChartInfoEdit::new(info),
            config,
            fs,
            next_scene: None,
            v_config: VideoConfig::default(),

            loading_scene_task: None,
        }
    }
}

impl Scene for MainScene {
    fn on_result(&mut self, _tm: &mut TimeManager, result: Box<dyn std::any::Any>) -> Result<()> {
        show_error(result.downcast::<anyhow::Error>().unwrap().context("加载谱面失败"));
        Ok(())
    }

    fn touch(&mut self, tm: &mut TimeManager, touch: &Touch) -> Result<bool> {
        Ok(self.scroll.touch(touch, tm.now() as _))
    }

    fn update(&mut self, tm: &mut TimeManager) -> Result<()> {
        self.scroll.update(tm.now() as _);
        if let Some(future) = &mut self.loading_scene_task {
            if let Some(scene) = poll_future(future.as_mut()) {
                self.loading_scene_task = None;
                self.next_scene = Some(NextScene::Overlay(Box::new(scene?)));
            }
        }
        Ok(())
    }

    fn render(&mut self, _tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        set_camera(&Camera2D {
            zoom: vec2(1., -screen_aspect()),
            render_target: self.target,
            ..Default::default()
        });
        clear_background(GRAY);
        let width = 1.;
        ui.scope(|ui| {
            ui.dx(-1.);
            ui.dy(-ui.top);
            let h = 0.1;
            let pad = 0.01;
            self.scroll.size((width, ui.top * 2. - h));
            let dx = width / 2.;
            let mut r = Rect::new(pad, ui.top * 2. - h + pad, dx - pad * 2., h - pad * 2.);
            if ui.button("preview", r, "预览") {
                let info = self.edit.info.clone();
                let config = self.config.clone();
                let fs = self.fs.clone_box();
                let edit = self.edit.clone();
                self.loading_scene_task = Some(Box::pin(async move {
                    LoadingScene::new(GameMode::Normal, info, config, Box::new(PatchedFileSystem(fs, edit.to_patches().await?)), None, None, None)
                        .await
                }));
            }
            r.x += dx;
            if ui.button("render", r, "渲染") {
                *INFO_EDIT.lock().unwrap() = Some(self.edit.clone());
                *VIDEO_CONFIG.lock().unwrap() = Some(self.v_config.clone());
                self.next_scene = Some(NextScene::Exit);
            }
            self.scroll.render(ui, |ui| {
                ui.dy(pad);
                let r = ui.text("注：可以通过鼠标拖动屏幕来查看更下面的配置项").size(0.4).draw();
                ui.dy(r.h + pad);
                let (w, mut h) = render_chart_info(ui, &mut self.edit, width);
                ui.scope(|ui| {
                    ui.dy(h);
                    h += r.h + pad * 2.;
                    let width = ui.text("一二三四").size(0.4).measure().w;
                    ui.dx(width);
                    let res = self.v_config.resolution;
                    let mut string = format!("{}x{}", res.0, res.1);
                    let r = ui.input("分辨率", &mut string, 0.8);
                    match || -> Result<(u32, u32)> {
                        if let Some((w, h)) = string.split_once(['x', 'X', '×', '*']) {
                            Ok((w.parse::<u32>()?, h.parse::<u32>()?))
                        } else {
                            bail!("格式应当为 “宽x高”")
                        }
                    }() {
                        Err(_) => {
                            show_message("输入非法");
                        }
                        Ok(value) => {
                            self.v_config.resolution = value;
                        }
                    }
                    ui.dy(r.h + pad);
                    h += r.h + pad;

                    let mut string = self.v_config.fps.to_string();
                    let old = string.clone();
                    let r = ui.input("FPS", &mut string, 0.8);
                    if string != old {
                        match string.parse::<u32>() {
                            Err(_) => {
                                show_message("输入非法");
                            }
                            Ok(value) => {
                                self.v_config.fps = value;
                            }
                        }
                    }
                    ui.dy(r.h + pad);
                    h += r.h + pad;

                    let r = ui.input("码率", &mut self.v_config.bitrate, 0.8);
                    ui.dy(r.h + pad);
                    h += r.h + pad;

                    let mut string = format!("{:.2}", self.v_config.ending_length);
                    let old = string.clone();
                    let r = ui.input("结算时间", &mut string, 0.8);
                    if string != old {
                        match string.parse::<f64>() {
                            Err(_) => {
                                show_message("输入非法");
                            }
                            Ok(value) => {
                                if !value.is_finite() || value < 0. {
                                    show_message("输入非法");
                                }
                                self.v_config.ending_length = value;
                            }
                        }
                    }
                    ui.dy(r.h + pad);
                    h += r.h + pad;

                    let r = ui.checkbox("启用硬件加速", &mut self.v_config.hardware_accel);
                    ui.dy(r.h + pad);
                    h += r.h + pad;
                });
                (w, h)
            });
        });
        Ok(())
    }

    fn next_scene(&mut self, _tm: &mut TimeManager) -> NextScene {
        self.next_scene.take().unwrap_or_default()
    }
}
