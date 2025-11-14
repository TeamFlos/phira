prpr_l10n::tl_file!("chart_info");

use super::Ui;
use crate::{core::BOLD_FONT, ext::parse_time, info::ChartInfo, scene::show_message};
use anyhow::Result;
use std::{borrow::Cow, collections::HashMap};

#[derive(Clone)]
pub struct ChartInfoEdit {
    pub info: ChartInfo,
    pub chart: Option<String>,
    pub music: Option<String>,
    pub illustration: Option<String>,
    pub unlock_video: Option<String>,
    pub enable_unlock: bool,
    pub updated: bool,
}

impl ChartInfoEdit {
    pub fn new(info: ChartInfo) -> Self {
        let enable_unlock = info.unlock_video.is_some();
        Self {
            info,
            chart: None,
            music: None,
            illustration: None,
            unlock_video: None,
            enable_unlock,
            updated: false,
        }
    }

    pub async fn to_patches(&self) -> Result<HashMap<String, Vec<u8>>> {
        let mut res = HashMap::new();
        res.insert("info.yml".to_owned(), serde_yaml::to_string(&self.info)?.into_bytes());
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(chart) = &self.chart {
                res.insert(self.info.chart.clone(), tokio::fs::read(chart).await?);
            }
            if let Some(music) = &self.music {
                res.insert(self.info.music.clone(), tokio::fs::read(music).await?);
            }
            if let Some(illustration) = &self.illustration {
                res.insert(self.info.illustration.clone(), tokio::fs::read(illustration).await?);
            }
            if self.enable_unlock {
                if let Some(unlock) = &self.unlock_video {
                    res.insert(self.info.unlock_video.clone().unwrap_or("unlock.mp4".to_string()), tokio::fs::read(unlock).await?);
                }
            }
        }
        Ok(res)
    }
}

fn format_time(t: f32) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    let it = t as u32;
    write!(&mut s, "{:02}:{:02}:{:05.2}", it / 3600, (it / 60) % 60, t % 60.).unwrap();
    s
}

