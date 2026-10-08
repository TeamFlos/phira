prpr_l10n::tl_file!("input");

use super::Ui;
use crate::{
    ext::RectExt, judge::take_wheel, ui::{DRectButton, scroll::WHEEL_STEP},
};
use macroquad::{
    input::Touch,
    prelude::*,
    miniquad::window::{clipboard_get, clipboard_set},
};

const CONTEXT_MENU_MENU_W: f32 = 0.12;
const CONTEXT_MENU_ITEM_Y: f32 = 0.04;

pub struct InlineInputBtn {
    pub input: InlineInputBox,
    pub btn: DRectButton,
}

impl InlineInputBtn {
    pub fn new() -> Self {
        Self {
            input: InlineInputBox::new(),
            btn: DRectButton::new(),
        }
    }

    pub fn set_multiline(mut self) -> Self {
        self.input = self.input.set_multiline();
        self
    }

    pub fn set_password(mut self) -> Self {
        self.input = self.input.set_password();
        self
    }

    pub fn set_centered(mut self) -> Self {
        self.input = self.input.set_centered();
        self
    }

    pub fn set_no_background_input(mut self) -> Self {
        self.input = self.input.set_no_background();
        self
    }

    pub fn touch(&mut self, touch: &Touch) {
        if self.input.is_active() {
            self.input.touch(touch);
        }
    }

    pub fn confirm(&mut self) -> Option<String> {
        if self.input.need_confirm() {
            Some(self.input.confirm())
        } else {
            None
        }
    }

    pub fn is_active(&self) -> bool {
        self.input.is_active()
    }

    pub fn activate(&mut self, touch: &Touch, t: f32, text: &str) {
        if !self.input.is_active() && self.btn.touch(touch, t) {
            self.input.activate(text);
        }
    }

    pub fn update(&mut self) {
        if self.input.is_active() {
            self.input.update();
        }
    }

    pub fn render(&mut self, ui: &mut Ui, rect: Rect, t: f32, color: Color, placeholder: &str, text: &str) {
        if self.input.is_active() {
            self.input.render(ui, rect, color.a, placeholder);
        } else {
            self.btn.render_text(ui, rect, t, text, 0.42, false);
        }
    }
}

struct ContextMenu {
    visible: bool,
    position: (f32, f32),
    rect: Rect,
    items: Vec<(Rect, String)>,
}

impl Default for ContextMenu {
    fn default() -> Self {
        Self {
            visible: false,
            position: (0.0, 0.0),
            rect: Rect::default(),
            items: vec![
                (Rect::default(), tl!("select-all").to_string()),
                (Rect::default(), tl!("copy").to_string()),
                (Rect::default(), tl!("cut").to_string()),
                (Rect::default(), tl!("paste").to_string()),
            ],
        }
    }
}

pub struct InlineInputBox {
    buffer: String,
    rect: Rect,
    multiline: bool,
    password: bool,
    centered: bool,
    render_background: bool,

    state: State,
    context_menu: ContextMenu,
}

#[derive(Default)]
enum TouchMode {
    #[default]
    None,
    Selecting,
    Scrolling,
    Wating,
}

#[derive(Default)]
struct State {
    active: bool,
    cursor: usize,
    selection_anchor: Option<usize>,
    backspace_time: Option<f64>,
    delete_time: Option<f64>,
    last_pop_time: Option<f64>,

    left_arrow_time: Option<f64>,
    right_arrow_time: Option<f64>,
    up_arrow_time: Option<f64>,
    down_arrow_time: Option<f64>,
    last_cursor_time: Option<f64>,
    last_preedit_time: Option<f64>,

    line_time: Option<f64>,
    last_line_time: Option<f64>,

    cursor_positions: Vec<(f32, f32)>,
    scroll_x: f32,
    scroll_y: f32,

    touch_start_pos: (f32, f32),
    touch_start_time: f64,
    touch_is_moved: bool,
    touch_mode: TouchMode,
    touch_start_scroll_x: f32,
    touch_start_scroll_y: f32,
    touch_scale_x: f32,
    touch_scale_y: f32,
    manual_scroll: bool,

    need_confirm: bool,
}

