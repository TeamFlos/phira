use super::Ui;
use crate::{
    ext::RectExt,
};
use macroquad::{
    input::Touch,
    prelude::*,
    miniquad::window::{clipboard_get, clipboard_set},
};

pub struct InlineInputBox {
    buffer: String,
    rect: Rect,
    multiline: bool,

    state: State,
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
    last_cursor_time: Option<f64>,

    cursor_positions: Vec<(f32, f32)>,
    scroll_x: f32,
    scroll_y: f32,
}

impl InlineInputBox {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            rect: Rect::new(0., 0., 0., 0.),
            multiline: false,
            state: State::default()
        }
    }

    pub fn activate(&mut self, initial: &str, multiline: bool) {
        self.state.active = true;
        self.buffer = initial.to_string();
        self.multiline = multiline;
        self.state.cursor = initial.chars().count();
        self.state.selection_anchor = None;
        self.state.backspace_time = None;
        self.state.scroll_x = 0.0;
        self.state.scroll_y = 0.0;
        miniquad::window::set_ime_enabled(true);
        miniquad::window::show_keyboard(true);
    }

    pub fn is_active(&self) -> bool {
        self.state.active
    }

    pub fn cancel(&mut self) {
        self.state.active = false;
        self.buffer.clear();
        self.state.selection_anchor = None;
        self.state.backspace_time = None;
        miniquad::window::set_ime_enabled(false);
        miniquad::window::show_keyboard(false);
    }

    pub fn confirm(&mut self) -> String {
        self.state.active = false;
        self.state.selection_anchor = None;
        self.state.backspace_time = None;
        miniquad::window::set_ime_enabled(false);
        miniquad::window::show_keyboard(false);
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

    pub fn touch(&mut self, touch: &Touch) -> bool {
        let p = touch.position;
        let in_rect = self.rect.contains(p);
        let cursor = self.find_nearest_cursor(p.x, p.y);
        match touch.phase {
            TouchPhase::Moved => {
                if in_rect {
                    self.state.cursor = cursor;
                }
                false
            }
            TouchPhase::Stationary | TouchPhase::Ended | TouchPhase::Cancelled => {
                false
            }
            TouchPhase::Started => {
                if in_rect {
                    self.state.cursor = cursor;
                    self.state.selection_anchor = Some(cursor);
                }
                !in_rect
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
            let dy = touch_y - py;
            let dist = dx * dx + dy * dy * 10000.0;
            if dist < best_dist {
                best_dist = dist;
                best_idx = i;
            }
        }
        best_idx
    }

    pub fn update(&mut self) {
        let now = get_time();
        let ctrl = is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl);
        let shift = is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);

        // Arrow keys
        if is_key_pressed(KeyCode::Right) {
            self.state.right_arrow_time = Some(now);
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
        } else if let Some(arrow_time) = self.state.right_arrow_time {
            if is_key_down(KeyCode::Right) {
                if now - arrow_time > 0.5 {
                    if self.state.last_cursor_time.map_or(true, |t| now - t > 0.02) {
                        self.state.last_cursor_time = Some(now);
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
                }
            } else {
                self.state.right_arrow_time = None;
            }
        }
        if is_key_pressed(KeyCode::Left) {
            self.state.left_arrow_time = Some(now);
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
        } else if let Some(arrow_time) = self.state.left_arrow_time {
            if is_key_down(KeyCode::Left) {
                if now - arrow_time > 0.5 {
                    if self.state.last_cursor_time.map_or(true, |t| now - t > 0.02) {
                        self.state.last_cursor_time = Some(now);
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
                }
            } else {
                self.state.left_arrow_time = None;
            }
        }
        if self.multiline {
            if is_key_pressed(KeyCode::Up) {
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
                    self.state.cursor = self.buffer.char_indices().take_while(|(i, _)| *i < target_byte).count();
                }
            }
            if is_key_pressed(KeyCode::Down) {
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
                    self.state.cursor = self.buffer.char_indices().take_while(|(i, _)| *i < next_line_start + target_col).count();
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
        }

        // Copy/Paste/Cut
        if ctrl {
            if is_key_pressed(KeyCode::C) {
                if let Some(text) = self.selected_text() {
                    clipboard_set(&text);
                }
            }
            if is_key_pressed(KeyCode::X) {
                if let Some(text) = self.selected_text() {
                    clipboard_set(&text);
                    self.delete_selection();
                }
            }
            if is_key_pressed(KeyCode::V) {
                if let Some(text) = clipboard_get().map(|s| s.to_string()) {
                    // Delete selection first
                    self.delete_selection();
                    let byte_pos = self.byte_at(self.state.cursor);
                    self.buffer.insert_str(byte_pos, &text);
                    self.state.cursor += text.chars().count();
                }
            }
            if is_key_pressed(KeyCode::A) {
                self.state.selection_anchor = Some(0);
                self.state.cursor = self.buffer.chars().count();
            }
        }

        if is_key_pressed(KeyCode::Backspace) {
            self.state.backspace_time = Some(now);
            if !self.delete_selection() {
                if self.state.cursor > 0 {
                    self.state.cursor -= 1;
                    self.remove_char_at(self.state.cursor);
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
                            }
                        }
                    }
                }
            } else {
                self.state.backspace_time = None;
            }
        }

        // Delete key
        if is_key_pressed(KeyCode::Delete) {
            self.state.delete_time = Some(now);
            if !self.delete_selection() {
                if self.state.cursor < self.buffer.chars().count() {
                    self.remove_char_at(self.state.cursor);
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
                            }
                        }
                    }
                }
            } else {
                self.state.delete_time = None;
            }
        }

        // Enter key
        if is_key_pressed(KeyCode::Enter) {
            if self.multiline {
                self.delete_selection();
                let byte_pos = self.byte_at(self.state.cursor);
                self.buffer.insert(byte_pos, '\n');
                self.state.cursor += 1;
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
            }
        }

        if is_key_pressed(KeyCode::Escape) {
            self.cancel();
        }
    }

    pub fn render(&mut self, ui: &mut Ui, rect: Rect, c: Color, placeholder: &str) {
        self.rect = ui.rect_to_global(rect);
        let bx = rect.x;
        let by = rect.y;
        let bw = rect.w;
        let bh = rect.h;

        ui.fill_path(
            &Rect::new(bx, by, bw, bh).rounded(0.008),
            Color::new(0.35, 0.5, 1.0, c.a * 0.8),
        );
        ui.fill_path(
            &Rect::new(bx + 0.002, by + 0.002, bw - 0.004, bh - 0.004).rounded(0.006),
            Color::new(0.15, 0.15, 0.18, c.a),
        );

        let line_h = ui.text("0").size(0.42).measure().h;
        let text_x = bx + 0.02;
        let max_w = bw - 0.04;
        let max_h = bh - 0.04;
        let clip = Rect::new(bx + 0.002, by + 0.002, bw - 0.004, bh - 0.004);
        ui.scissor(clip, |ui| {
            if self.buffer.is_empty() {
                let text_y = by + bh / 2.0;
                ui.text(placeholder)
                    .pos(text_x, text_y)
                    .anchor(0.0, 0.5)
                    .no_baseline()
                    .size(0.42)
                    .color(Color::new(1.0, 1.0, 1.0, c.a * 0.3))
                    .draw();
                let cursor_x = text_x;
                let cursor_y = by + 0.01;
                ui.fill_rect(Rect::new(cursor_x, cursor_y, 0.003, bh - 0.02), Color::new(1.0, 1.0, 1.0, c.a * 0.9));
                self.update_ime(ui, (cursor_x, text_y - line_h * 0.5));
                self.state.cursor_positions.clear();
                self.state.cursor_positions.push(ui.to_global((cursor_x, text_y)));
            } else if self.multiline {
                let text_y = by + 0.02;
                let line_h_with_space = ui.text("0\n0").size(0.42).multiline().measure().h - ui.text("0").size(0.42).measure().h;
                let before = self.text_before();
                let line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
                let cursor_line_text = &before[line_start..];
                let line_num = before.chars().filter(|c| *c == '\n').count() as f32;
                let cursor_w = ui.text(cursor_line_text).size(0.42).multiline().measure().w;
                let cursor_y = line_num * line_h_with_space;
                let full_text = ui.text(&self.buffer).size(0.42).multiline().measure();
                let text_x_adj = if full_text.w > max_w {
                    let margin = max_w * 0.1;
                    let lo = (cursor_w - max_w + margin).max(0.0);
                    let hi = (cursor_w - margin).max(0.0).min(full_text.w - max_w);
                    if lo <= hi {
                        self.state.scroll_x = self.state.scroll_x.clamp(lo, hi);
                    } else {
                    self.state.scroll_x = hi;
                    }
                    text_x - self.state.scroll_x
                } else {
                    self.state.scroll_x = 0.0;
                    text_x
                };
                let text_y_adj = if full_text.h > max_h {
                    let margin = max_h * 0.1;
                    let lo = (cursor_y + line_h - max_h + margin).max(0.0);
                    let hi = (cursor_y - margin).max(0.0).min(full_text.h - max_h);
                    if lo <= hi {
                        self.state.scroll_y = self.state.scroll_y.clamp(lo, hi);
                    } else {
                        self.state.scroll_y = hi;
                    }
                    text_y - self.state.scroll_y
                } else {
                    self.state.scroll_y = 0.0;
                    text_y
                };
                let cursor_y_adj = text_y_adj + line_num * line_h_with_space;
                if let Some((sel_start, sel_end)) = self.selection_range() {
                    let mut char_offset = 0usize;
                    for (line_idx, line) in self.buffer.split('\n').enumerate() {
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
                            let x = text_x_adj + start_w;
                            let w = end_w - start_w;
                            if w > 0.0 {
                                ui.fill_rect(Rect::new(x, y, w, line_h + 0.01), Color::new(0.3, 0.5, 1.0, c.a * 0.3));
                            }
                        }

                        char_offset += line_len + 1;
                    }
                }
                ui.text(&self.buffer)
                    .pos(text_x_adj, text_y_adj)
                    .size(0.42)
                    .color(Color::new(1.0, 1.0, 1.0, c.a))
                    .multiline()
                    .draw();
                let cx = text_x_adj + cursor_w;
                ui.fill_rect(Rect::new(cx, cursor_y_adj, 0.003, line_h + 0.01), Color::new(1.0, 1.0, 1.0, c.a * 0.9));
                self.update_ime(ui, (cx, cursor_y_adj + 0.002));
                self.state.cursor_positions.clear();
                let chars_count = self.buffer.chars().count();
                let mut line_num_cur = 0usize;
                let mut line_start_byte = 0usize;
                for i in 0..=chars_count {
                    if i > 0 {
                        let prev_byte = self.byte_at(i - 1);
                        if self.buffer.as_bytes()[prev_byte] == b'\n' {
                            line_num_cur += 1;
                            line_start_byte = prev_byte + 1;
                        }
                    }
                    let byte_pos = self.byte_at(i);
                    let line_text = &self.buffer[line_start_byte..byte_pos];
                    let w = if line_text.is_empty() {
                        0.0
                    } else {
                        ui.text(line_text).size(0.42).multiline().measure().w
                    };
                    let x = text_x_adj + w;
                    let y = text_y_adj + line_num_cur as f32 * line_h_with_space;
                    self.state.cursor_positions.push(ui.to_global((x, y)));
                }
            } else {
                let text_y = by + bh / 2.0;
                let before = self.text_before();
                let cursor_w = ui.text(before).size(0.42).measure().w;
                let full_w = ui.text(&self.buffer).size(0.42).measure().w;
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
                    text_x
                };
                // Draw selection highlight
                if let Some((sel_start, sel_end)) = self.selection_range() {
                    let start_before = &self.buffer[..self.byte_at(sel_start)];
                    let end_before = &self.buffer[..self.byte_at(sel_end)];
                    let sel_start_w = ui.text(start_before).size(0.42).measure().w;
                    let sel_end_w = ui.text(end_before).size(0.42).measure().w;
                    let sel_x = text_x_adj + sel_start_w;
                    let sel_w = sel_end_w - sel_start_w;
                    ui.fill_rect(Rect::new(sel_x, by + 0.01, sel_w, bh - 0.02), Color::new(0.3, 0.5, 1.0, c.a * 0.3));
                }
                ui.text(&self.buffer)
                    .pos(text_x_adj, text_y)
                    .anchor(0.0, 0.5)
                    .no_baseline()
                    .size(0.42)
                    .color(Color::new(1.0, 1.0, 1.0, c.a))
                    .draw();
                let cx = text_x_adj + cursor_w;
                ui.fill_rect(Rect::new(cx, by + 0.01, 0.003, bh - 0.02), Color::new(1.0, 1.0, 1.0, c.a * 0.9));
                self.update_ime(ui, (cx, text_y - line_h * 0.5));
                self.state.cursor_positions.clear();
                let chars_count = self.buffer.chars().count();
                for i in 0..=chars_count {
                    let before_i = &self.buffer[..self.byte_at(i)];
                    let w = ui.text(before_i).size(0.42).measure().w;
                    let x = text_x_adj + w;
                    self.state.cursor_positions.push(ui.to_global((x, text_y)));
                }
            }
        });
    }
}
