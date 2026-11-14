use crate::page;
use anyhow::{anyhow, Result};
use macroquad::prelude::*;
use prpr::{
    ext::{semi_black, semi_white, RectExt},
    log::{self, CrashRecord},
    scene::{NextScene, Scene},
    time::TimeManager,
    ui::{DRectButton, Ui},
};
use std::{panic::AssertUnwindSafe, sync::mpsc, time::SystemTime};

pub struct CrashScene {
    record: CrashRecord,
    export_button: DRectButton,
    exit_button: DRectButton,
    export_requested: bool,
    export_task: Option<mpsc::Receiver<Result<()>>>,
    next_scene: NextScene,
    status: Option<String>,
}

impl CrashScene {
    pub fn new(record: CrashRecord) -> Self {
        Self {
            record,
            export_button: DRectButton::new().with_radius(0.012),
            exit_button: DRectButton::new().with_radius(0.012),
            export_requested: false,
            export_task: None,
            next_scene: NextScene::None,
            status: None,
        }
    }

    fn crash_metadata(&self) -> String {
        format!(
            "Phira {}\nos={} arch={}\ntime={:?}\nsummary={}\n\n{}",
            env!("VERSION_STRING"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            SystemTime::now(),
            self.record.summary,
            self.record.details,
        )
    }

    fn request_export(&mut self) {
        self.export_requested = true;
        self.status = Some(crate::ttl!("crash-exporting").into_owned());
        let name = format!("phira-crash-{}.zip", chrono::Local::now().format("%Y%m%d-%H%M%S"));
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| page::request_export(name)));
        match result {
            Ok(true) => {}
            Ok(false) => {
                self.export_requested = false;
                self.status = None;
            }
            Err(_) => {
                self.export_requested = false;
                self.status = Some(crate::ttl!("crash-export-failed").into_owned());
            }
        }
    }

    fn finish_export(&mut self, config: page::ExportConfig) {
        let manager = log::manager();
        let details = self.crash_metadata();
        let (tx, rx) = mpsc::sync_channel(1);
        // Deflating a week of logs can take seconds; keep it off the frame
        // thread to avoid an Android ANR on the crash screen itself.
        std::thread::spawn(move || {
            let result = manager
                .ok_or_else(|| anyhow!("log manager is unavailable"))
                .and_then(|manager| manager.write_zip(config.file, &details).map_err(Into::into));
            if result.is_err() {
                if let Err(error) = (config.deleter)() {
                    tracing::warn!("failed to delete incomplete crash export: {error}");
                }
            }
            let _ = tx.send(result);
        });
        self.export_task = Some(rx);
    }
}

impl Scene for CrashScene {
    fn touch(&mut self, tm: &mut TimeManager, touch: &Touch) -> Result<bool> {
        if self.exit_button.touch(touch, tm.now() as _) {
            self.next_scene = NextScene::Exit;
            return Ok(true);
        }
        if !self.export_requested && self.export_button.touch(touch, tm.now() as _) {
            self.request_export();
            return Ok(true);
        }
        Ok(true)
    }

    fn update(&mut self, _tm: &mut TimeManager) -> Result<()> {
        if self.export_requested {
            if let Some(config) = page::take_export() {
                match config {
                    Ok(config) => self.finish_export(config),
                    Err(error) => {
                        self.export_requested = false;
                        self.status = Some(format!("{}: {error}", crate::ttl!("crash-export-failed")));
                    }
                }
            }
        }
        if let Some(task) = &self.export_task {
            match task.try_recv() {
                Ok(result) => {
                    self.export_task = None;
                    self.export_requested = false;
                    match result {
                        Ok(()) => {
                            page::resolve_export();
                            self.status = Some(crate::ttl!("crash-exported").into_owned());
                        }
                        Err(error) => {
                            self.status = Some(format!("{}: {error}", crate::ttl!("crash-export-failed")));
                        }
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.export_task = None;
                    self.export_requested = false;
                    self.status = Some(crate::ttl!("crash-export-failed").into_owned());
                }
            }
        }
        Ok(())
    }

    fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        ui.fill_rect(ui.screen_rect(), Color::new(0.035, 0.04, 0.055, 1.));
        let panel = Rect::new(-0.82, -0.72, 1.64, 1.44);
        ui.fill_path(&panel.rounded(0.025), semi_black(0.7));
        ui.text(crate::ttl!("crash-title"))
            .pos(0., -0.56)
            .anchor(0.5, 0.5)
            .size(0.72)
            .color(WHITE)
            .draw();
        ui.text(crate::ttl!("crash-message"))
            .pos(0., -0.38)
            .anchor(0.5, 0.)
            .size(0.38)
            .max_width(1.38)
            .color(semi_white(0.85))
            .draw();
        ui.text(crate::ttl!("crash-summary", "error" => self.record.summary.clone()))
            .pos(-0.66, -0.1)
            .anchor(0., 0.)
            .size(0.34)
            .max_width(1.32)
            .color(semi_white(0.8))
            .draw();
        ui.text(crate::ttl!("crash-log-retention"))
            .pos(-0.66, 0.11)
            .anchor(0., 0.)
            .size(0.3)
            .max_width(1.32)
            .color(semi_white(0.65))
            .draw();

        let button_time = tm.now() as _;
        let export_rect = Rect::new(-0.64, 0.34, 0.58, 0.14);
        self.export_button
            .render_text(ui, export_rect, button_time, crate::ttl!("crash-export"), 0.42, false);
        let exit_rect = Rect::new(0.06, 0.34, 0.58, 0.14);
        self.exit_button
            .render_text(ui, exit_rect, button_time, crate::ttl!("crash-exit"), 0.42, false);
        if let Some(status) = &self.status {
            ui.text(status.as_str())
                .pos(0., 0.59)
                .anchor(0.5, 0.)
                .size(0.3)
                .max_width(1.35)
                .color(semi_white(0.75))
                .draw();
        }
        Ok(())
    }

    fn next_scene(&mut self, _tm: &mut TimeManager) -> NextScene {
        std::mem::take(&mut self.next_scene)
    }
}