impl InlineInputBox {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            rect: Rect::new(0., 0., 0., 0.),
            multiline: false,
            password: false,
            centered: false,
            render_background: true,
            state: State::default(),
            context_menu: ContextMenu::default(),
        }
    }

    pub fn set_multiline(mut self) -> Self {
        self.multiline = true;
        self
    }

    pub fn set_password(mut self) -> Self {
        self.password = true;
        self
    }

    pub fn set_centered(mut self) -> Self {
        self.centered = true;
        self
    }

    pub fn set_no_background(mut self) -> Self {
        self.render_background = false;
        self
    }

    pub fn activate(&mut self, initial: &str) {
        self.state.active = true;
        self.buffer = initial.to_string();
        self.state.cursor = initial.chars().count();
        self.state.selection_anchor = None;
        self.state.backspace_time = None;
        self.state.scroll_x = 0.0;
        self.state.scroll_y = 0.0;
        self.state.manual_scroll = false;
        self.state.need_confirm = false;
        miniquad::window::set_ime_enabled(true);
        miniquad::window::show_keyboard(true);
        miniquad::window::update_text_input_state(
            initial.to_string(),
            self.state.cursor,
            self.state.selection_anchor.unwrap_or(self.state.cursor),
            self.password,
            self.multiline,
            1,
            0,
        );

        while get_char_pressed().is_some() {}
    }

    pub fn is_active(&self) -> bool {
        self.state.active
    }

    pub fn need_confirm(&self) -> bool {
        self.state.need_confirm
    }

    pub fn cancel(&mut self) {
        self.state.active = false;
        self.buffer.clear();
        self.state.selection_anchor = None;
        self.state.backspace_time = None;
        self.context_menu.visible = false;
        miniquad::window::set_ime_enabled(false);
        miniquad::window::show_keyboard(false);
        miniquad::window::update_text_input_state(
            String::new(),
            0,
            0,
            false,
            false,
            0,
            0,
        );
    }

    pub fn confirm(&mut self) -> String {
        self.state.active = false;
        self.state.selection_anchor = None;
        self.state.backspace_time = None;
        self.context_menu.visible = false;
        self.state.need_confirm = false;
        miniquad::window::set_ime_enabled(false);
        miniquad::window::show_keyboard(false);
        miniquad::window::update_text_input_state(
            String::new(),
            0,
            0,
            false,
            false,
            0,
            0,
        );
        std::mem::take(&mut self.buffer)
    }

    fn byte_at(&self, char_idx: usize) -> usize {
        self.buffer.char_indices().nth(char_idx).map(|(i, _)| i).unwrap_or(self.buffer.len())
    }

    fn remove_char_at(&mut self, idx: usize) {
        let start = self.byte_at(idx);
        let end = self.byte_at(idx + 1);
        self.buffer.replace_range(start..end, "");
    }

    fn text_before(&self) -> &str {
        let end = self.byte_at(self.state.cursor);
        &self.buffer[..end]
    }

    fn selection_range(&self) -> Option<(usize, usize)> {
        self.state.selection_anchor.map(|anchor| {
            let start = anchor.min(self.state.cursor);
            let end = anchor.max(self.state.cursor);
            (start, end)
        })
    }

    fn selected_text(&self) -> Option<String> {
        self.selection_range().map(|(start, end)| {
            let start_byte = self.byte_at(start);
            let end_byte = self.byte_at(end);
            self.buffer[start_byte..end_byte].to_string()
        })
    }

    fn delete_selection(&mut self) -> bool {
        if let Some((start, end)) = self.selection_range() {
            let start_byte = self.byte_at(start);
            let end_byte = self.byte_at(end);
            self.buffer.replace_range(start_byte..end_byte, "");
            self.state.cursor = start;
            self.state.selection_anchor = None;
            true
        } else {
            false
        }
    }

    fn update_ime(&self, ui: &Ui, cursor_screen: (f32, f32)) {
        let dpi = miniquad::window::dpi_scale();
        let (x, y) = ui.to_global(cursor_screen);
        let vp = ui.viewport;
        let asp = vp.2 as f32 / vp.3 as f32;
        let x = (x + 1.0) * 0.5 * vp.2 as f32 * dpi;
        let y = (y * asp + 1.0) * 0.5 * vp.3 as f32 * dpi;
        miniquad::window::set_ime_position(x as i32, y as i32);
    }

    fn update_ime_state(&mut self) {
        miniquad::window::update_text_input_state(
            self.buffer.clone(),
            self.state.selection_anchor.unwrap_or(self.state.cursor),
            self.state.cursor,
            self.password,
            self.multiline,
            1,
            0,
        );
    }

    fn render_preedit(&self, ui: &mut Ui, x: f32, y: f32, t: f32, center_x: Option<f32>) {
        if let Some((text, cursor)) = get_ime_preedit() {
            let display_before = &text[..cursor];
            let cursor_w = ui.text(display_before).size(0.42).measure().w;
            let mut start_x = x;
            let mut text = ui.text(&text)
                .pos(x, y)
                .anchor(0.0, 0.5)
                .size(0.42)
                .no_baseline()
                .color(Color::new(1.0, 1.0, 1.0, t));
            let mut r = text.measure();
            if let Some(cx) = center_x {
                start_x = cx - r.w * 0.5;
                text = text.pos(start_x, y);
                r.x = start_x;
            }
            text.ui.fill_rect(r, Color::new(0.0, 0.0, 0.0, t));
            text.draw();
            let cx = start_x + cursor_w;
            ui.fill_rect(Rect::new(cx - 0.001, r.top(), 0.003, r.h), Color::new(1.0, 1.0, 1.0, t * 0.9));
        }
    }

    pub fn touch(&mut self, touch: &Touch) -> bool {
        let p = touch.position;
        let in_rect = self.rect.contains(p);
        let cursor = self.find_nearest_cursor(p.x, p.y);
        match touch.phase {
            TouchPhase::Started => {}
            TouchPhase::Moved | TouchPhase::Stationary => {}
            TouchPhase::Ended | TouchPhase::Cancelled => {
                if self.context_menu.visible && !matches!(self.state.touch_mode, TouchMode::Wating) {
                    if self.context_menu.rect.contains(p) {
                        for (i, btn_rect) in self.context_menu.items.iter().enumerate() {
                            if btn_rect.0.contains(p) {
                                match i {
                                    0 => { // Select All
                                        self.state.selection_anchor = Some(0);
                                        self.state.cursor = self.buffer.chars().count();
                                        self.update_ime_state();
                                    }
                                    1 => { // Copy
                                        if let Some(text) = self.selected_text() {
                                            clipboard_set(&text);
                                        }
                                    }
                                    2 => { // Cut
                                        if let Some(text) = self.selected_text() {
                                            clipboard_set(&text);
                                            self.delete_selection();
                                            self.update_ime_state();
                                        }
                                    }
                                    3 => { // Paste
                                        if let Some(text) = clipboard_get().map(|s| s.to_string()) {
                                            self.delete_selection();
                                            let byte_pos = self.byte_at(self.state.cursor);
                                            self.buffer.insert_str(byte_pos, &text);
                                            self.state.cursor += text.chars().count();
                                            self.update_ime_state();
                                        }
                                    }
                                    _ => {}
                                }
                                self.context_menu.visible = false;
                                break;
                            }
                        }
                    } else {
                        self.context_menu.visible = false;
                    }
                }
            }
        }
        if is_mouse_button_down(MouseButton::Left) || is_mouse_button_released(MouseButton::Left) {
            match touch.phase {
                TouchPhase::Moved | TouchPhase::Stationary => {
                    if !self.context_menu.visible {
                        self.state.cursor = cursor;
                        self.update_ime_state();
                    }
                    false
                }
                TouchPhase::Ended | TouchPhase::Cancelled => {
                    false
                }
                TouchPhase::Started => {
                    if !self.context_menu.visible && in_rect {
                        self.state.cursor = cursor;
                        self.state.selection_anchor = Some(cursor);
                        self.state.manual_scroll = false;
                        self.update_ime_state();
                    }
                    if !self.context_menu.visible && !in_rect {
                        self.state.need_confirm = true;
                    }
                    !in_rect
                }
            }
        } else if is_mouse_button_down(MouseButton::Right) || is_mouse_button_released(MouseButton::Right) {
            match touch.phase {
                TouchPhase::Moved | TouchPhase::Stationary => {
                    false
                }
                TouchPhase::Ended | TouchPhase::Cancelled => {
                    if in_rect {
                        if !self.password && self.rect.contains(p) && !self.context_menu.visible {
                            self.context_menu.visible = true;
                            self.context_menu.position = (
                                p.x.max(self.rect.x).min(self.rect.right() - CONTEXT_MENU_MENU_W),
                                p.y.max(self.rect.y).min(self.rect.bottom() - CONTEXT_MENU_ITEM_Y * self.context_menu.items.len() as f32)
                            );
                        }
                    } else {
                        self.state.need_confirm = true;
                    }
                    false
                }
                TouchPhase::Started => {
                    !in_rect
                }
            }
        } else {
            match touch.phase {
                TouchPhase::Started => {
                    if in_rect {
                        self.state.touch_start_pos = (p.x, p.y);
                        self.state.touch_start_time = get_time();
                        self.state.touch_mode = TouchMode::None;
                        self.state.touch_start_scroll_x = self.state.scroll_x;
                        self.state.touch_start_scroll_y = self.state.scroll_y;
                        self.state.touch_is_moved = false;
                    } else {
                        self.state.need_confirm = true;
                    }
                    !in_rect
                }
                TouchPhase::Moved | TouchPhase::Stationary => {
                    let dx = p.x - self.state.touch_start_pos.0;
                    let dy = p.y - self.state.touch_start_pos.1;
                    let dist = (dx * dx + dy * dy).sqrt();
                    if !self.state.touch_is_moved && dist > 0.05 {
                        self.state.touch_is_moved = true;
                    }
                    let dt = get_time() - self.state.touch_start_time;

                    match self.state.touch_mode {
                        TouchMode::None => {
                            if self.state.touch_is_moved {
                                self.state.touch_mode = TouchMode::Scrolling;
                                self.state.manual_scroll = true;
                            } else if dt > 0.5 && self.state.selection_anchor.is_none() {
                                self.state.touch_mode = TouchMode::Selecting;
                                self.state.cursor = cursor;
                                self.state.selection_anchor = Some(cursor);
                                self.state.manual_scroll = false;
                                self.update_ime_state();
                            }
                        }
                        TouchMode::Scrolling => {
                            let dx_ui = dx * self.state.touch_scale_x;
                            let dy_ui = dy * self.state.touch_scale_y;
                            self.state.scroll_x = self.state.touch_start_scroll_x - dx_ui;
                            self.state.scroll_y = self.state.touch_start_scroll_y - dy_ui;
                        }
                        TouchMode::Selecting => {
                            self.state.cursor = cursor;
                            self.update_ime_state();
                        }
                        _ => {}
                    }
                    false
                }
                TouchPhase::Ended | TouchPhase::Cancelled => {
                    let dt = get_time() - self.state.touch_start_time;
                    if dt > 0.5 && !self.state.touch_is_moved && !self.password && self.rect.contains(p) && !self.context_menu.visible {
                        self.context_menu.visible = true;
                        self.context_menu.position = (
                            self.state.cursor_positions.get(self.state.cursor).map_or(p.x, |&(x, _)| x)
                                .max(self.rect.x).min(self.rect.right() - CONTEXT_MENU_MENU_W),
                            self.state.cursor_positions.get(self.state.cursor).map_or(p.y, |&(_, y)| y)
                                .max(self.rect.y).min(self.rect.bottom() - CONTEXT_MENU_ITEM_Y * self.context_menu.items.len() as f32),
                        );
                        self.state.touch_mode = TouchMode::Wating;
                    }
                    if matches!(self.state.touch_mode, TouchMode::None) && in_rect && !self.context_menu.visible {
                        self.state.cursor = cursor;
                        self.state.selection_anchor = None;
                        self.state.manual_scroll = false;
                        miniquad::window::show_keyboard(true);
                        self.update_ime_state();
                    }
                    self.state.touch_mode = TouchMode::None;
                    false
                }
            }
        }
    }

    fn find_nearest_cursor(&self, touch_x: f32, touch_y: f32) -> usize {
        if self.state.cursor_positions.is_empty() {
            return 0;
        }
        let mut best_idx = 0;
        let mut best_dist = f32::MAX;
        for (i, &(px, py)) in self.state.cursor_positions.iter().enumerate() {
            let dx = touch_x - px;
            let dy = touch_y - py - 0.0175;
            let dist = dx * dx + dy * dy * 10000.0;
            if dist < best_dist {
                best_dist = dist;
                best_idx = i;
            }
        }
        best_idx
    }

    fn find_nearest_col_cursor(&self, from_cursor: usize, to_cursor: usize) -> usize {
        if self.state.cursor_positions.is_empty() {
            return 0;
        }
        let mut best_idx = 0;
        let mut best_dist = f32::MAX;
        let x = self.state.cursor_positions.get(from_cursor).map_or(0.0, |&(x, _)| x);
        let y = self.state.cursor_positions.get(to_cursor).map_or(0.0, |&(_, y)| y);
        for (i, &(px, py)) in self.state.cursor_positions.iter().enumerate() {
            let dx = x - px;
            let dy = y - py;
            let dist = dx * dx + dy * dy * 10000.0;
            if dist < best_dist {
                best_dist = dist;
                best_idx = i;
            }
        }
        best_idx
    }

    fn cursor_right(&mut self, shift: bool) {
        if shift {
            if self.state.selection_anchor.is_none() {
                self.state.selection_anchor = Some(self.state.cursor);
            }
        } else {
            self.state.selection_anchor = None;
        }
        if self.state.cursor < self.buffer.chars().count() {
            self.state.cursor += 1;
        }
    }

    fn cursor_left(&mut self, shift: bool) {
        if shift {
            if self.state.selection_anchor.is_none() {
                self.state.selection_anchor = Some(self.state.cursor);
            }
        } else {
            self.state.selection_anchor = None;
        }
        if self.state.cursor > 0 {
            self.state.cursor -= 1;
        }
    }

    fn cursor_up(&mut self, shift: bool) {
        if shift {
            if self.state.selection_anchor.is_none() {
                self.state.selection_anchor = Some(self.state.cursor);
            }
        } else {
            self.state.selection_anchor = None;
        }
        let before = self.text_before();
        if let Some(line_start) = before.rfind('\n') {
            let col = before.len() - line_start - 1;
            let prev_line = &before[..line_start];
            let prev_start = prev_line.rfind('\n').map(|i| i + 1).unwrap_or(0);
            let prev_col = col.min(line_start - prev_start);
            let target_byte = prev_start + prev_col;
            let target_byte_adj = self.find_nearest_col_cursor(self.state.cursor, target_byte);
            self.state.cursor = self.buffer.char_indices().take_while(|(i, _)| *i < target_byte_adj).count();
        }
    }

    fn cursor_down(&mut self, shift: bool) {
        if shift {
            if self.state.selection_anchor.is_none() {
                self.state.selection_anchor = Some(self.state.cursor);
            }
        } else {
            self.state.selection_anchor = None;
        }
        let before = self.text_before();
        let before_byte = self.byte_at(self.state.cursor);
        let line_start_byte = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
        let col = before_byte - line_start_byte;
        let after = &self.buffer[before_byte..];
        if let Some(rel_nl) = after.find('\n') {
            let next_line_start = before_byte + rel_nl + 1;
            let next_line_end = self.buffer[next_line_start..].find('\n').map(|i| next_line_start + i).unwrap_or(self.buffer.chars().count());
            let next_line_len = next_line_end - next_line_start;
            let target_col = col.min(next_line_len);
            let target_byte = next_line_start + target_col;
            let target_byte_adj = self.find_nearest_col_cursor(self.state.cursor, target_byte);
            self.state.cursor = self.buffer.char_indices().take_while(|(i, _)| *i < target_byte_adj).count();
        }
    }

    pub fn update(&mut self) {
        let now = get_time();
        let ctrl = is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl);
        let shift = is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);

        if get_ime_preedit().is_some() {
            self.state.last_preedit_time = Some(now);
            return;
        }

        // Arrow keys
        if is_key_pressed(KeyCode::Right) {
            self.state.right_arrow_time = Some(now);
            self.cursor_right(shift);
            self.state.manual_scroll = false;
            self.update_ime_state();
        } else if let Some(arrow_time) = self.state.right_arrow_time {
            if is_key_down(KeyCode::Right) {
                if now - arrow_time > 0.5 {
                    if self.state.last_cursor_time.map_or(true, |t| now - t > 0.02) {
                        self.state.last_cursor_time = Some(now);
                        self.cursor_right(shift);
                        self.update_ime_state();
                    }
                }
            } else {
                self.state.right_arrow_time = None;
            }
        }
        if is_key_pressed(KeyCode::Left) {
            self.state.left_arrow_time = Some(now);
            self.cursor_left(shift);
            self.state.manual_scroll = false;
            self.update_ime_state();
        } else if let Some(arrow_time) = self.state.left_arrow_time {
            if is_key_down(KeyCode::Left) {
                if now - arrow_time > 0.5 {
                    if self.state.last_cursor_time.map_or(true, |t| now - t > 0.02) {
                        self.state.last_cursor_time = Some(now);
                        self.cursor_left(shift);
                        self.update_ime_state();
                    }
                }
            } else {
                self.state.left_arrow_time = None;
            }
        }
        if self.multiline {
            if is_key_pressed(KeyCode::Up) {
                self.state.up_arrow_time = Some(now);
                self.cursor_up(shift);
                self.state.manual_scroll = false;
                self.update_ime_state();
            } else if let Some(arrow_time) = self.state.up_arrow_time {
                if is_key_down(KeyCode::Up) {
                    if now - arrow_time > 0.5 {
                        if self.state.last_cursor_time.map_or(true, |t| now - t > 0.02) {
                            self.state.last_cursor_time = Some(now);
                            self.cursor_up(shift);
                            self.update_ime_state();
                        }
                    }
                } else {
                    self.state.up_arrow_time = None;
                }
            }
            if is_key_pressed(KeyCode::Down) {
                self.state.down_arrow_time = Some(now);
                self.cursor_down(shift);
                self.state.manual_scroll = false;
                self.update_ime_state();
            } else if let Some(arrow_time) = self.state.down_arrow_time {
                if is_key_down(KeyCode::Down) {
                    if now - arrow_time > 0.5 {
                        if self.state.last_cursor_time.map_or(true, |t| now - t > 0.02) {
                            self.state.last_cursor_time = Some(now);
                            self.cursor_down(shift);
                            self.update_ime_state();
                        }
                    }
                } else {
                    self.state.down_arrow_time = None;
                }
            }
        }
        if is_key_pressed(KeyCode::Home) {
            if shift {
                if self.state.selection_anchor.is_none() {
                    self.state.selection_anchor = Some(self.state.cursor);
                }
            } else {
                self.state.selection_anchor = None;
            }
            let before = self.text_before();
            self.state.cursor = before.rfind('\n').map(|i| self.buffer[..i].chars().count() + 1).unwrap_or(0);
            self.state.manual_scroll = false;
            self.update_ime_state();
        }
        if is_key_pressed(KeyCode::End) {
            if shift {
                if self.state.selection_anchor.is_none() {
                    self.state.selection_anchor = Some(self.state.cursor);
                }
            } else {
                self.state.selection_anchor = None;
            }
            let after_byte = self.byte_at(self.state.cursor);
            self.state.cursor = self.buffer[after_byte..].find('\n').map(|i| {
                self.buffer[..after_byte + i].chars().count()
            }).unwrap_or(self.buffer.chars().count());
            self.state.manual_scroll = false;
            self.update_ime_state();
        }

        // Copy/Paste/Cut
        if ctrl & !self.password {
            if is_key_pressed(KeyCode::C) {
                if let Some(text) = self.selected_text() {
                    clipboard_set(&text);
                }
            }
            if is_key_pressed(KeyCode::X) {
                if let Some(text) = self.selected_text() {
                    clipboard_set(&text);
                    self.delete_selection();
                    self.update_ime_state();
                }
            }
            if is_key_pressed(KeyCode::V) {
                if let Some(text) = clipboard_get().map(|s| s.to_string()) {
                    // Delete selection first
                    self.delete_selection();
                    let byte_pos = self.byte_at(self.state.cursor);
                    self.buffer.insert_str(byte_pos, &text);
                    self.state.cursor += text.chars().count();
                    self.update_ime_state();
                }
            }
            if is_key_pressed(KeyCode::A) {
                self.state.selection_anchor = Some(0);
                self.state.cursor = self.buffer.chars().count();
                self.update_ime_state();
            }
        }

        let block_remove = self.state.last_preedit_time.map_or(false, |t| now - t < 0.25);

        if is_key_pressed(KeyCode::Backspace) && !block_remove {
            self.state.backspace_time = Some(now);
            if !self.delete_selection() {
                if self.state.cursor > 0 {
                    self.state.cursor -= 1;
                    self.remove_char_at(self.state.cursor);
                    self.state.manual_scroll = false;
                    self.update_ime_state();
                }
            }
        } else if let Some(backspace_time) = self.state.backspace_time {
            if is_key_down(KeyCode::Backspace) {
                if now - backspace_time > 0.5 {
                    if self.state.last_pop_time.map_or(true, |t| now - t > 0.02) {
                        self.state.last_pop_time = Some(now);
                        if !self.delete_selection() {
                            if self.state.cursor > 0 {
                                self.state.cursor -= 1;
                                self.remove_char_at(self.state.cursor);
                                self.update_ime_state();
                            }
                        }
                    }
                }
            } else {
                self.state.backspace_time = None;
            }
        }

        // Delete key
        if is_key_pressed(KeyCode::Delete) && !block_remove {
            self.state.delete_time = Some(now);
            if !self.delete_selection() {
                if self.state.cursor < self.buffer.chars().count() {
                    self.remove_char_at(self.state.cursor);
                    self.state.manual_scroll = false;
                    self.update_ime_state();
                }
            }
        } else if let Some(delete_time) = self.state.delete_time {
            if is_key_down(KeyCode::Delete) {
                if now - delete_time > 0.5 {
                    if self.state.last_pop_time.map_or(true, |t| now - t > 0.02) {
                        self.state.last_pop_time = Some(now);
                        if !self.delete_selection() {
                            if self.state.cursor < self.buffer.chars().count() {
                                self.remove_char_at(self.state.cursor);
                                self.update_ime_state();
                            }
                        }
                    }
                }
            } else {
                self.state.delete_time = None;
            }
        }

        if self.multiline {
            // Enter key
            if is_key_pressed(KeyCode::Enter) {
                self.state.line_time = Some(now);
                self.delete_selection();
                let byte_pos = self.byte_at(self.state.cursor);
                self.buffer.insert(byte_pos, '\n');
                self.state.cursor += 1;
                self.state.manual_scroll = false;
                self.update_ime_state();
            } else if let Some(line_time) = self.state.line_time {
                if is_key_down(KeyCode::Enter) {
                    if now - line_time > 0.5 {
                        if self.state.last_line_time.map_or(true, |t| now - t > 0.02) {
                            self.state.last_line_time = Some(now);
                            self.delete_selection();
                            let byte_pos = self.byte_at(self.state.cursor);
                            self.buffer.insert(byte_pos, '\n');
                            self.state.cursor += 1;
                            self.update_ime_state();
                        }
                    }
                } else {
                    self.state.line_time = None;
                }
            }
        }

        // Character input
        while let Some(ch) = get_char_pressed() {
            if !ch.is_control() {
                // Delete selection first if any
                self.delete_selection();
                let byte_pos = self.byte_at(self.state.cursor);
                self.buffer.insert(byte_pos, ch);
                self.state.cursor += 1;
                self.state.manual_scroll = false;
                self.update_ime_state();
            }
        }

        while let Some(ImeCommit::Text(text)) = get_ime_commit() {
            if !text.is_empty() {
                self.delete_selection();
                let byte_pos = self.byte_at(self.state.cursor);
                self.buffer.insert_str(byte_pos, &text);
                self.state.cursor += text.chars().count();
                self.state.manual_scroll = false;
                self.update_ime_state();
            }
        }

        if let Some(ime_state) = get_ime_state() {
            if ime_state.text.trim_end() != self.buffer.trim_end() {
                self.buffer = ime_state.text;
            }
            self.state.cursor = ime_state.selection_end;
            if ime_state.selection_start == ime_state.selection_end {
                self.state.selection_anchor = None;
            } else {
                self.state.selection_anchor = Some(ime_state.selection_start);
            }
        }

        if !self.multiline && is_key_pressed(KeyCode::Enter) {
            self.state.need_confirm = true;
        }

        if is_key_pressed(KeyCode::Escape) {
            self.cancel();
        }

        // Mouse wheel
        if self.multiline {
            let (x, y) = take_wheel();
            if shift && (x.abs() > 1e-5 || y.abs() > 1e-5) {
                self.state.scroll_x -= x * WHEEL_STEP;
                self.state.scroll_x -= y * WHEEL_STEP;
                self.state.manual_scroll = true;
            } else if x.abs() > 1e-5 || y.abs() > 1e-5 {
                self.state.scroll_x -= x * WHEEL_STEP;
                self.state.scroll_y -= y * WHEEL_STEP;
                self.state.manual_scroll = true;
            }
        }
    }

    pub fn render(&mut self, ui: &mut Ui, rect: Rect, t: f32, placeholder: &str) {
        let global_rect = ui.rect_to_global(rect);
        let screen = ui.screen_rect().feather(-0.04);
        let w = global_rect.w.min(screen.w).max(0.0);
        let h = global_rect.h.min(screen.h).max(0.0);
        let global_rect = Rect::new(
            global_rect.x.clamp(screen.x, screen.right() - w),
            global_rect.y.clamp(screen.y, screen.bottom() - h),
            w,
            h,
        );
        self.rect = global_rect;
        let rect = ui.rect_to_local(global_rect);
        self.state.touch_scale_x = if self.rect.w > 0.0 { rect.w / self.rect.w } else { 1.0 };
        self.state.touch_scale_y = if self.rect.h > 0.0 { rect.h / self.rect.h } else { 1.0 };
        let bx = rect.x;
        let by = rect.y;
        let bw = rect.w;
        let bh = rect.h;

        if self.render_background {
            ui.fill_path(
                &Rect::new(bx, by, bw, bh).rounded(0.008),
                Color::new(0.35, 0.5, 1.0, t),
            );
            ui.fill_path(
                &Rect::new(bx + 0.002, by + 0.002, bw - 0.004, bh - 0.004).rounded(0.006),
                Color::new(0.15, 0.15, 0.18, t),
            );
        }

        let line_h = ui.text("0").size(0.42).measure().h;
        let text_x = bx + 0.02;
        let max_w = bw - 0.04;
        let max_h = bh - 0.04;
        let clip = Rect::new(bx + 0.002, by + 0.002, bw - 0.004, bh - 0.004);
        ui.scissor(clip, |ui| {
            if self.multiline {
                let text_y = by + 0.02;
                let line_h_with_space = ui.text("0\n0").size(0.42).multiline().measure().h - line_h;
                if self.buffer.is_empty() {
                    let ph_w = ui.text(placeholder).size(0.42).measure().w;
                    let ph_x = if self.centered { bx + (bw - ph_w) * 0.5 } else { text_x };
                    let cursor_x = if self.centered { bx + bw * 0.5 } else { text_x };
                    ui.text(placeholder)
                        .pos(ph_x, text_y)
                        .anchor(0.0, 0.0)
                        .no_baseline()
                        .size(0.42)
                        .color(Color::new(1.0, 1.0, 1.0, t * 0.3))
                        .draw();
                    ui.fill_rect(Rect::new(cursor_x, text_y, 0.003, line_h + 0.01), Color::new(1.0, 1.0, 1.0, t * 0.9));
                    self.update_ime(ui, (cursor_x, text_y));
                    self.state.cursor_positions.clear();
                    self.state.cursor_positions.push(ui.to_global((cursor_x, text_y)));
                    self.render_preedit(ui, cursor_x + 0.003, text_y + line_h / 2., t, if self.centered { Some(bx + bw * 0.5) } else { None });
                    return;
                }
                let display = if self.password {
                    &self.buffer.chars().map(|_| '•').collect::<String>()
                } else {
                    &self.buffer
                };
                let display_before = if self.password {
                    &self.text_before().chars().map(|_| '•').collect::<String>()
                } else {
                    self.text_before()
                };
                let line_start = display_before.rfind('\n').map(|i| i + 1).unwrap_or(0);
                let cursor_line_text = &display_before[line_start..];
                let line_num = display_before.chars().filter(|c| *c == '\n').count() as f32;
                let cursor_w = ui.text(cursor_line_text).size(0.42).multiline().measure().w;
                let cursor_y = line_num * line_h_with_space;
                let full_text = ui.text(display).size(0.42).multiline().measure();
                let text_x_adj = if full_text.w > max_w {
                    if self.state.manual_scroll {
                        self.state.scroll_x = self.state.scroll_x.clamp(0.0, full_text.w - max_w);
                    } else {
                        let margin = max_w * 0.1;
                        let lo = (cursor_w - max_w + margin).max(0.0);
                        let hi = (cursor_w - margin).max(0.0).min(full_text.w - max_w);
                        if lo <= hi {
                            self.state.scroll_x = self.state.scroll_x.clamp(lo, hi);
                        } else {
                            self.state.scroll_x = hi;
                        }
                    }
                    text_x - self.state.scroll_x
                } else {
                    self.state.scroll_x = 0.0;
                    text_x
                };
                let text_y_adj = if full_text.h > max_h {
                    if self.state.manual_scroll {
                        let max_scroll = (full_text.h - max_h).max(0.0);
                        self.state.scroll_y = self.state.scroll_y.clamp(0.0, max_scroll);
                    } else {
                        let margin = max_h * 0.1;
                        let lo = (cursor_y + line_h - max_h + margin).max(0.0);
                        let hi = (cursor_y - margin).max(0.0).min(full_text.h - max_h);
                        if lo <= hi {
                            self.state.scroll_y = self.state.scroll_y.clamp(lo, hi);
                        } else {
                            self.state.scroll_y = hi;
                        }
                    }
                    text_y - self.state.scroll_y
                } else {
                    self.state.scroll_y = 0.0;
                    text_y
                };
                let cursor_y_adj = text_y_adj + line_num * line_h_with_space;
                let centered = self.centered && full_text.w <= max_w;
                let line_widths: Option<Vec<f32>> = if centered {
                    Some(display.split('\n').map(|line| ui.text(line).size(0.42).multiline().measure().w).collect())
                } else {
                    None
                };
                let line_x = |idx: usize| -> f32 {
                    match &line_widths {
                        Some(ws) => bx + (bw - ws[idx]) * 0.5,
                        None => text_x_adj,
                    }
                };
                if let Some((sel_start, sel_end)) = self.selection_range() {
                    let mut char_offset = 0usize;
                    for (line_idx, line) in display.split('\n').enumerate() {
                        let line_len = line.chars().count();
                        let line_char_start = char_offset;
                        let line_char_end = char_offset + line_len;

                        let overlap_start = sel_start.max(line_char_start).min(line_char_end);
                        let overlap_end = sel_end.max(line_char_start).min(line_char_end);

                        if overlap_start < overlap_end {
                            let sel_start_in_line = overlap_start - line_char_start;
                            let sel_end_in_line = overlap_end - line_char_start;

                            let start_byte = line.char_indices().nth(sel_start_in_line).map(|(i, _)| i).unwrap_or(line.len());
                            let end_byte = line.char_indices().nth(sel_end_in_line).map(|(i, _)| i).unwrap_or(line.len());

                            let start_w = if start_byte == 0 { 0.0 } else { ui.text(&line[..start_byte]).size(0.42).multiline().measure().w };
                            let end_w = if end_byte == 0 { 0.0 } else { ui.text(&line[..end_byte]).size(0.42).multiline().measure().w };

                            let y = text_y_adj + line_idx as f32 * line_h_with_space;
                            let x = line_x(line_idx) + start_w;
                            let w = end_w - start_w;
                            if w > 0.0 {
                                ui.fill_rect(Rect::new(x, y, w, line_h + 0.01), Color::new(0.3, 0.5, 1.0, t * 0.3));
                            }
                        }

                        char_offset += line_len + 1;
                    }
                }
                if line_widths.is_some() {
                    for (line_idx, line) in display.split('\n').enumerate() {
                        ui.text(line)
                            .pos(line_x(line_idx), text_y_adj + line_idx as f32 * line_h_with_space)
                            .size(0.42)
                            .color(Color::new(1.0, 1.0, 1.0, t))
                            .multiline()
                            .draw();
                    }
                } else {
                    ui.text(display)
                        .pos(text_x_adj, text_y_adj)
                        .size(0.42)
                        .color(Color::new(1.0, 1.0, 1.0, t))
                        .multiline()
                        .draw();
                }
                let cx = line_x(line_num as usize) + cursor_w;
                ui.fill_rect(Rect::new(cx, cursor_y_adj, 0.003, line_h + 0.01), Color::new(1.0, 1.0, 1.0, t * 0.9));
                self.update_ime(ui, (cx, cursor_y_adj + 0.002));
                self.state.cursor_positions.clear();
                let chars_count = display.chars().count();
                let mut line_num_cur = 0usize;
                let mut line_start_byte = 0usize;
                for i in 0..=chars_count {
                    if i > 0 {
                        let prev_byte = display.char_indices().nth(i - 1).map(|(j, _)| j).unwrap_or(display.len());
                        if display.as_bytes()[prev_byte] == b'\n' {
                            line_num_cur += 1;
                            line_start_byte = prev_byte + 1;
                        }
                    }
                    let byte_pos = display.char_indices().nth(i).map(|(j, _)| j).unwrap_or(display.len());
                    let line_text = &display[line_start_byte..byte_pos];
                    let w = if line_text.is_empty() {
                        0.0
                    } else {
                        ui.text(line_text).size(0.42).multiline().measure().w
                    };
                    let x = line_x(line_num_cur) + w;
                    let y = text_y_adj + line_num_cur as f32 * line_h_with_space;
                    self.state.cursor_positions.push(ui.to_global((x, y)));
                }
                self.render_preedit(ui, cx + 0.003, cursor_y_adj + line_h / 2. + 0.002, t, if self.centered { Some(bx + bw * 0.5) } else { None });
            } else {
                if self.buffer.is_empty() {
                    let text_y = by + bh * 0.5;
                    let ph_w = ui.text(placeholder).size(0.42).measure().w;
                    let ph_x = if self.centered { bx + (bw - ph_w) * 0.5 } else { text_x };
                    let cursor_x = if self.centered { bx + bw * 0.5 } else { text_x };
                    ui.text(placeholder)
                        .pos(ph_x, text_y)
                        .anchor(0.0, 0.5)
                        .no_baseline()
                        .size(0.42)
                        .color(Color::new(1.0, 1.0, 1.0, t * 0.3))
                        .draw();
                    let cursor_y = by + 0.01;
                    ui.fill_rect(Rect::new(cursor_x, cursor_y, 0.003, bh - 0.02), Color::new(1.0, 1.0, 1.0, t * 0.9));
                    self.update_ime(ui, (cursor_x, text_y - line_h * 0.5));
                    self.state.cursor_positions.clear();
                    self.state.cursor_positions.push(ui.to_global((cursor_x, text_y)));
                    self.render_preedit(ui, cursor_x + 0.003, text_y, t, if self.centered { Some(bx + bw * 0.5) } else { None });
                    return;
                }
                let text_y = by + bh * 0.5;
                let display = if self.password {
                    &self.buffer.chars().map(|_| '•').collect::<String>()
                } else {
                    &self.buffer
                };
                let display_before = if self.password {
                    &self.text_before().chars().map(|_| '•').collect::<String>()
                } else {
                    self.text_before()
                };
                let cursor_w = ui.text(display_before).size(0.42).measure().w;
                let full_w = ui.text(display).size(0.42).measure().w;
                let text_x_adj = if full_w > max_w {
                    let margin = max_w * 0.1;
                    let lo = (cursor_w - max_w + margin).max(0.0);
                    let hi = (cursor_w - margin).max(0.0).min(full_w - max_w);
                    if lo <= hi {
                        self.state.scroll_x = self.state.scroll_x.clamp(lo, hi);
                    } else {
                        self.state.scroll_x = hi;
                    }
                    text_x - self.state.scroll_x
                } else {
                    self.state.scroll_x = 0.0;
                    if self.centered {
                        bx + (bw - full_w) * 0.5
                    } else {
                        text_x
                    }
                };
                // Draw selection highlight
                if let Some((sel_start, sel_end)) = self.selection_range() {
                    let start_before = &display[..display.char_indices().nth(sel_start).map(|(i, _)| i).unwrap_or(display.len())];
                    let end_before = &display[..display.char_indices().nth(sel_end).map(|(i, _)| i).unwrap_or(display.len())];
                    let sel_start_w = ui.text(start_before).size(0.42).measure().w;
                    let sel_end_w = ui.text(end_before).size(0.42).measure().w;
                    let sel_x = text_x_adj + sel_start_w;
                    let sel_w = sel_end_w - sel_start_w;
                    ui.fill_rect(Rect::new(sel_x, by + 0.01, sel_w, bh - 0.02), Color::new(0.3, 0.5, 1.0, t * 0.3));
                }
                ui.text(display)
                    .pos(text_x_adj, text_y)
                    .anchor(0.0, 0.5)
                    .no_baseline()
                    .size(0.42)
                    .color(Color::new(1.0, 1.0, 1.0, t))
                    .draw();
                let cx = text_x_adj + cursor_w;
                ui.fill_rect(Rect::new(cx, by + 0.01, 0.003, bh - 0.02), Color::new(1.0, 1.0, 1.0, t * 0.9));
                self.update_ime(ui, (cx, text_y - line_h * 0.5));
                self.state.cursor_positions.clear();
                let chars_count = display.chars().count();
                for i in 0..=chars_count {
                    let before_i = &display[..display.char_indices().nth(i).map(|(j, _)| j).unwrap_or(display.len())];
                    let w = ui.text(before_i).size(0.42).measure().w;
                    let x = text_x_adj + w;
                    self.state.cursor_positions.push(ui.to_global((x, text_y)));
                }
                self.render_preedit(ui, cx + 0.003, text_y, t, if self.centered { Some(bx + bw * 0.5) } else { None });
            }
        });

        if self.context_menu.visible && !self.password {
            let menu_h = CONTEXT_MENU_ITEM_Y * self.context_menu.items.len() as f32;
            let local_pos = ui.to_local(self.context_menu.position);
            let menu_x = local_pos.0;
            let menu_y = local_pos.1;

            let menu_rect = Rect::new(menu_x, menu_y, CONTEXT_MENU_MENU_W, menu_h);
            self.context_menu.rect = ui.rect_to_global(menu_rect);

            ui.fill_path(&menu_rect.rounded(0.006), Color::new(0.2, 0.2, 0.22, 1.0));

            for (i, item) in self.context_menu.items.iter_mut().enumerate() {
                let btn_rect = Rect::new(menu_x + 0.005, menu_y + i as f32 * CONTEXT_MENU_ITEM_Y + 0.005, CONTEXT_MENU_MENU_W - 0.01, CONTEXT_MENU_ITEM_Y - 0.01);
                item.0 = ui.rect_to_global(btn_rect);

                ui.text(&item.1)
                    .pos(menu_x + 0.02, menu_y + i as f32 * CONTEXT_MENU_ITEM_Y + CONTEXT_MENU_ITEM_Y * 0.5)
                    .anchor(0.0, 0.5)
                    .no_baseline()
                    .size(0.38)
                    .color(Color::new(1.0, 1.0, 1.0, t))
                    .draw();
            }
        }
    }
}
