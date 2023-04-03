prpr::tl_file!("settings");

use crate::{get_data, get_data_mut, popup::ChooseButton, save_data, sync_data};

use super::{NextPage, OffsetPage, Page, SharedState};
use anyhow::Result;
use macroquad::prelude::*;
use prpr::{
    ext::{poll_future, semi_black, LocalTask, RectExt, SafeTexture, ScaleType},
    l10n::{LanguageIdentifier, LANG_IDENTS, LANG_NAMES},
    scene::show_error,
    ui::{DRectButton, Scroll, Slider, Ui},
};
use std::borrow::Cow;

const ITEM_HEIGHT: f32 = 0.15;

#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingListType {
    General,
    Audio,
    Chart,
}

pub struct SettingsPage {
    btn_general: DRectButton,
    btn_audio: DRectButton,
    btn_chart: DRectButton,
    chosen: SettingListType,

    list_general: GeneralList,
    list_audio: AudioList,
    list_chart: ChartList,

    scroll: Scroll,
    save_time: f32,
}

impl SettingsPage {
    const SAVE_TIME: f32 = 0.5;

    pub fn new(icon_lang: SafeTexture) -> Self {
        Self {
            btn_general: DRectButton::new(),
            btn_audio: DRectButton::new(),
            btn_chart: DRectButton::new(),
            chosen: SettingListType::General,

            list_general: GeneralList::new(icon_lang),
            list_audio: AudioList::new(),
            list_chart: ChartList::new(),

            scroll: Scroll::new(),
            save_time: f32::INFINITY,
        }
    }

    #[inline]
    fn switch_to_type(&mut self, ty: SettingListType) {
        if self.chosen != ty {
            self.chosen = ty;
            self.scroll.y_scroller.offset = 0.;
        }
    }
}

impl Page for SettingsPage {
    fn label(&self) -> Cow<'static, str> {
        "SETTINGS".into()
    }

    fn exit(&mut self) -> Result<()> {
        if self.save_time.is_finite() {
            save_data()?;
        }
        Ok(())
    }

    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        let t = s.t;
        if match self.chosen {
            SettingListType::General => self.list_general.top_touch(touch, t),
            SettingListType::Audio => self.list_audio.top_touch(touch, t),
            SettingListType::Chart => self.list_chart.top_touch(touch, t),
        } {
            return Ok(true);
        }

        if self.btn_general.touch(touch, t) {
            self.switch_to_type(SettingListType::General);
            return Ok(true);
        }
        if self.btn_audio.touch(touch, t) {
            self.switch_to_type(SettingListType::Audio);
            return Ok(true);
        }
        if self.btn_chart.touch(touch, t) {
            self.switch_to_type(SettingListType::Chart);
            return Ok(true);
        }
        if self.scroll.touch(touch, t) {
            return Ok(true);
        }
        if let Some(p) = match self.chosen {
            SettingListType::General => self.list_general.touch(touch, t)?,
            SettingListType::Audio => self.list_audio.touch(touch, t)?,
            SettingListType::Chart => self.list_chart.touch(touch, t)?,
        } {
            if p {
                self.save_time = t;
            }
            self.scroll.y_scroller.halt();
            return Ok(true);
        }
        Ok(false)
    }

    fn update(&mut self, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        self.scroll.update(t);
        if match self.chosen {
            SettingListType::General => self.list_general.update(t)?,
            SettingListType::Audio => self.list_audio.update(t)?,
            SettingListType::Chart => self.list_chart.update(t)?,
        } {
            self.save_time = t;
        }
        if t > self.save_time + Self::SAVE_TIME {
            save_data()?;
            self.save_time = f32::INFINITY;
        }
        Ok(())
    }

    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        s.render_fader(ui, |ui, c| {
            ui.tab_rects(
                c,
                t,
                [
                    (&mut self.btn_general, tl!("general"), SettingListType::General),
                    (&mut self.btn_audio, tl!("audio"), SettingListType::Audio),
                    (&mut self.btn_chart, tl!("chart"), SettingListType::Chart),
                ]
                .into_iter()
                .map(|(btn, text, ty)| (btn, text, ty == self.chosen)),
            );
        });
        let r = ui.content_rect();
        s.fader.render(ui, t, |ui, c| {
            let path = r.rounded(0.02);
            ui.fill_path(&path, semi_black(0.4 * c.a));
            let r = r.feather(-0.01);
            self.scroll.size((r.w, r.h));
            ui.scope(|ui| {
                ui.dx(r.x);
                ui.dy(r.y);
                self.scroll.render(ui, |ui| match self.chosen {
                    SettingListType::General => self.list_general.render(ui, r, t, c),
                    SettingListType::Audio => self.list_audio.render(ui, r, t, c),
                    SettingListType::Chart => self.list_chart.render(ui, r, t, c),
                });
            });
        });
        Ok(())
    }

    fn next_page(&mut self) -> NextPage {
        if matches!(self.chosen, SettingListType::Audio) {
            return self.list_audio.next_page().unwrap_or_default();
        }
        NextPage::None
    }
}