pub fn render_chart_info(ui: &mut Ui, edit: &mut ChartInfoEdit, width: f32) -> (f32, f32) {
    let mut sy = 0.02;
    ui.scope(|ui| {
        let s = 0.01;
        ui.dx(0.01);
        ui.dy(sy);
        macro_rules! dy {
            ($dy:expr) => {{
                let dy = $dy;
                sy += dy;
                ui.dy(dy);
            }};
        }
        dy!(0.01);
        let r = ui.text(tl!("edit-chart")).size(0.9).draw_using(&BOLD_FONT);
        dy!(r.h + 0.04);
        let rt = 0.22;
        ui.dx(rt);
        let len = width - rt - 0.04;
        let info = &mut edit.info;
        let r = ui.input(tl!("chart-name"), &mut info.name, (len, &mut edit.updated));
        dy!(r.h + s);
        let r = ui.input(tl!("author"), &mut info.charter, (len, &mut edit.updated));
        dy!(r.h + s);
        let r = ui.input(tl!("composer"), &mut info.composer, (len, &mut edit.updated));
        dy!(r.h + s);
        let r = ui.input(tl!("illustrator"), &mut info.illustrator, (len, &mut edit.updated));
        dy!(r.h + s + 0.02);

        let r = ui.input(tl!("level-displayed"), &mut info.level, (len, &mut edit.updated));
        dy!(r.h + s);

        ui.dx(-rt);
        let last = info.difficulty;
        let r = ui.slider(tl!("diff"), 0.0..20.0, 0.1, &mut info.difficulty, Some(width - 0.2));
        if (info.difficulty - last).abs() > 1e-4 {
            edit.updated = true;
        }
        dy!(r.h + s + 0.01);
        ui.dx(rt);

        let mut string = format!("{} - {}", format_time(info.preview_start), format_time(info.preview_end.unwrap_or(info.preview_start + 15.)));
        let mut changed = false;
        let r = ui.input(tl!("preview-time"), &mut string, (len, &mut changed));
        dy!(r.h + s);
        if changed {
            edit.updated = true;
            match || -> Result<(f32, f32), Cow<'static, str>> {
                let (st, en) = string.split_once(['-', '—']).ok_or_else(|| tl!("illegal-input"))?;
                let st = parse_time(st.trim()).ok_or_else(|| tl!("invalid time"))?;
                let en = parse_time(en.trim()).ok_or_else(|| tl!("invalid time"))?;
                if st + 1. > en {
                    return Err(tl!("preview-too-short"));
                }
                if st + 20. < en {
                    return Err(tl!("preview-too-long"));
                }
                Ok((st, en))
            }() {
                Err(err) => {
                    show_message(err).error();
                }
                Ok((st, en)) => {
                    info.preview_start = st;
                    info.preview_end = Some(en);
                }
            }
        }
        dy!(ui.scope(|ui| {
            ui.text(tl!("ps")).anchor(1., 0.).size(0.35).draw();
            ui.text(tl!("preview-hint")).pos(0.02, 0.).size(0.35).max_width(len).multiline().draw().h + 0.03
        }));

        let mut string = format!("{:.3}", info.offset);
        let mut changed = false;
        let r = ui.input(tl!("offset"), &mut string, (len, &mut changed));
        dy!(r.h + s);
        if changed {
            edit.updated = true;
            match string.parse::<f32>() {
                Err(_) => {
                    show_message(tl!("illegal-input")).error();
                }
                Ok(value) => {
                    info.offset = value;
                }
            }
        }

        let mut string = format!("{:.5}", info.aspect_ratio);
        let mut changed = false;
        let r = ui.input(tl!("aspect-ratio"), &mut string, (len, &mut changed));
        dy!(r.h + s);
        if changed {
            edit.updated = true;
            match || -> Result<f32> {
                if let Some((w, h)) = string.split_once([':', '：']) {
                    Ok(w.trim().parse::<f32>()? / h.trim().parse::<f32>()?)
                } else {
                    Ok(string.parse()?)
                }
            }() {
                Err(_) => {
                    show_message(tl!("illegal-input")).error();
                }
                Ok(value) => {
                    if value.is_finite() && value > 0.0 {
                        info.aspect_ratio = value;
                    } else {
                        show_message(tl!("illegal-input")).error();
                    }
                }
            }
        }
        dy!(ui.scope(|ui| {
            ui.text(tl!("ps")).anchor(1., 0.).size(0.35).draw();
            ui.text(tl!("aspect-hint")).pos(0.02, 0.).size(0.35).max_width(len).multiline().draw().h + 0.03
        }));

        ui.dx(-rt);
        let last = info.background_dim;
        let r = ui.slider(tl!("dim"), 0.0..1.0, 0.05, &mut info.background_dim, Some(width - 0.2));
        if (info.background_dim - last).abs() > 1e-4 {
            edit.updated = true;
        }
        dy!(r.h + s + 0.01);
        ui.dx(rt);

        #[cfg(not(target_arch = "wasm32"))]
        {
            use crate::scene::{request_file, return_file, take_file};
            use macroquad::prelude::Rect;

            let r = ui.text(tl!("enable-unlock")).size(0.47).anchor(1., 0.).draw();
            let r = Rect::new(0.02, r.y - 0.01, r.h + 0.02, r.h + 0.02);
            let check_str = if edit.enable_unlock { "v" } else { "" };
            if ui.button("unlockchk", r, check_str.to_string()) {
                if edit.enable_unlock {
                    info.unlock_video = None;
                    edit.enable_unlock = false;
                } else {
                    info.unlock_video = Some("unlock.mp4".to_string());
                    edit.enable_unlock = true;
                }
                edit.updated = true;
            }
            dy!(r.h + s);

            let mut choose_file = |id: &str, label: Cow<'static, str>, value: &str| {
                let r = ui.text(label).size(0.47).anchor(1., 0.).draw();
                let r = Rect::new(0.02, r.y - 0.01, len, r.h + 0.02);
                if ui.button(id, r, value) {
                    request_file(id);
                }
                dy!(r.h + s);
            };

            choose_file("chart", tl!("chart-file"), &info.chart);
            choose_file("music", tl!("music-file"), &info.music);
            choose_file("illustration", tl!("illu-file"), &info.illustration);
            choose_file("unlock", tl!("unlock-file"), info.unlock_video.as_deref().unwrap_or("Disabled"));

            if let Some((id, file)) = take_file() {
                match id.as_str() {
                    "chart" => {
                        edit.chart = Some(file);
                        edit.updated = true;
                    }
                    "music" => {
                        edit.music = Some(file);
                        edit.updated = true;
                    }
                    "illustration" => {
                        edit.illustration = Some(file);
                        edit.updated = true;
                    }
                    "unlock" => {
                        if edit.enable_unlock {
                            edit.unlock_video = Some(file);
                            edit.updated = true;
                        }
                    }
                    _ => return_file(id, file),
                }
            }
        }

        let mut string = info.tip.clone().unwrap_or_default();
        let r = ui.input(tl!("tip"), &mut string, (len, &mut edit.updated));

        dy!(r.h + s);
        info.tip = if string.is_empty() { None } else { Some(string) };

        ui.input(tl!("intro"), &mut info.intro, (len, &mut edit.updated));
        ui.dx(-0.02);
    });
    (width, sy)
}