fn render_title<'a>(ui: &mut Ui, c: Color, title: impl Into<Cow<'a, str>>, subtitle: Option<Cow<'a, str>>) -> f32 {
    const TITLE_SIZE: f32 = 0.6;
    const SUBTITLE_SIZE: f32 = 0.35;
    const LEFT: f32 = 0.06;
    const PAD: f32 = 0.01;
    const SUB_MAX_WIDTH: f32 = 1.;
    if let Some(subtitle) = subtitle {
        let title = title.into();
        let r1 = ui.text(Cow::clone(&title)).size(TITLE_SIZE).measure();
        let r2 = ui
            .text(Cow::clone(&subtitle))
            .size(SUBTITLE_SIZE)
            .max_width(SUB_MAX_WIDTH)
            .no_baseline()
            .measure();
        let h = r1.h + PAD + r2.h;
        let r1 = ui
            .text(subtitle)
            .pos(LEFT, (ITEM_HEIGHT + h) / 2.)
            .anchor(0., 1.)
            .size(SUBTITLE_SIZE)
            .max_width(SUB_MAX_WIDTH)
            .color(Color { a: c.a * 0.6, ..c })
            .draw()
            .right();
        let r2 = ui
            .text(title)
            .pos(LEFT, (ITEM_HEIGHT - h) / 2.)
            .no_baseline()
            .size(TITLE_SIZE)
            .color(c)
            .draw()
            .right();
        r1.max(r2)
    } else {
        ui.text(title.into())
            .pos(LEFT, ITEM_HEIGHT / 2.)
            .anchor(0., 0.5)
            .no_baseline()
            .size(TITLE_SIZE)
            .color(c)
            .draw()
            .right()
    }
}

#[inline]
fn render_switch(ui: &mut Ui, r: Rect, t: f32, c: Color, btn: &mut DRectButton, on: bool) {
    btn.render_text(ui, r, t, c.a, if on { tl!("switch-on") } else { tl!("switch-off") }, 0.5, on);
}

#[inline]
fn right_rect(w: f32) -> Rect {
    let rh = ITEM_HEIGHT * 2. / 3.;
    Rect::new(w - 0.3, (ITEM_HEIGHT - rh) / 2., 0.26, rh)
}

struct GeneralList {
    icon_lang: SafeTexture,

    lang_btn: ChooseButton,
    offline_btn: DRectButton,
    lowq_btn: DRectButton,
}

impl GeneralList {
    pub fn new(icon_lang: SafeTexture) -> Self {
        Self {
            icon_lang,

            lang_btn: ChooseButton::new()
                .with_options(LANG_NAMES.iter().map(|s| s.to_string()).collect())
                .with_selected(
                    get_data()
                        .language
                        .as_ref()
                        .and_then(|it| it.parse::<LanguageIdentifier>().ok())
                        .and_then(|ident| LANG_IDENTS.iter().position(|it| *it == ident))
                        .unwrap_or_default(),
                ),
            offline_btn: DRectButton::new(),
            lowq_btn: DRectButton::new(),
        }
    }

    pub fn top_touch(&mut self, touch: &Touch, t: f32) -> bool {
        if self.lang_btn.top_touch(touch, t) {
            return true;
        }
        false
    }

    pub fn touch(&mut self, touch: &Touch, t: f32) -> Result<Option<bool>> {
        let data = get_data_mut();
        let config = &mut data.config;
        if self.lang_btn.touch(touch, t) {
            return Ok(Some(false));
        }
        if self.offline_btn.touch(touch, t) {
            config.offline_mode ^= true;
            return Ok(Some(true));
        }
        if self.lowq_btn.touch(touch, t) {
            config.sample_count = if config.sample_count == 1 { 2 } else { 1 };
            return Ok(Some(true));
        }
        Ok(None)
    }

    pub fn update(&mut self, t: f32) -> Result<bool> {
        self.lang_btn.update(t);
        let data = get_data_mut();
        if self.lang_btn.changed() {
            data.language = Some(LANG_IDENTS[self.lang_btn.selected()].to_string());
            sync_data();
            return Ok(true);
        }
        Ok(false)
    }

    pub fn render(&mut self, ui: &mut Ui, r: Rect, t: f32, c: Color) -> (f32, f32) {
        let w = r.w;
        let mut h = 0.;
        macro_rules! item {
            ($($b:tt)*) => {{
                $($b)*
                ui.dy(ITEM_HEIGHT);
                h += ITEM_HEIGHT;
            }}
        }
        let rr = right_rect(w);

        let data = get_data();
        let config = &data.config;
        item! {
            let rt = render_title(ui, c, tl!("item-lang"), None);
            let w = 0.06;
            let r = Rect::new(rt + 0.01, (ITEM_HEIGHT - w) / 2., w, w);
            ui.fill_rect(r, (*self.icon_lang, r, ScaleType::Fit, c));
            self.lang_btn.render(ui, rr, t, c.a);
        }
        item! {
            render_title(ui, c, tl!("item-offline"), Some(tl!("item-offline-sub")));
            render_switch(ui, rr, t, c, &mut self.offline_btn, config.offline_mode);
        }
        item! {
            render_title(ui, c, tl!("item-lowq"), Some(tl!("item-lowq-sub")));
            render_switch(ui, rr, t, c, &mut self.lowq_btn, config.sample_count == 1);
        }
        self.lang_btn.render_top(ui, t, c.a);
        (w, h)
    }
}

struct AudioList {
    adjust_btn: DRectButton,
    music_slider: Slider,
    sfx_slider: Slider,
    cali_btn: DRectButton,

    cali_task: LocalTask<Result<OffsetPage>>,
    next_page: Option<NextPage>,
}

impl AudioList {
    pub fn new() -> Self {
        Self {
            adjust_btn: DRectButton::new(),
            music_slider: Slider::new(0.0..2.0, 0.05),
            sfx_slider: Slider::new(0.0..2.0, 0.05),
            cali_btn: DRectButton::new(),

            cali_task: None,
            next_page: None,
        }
    }

    pub fn top_touch(&mut self, _touch: &Touch, _t: f32) -> bool {
        false
    }

    pub fn touch(&mut self, touch: &Touch, t: f32) -> Result<Option<bool>> {
        let data = get_data_mut();
        let config = &mut data.config;
        if self.adjust_btn.touch(touch, t) {
            config.adjust_time ^= true;
            return Ok(Some(true));
        }
        if let wt @ Some(_) = self.music_slider.touch(touch, t, &mut config.volume_music) {
            return Ok(wt);
        }
        if let wt @ Some(_) = self.sfx_slider.touch(touch, t, &mut config.volume_sfx) {
            return Ok(wt);
        }
        if self.cali_btn.touch(touch, t) {
            self.cali_task = Some(Box::pin(OffsetPage::new()));
            return Ok(Some(false));
        }
        Ok(None)
    }

    pub fn update(&mut self, t: f32) -> Result<bool> {
        if let Some(task) = &mut self.cali_task {
            if let Some(res) = poll_future(task.as_mut()) {
                match res {
                    Err(err) => show_error(err.context(tl!("load-cali-failed"))),
                    Ok(page) => {
                        self.next_page = Some(NextPage::Overlay(Box::new(page)));
                    }
                }
                self.cali_task = None;
            }
        }
        Ok(false)
    }

    pub fn render(&mut self, ui: &mut Ui, r: Rect, t: f32, c: Color) -> (f32, f32) {
        let w = r.w;
        let mut h = 0.;
        macro_rules! item {
            ($($b:tt)*) => {{
                $($b)*
                ui.dy(ITEM_HEIGHT);
                h += ITEM_HEIGHT;
            }}
        }
        let rr = right_rect(w);

        let data = get_data();
        let config = &data.config;
        item! {
            render_title(ui, c, tl!("item-adjust"), Some(tl!("item-adjust-sub")));
            render_switch(ui, rr, t, c, &mut self.adjust_btn, config.adjust_time);
        }
        item! {
            render_title(ui, c, tl!("item-music"), None);
            self.music_slider.render(ui, rr, t,c, config.volume_music, format!("{:.2}", config.volume_music));
        }
        item! {
            render_title(ui, c, tl!("item-sfx"), None);
            self.sfx_slider.render(ui, rr, t, c, config.volume_sfx, format!("{:.2}", config.volume_sfx));
        }
        item! {
            render_title(ui, c, tl!("item-cali"), None);
            self.cali_btn.render_text(ui, rr, t, c.a, format!("{:.0}ms", config.offset * 1000.), 0.5, true);
        }
        (w, h)
    }

    pub fn next_page(&mut self) -> Option<NextPage> {
        self.next_page.take()
    }
}

struct ChartList {
    autoplay_btn: DRectButton,
    dc_pause_btn: DRectButton,
    dhint_btn: DRectButton,
    opt_btn: DRectButton,
}

impl ChartList {
    pub fn new() -> Self {
        Self {
            autoplay_btn: DRectButton::new(),
            dc_pause_btn: DRectButton::new(),
            dhint_btn: DRectButton::new(),
            opt_btn: DRectButton::new(),
        }
    }

    pub fn top_touch(&mut self, _touch: &Touch, _t: f32) -> bool {
        false
    }

    pub fn touch(&mut self, touch: &Touch, t: f32) -> Result<Option<bool>> {
        let data = get_data_mut();
        let config = &mut data.config;
        if self.autoplay_btn.touch(touch, t) {
            config.autoplay ^= true;
            return Ok(Some(true));
        }
        if self.dc_pause_btn.touch(touch, t) {
            config.double_click_to_pause ^= true;
            return Ok(Some(true));
        }
        if self.dhint_btn.touch(touch, t) {
            config.double_hint ^= true;
            return Ok(Some(true));
        }
        if self.opt_btn.touch(touch, t) {
            config.aggressive ^= true;
            return Ok(Some(true));
        }
        Ok(None)
    }

    pub fn update(&mut self, _t: f32) -> Result<bool> {
        Ok(false)
    }

    pub fn render(&mut self, ui: &mut Ui, r: Rect, t: f32, c: Color) -> (f32, f32) {
        let w = r.w;
        let mut h = 0.;
        macro_rules! item {
            ($($b:tt)*) => {{
                $($b)*
                ui.dy(ITEM_HEIGHT);
                h += ITEM_HEIGHT;
            }}
        }
        let rr = right_rect(w);

        let data = get_data();
        let config = &data.config;
        item! {
            render_title(ui, c, tl!("item-autoplay"), Some(tl!("item-autoplay-sub")));
            render_switch(ui, rr, t, c, &mut self.autoplay_btn, config.autoplay);
        }
        item! {
            render_title(ui, c, tl!("item-dc-pause"), None);
            render_switch(ui, rr, t, c, &mut self.dc_pause_btn, config.double_click_to_pause);
        }
        item! {
            render_title(ui, c, tl!("item-dhint"), Some(tl!("item-dhint-sub")));
            render_switch(ui, rr, t, c, &mut self.dhint_btn, config.double_hint);
        }
        item! {
            render_title(ui, c, tl!("item-opt"), Some(tl!("item-opt-sub")));
            render_switch(ui, rr, t, c, &mut self.opt_btn, config.aggressive);
        }
        (w, h)
    }
}
