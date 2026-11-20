//! Unified single-line text input, password field, and search input controls (`RUSTYOS-130`).

use std::time::Duration;

use crate::clipboard::ClipboardSink;
use crate::draw::UiPainter;
use crate::geometry::Rect;
use crate::motion::CaretBlink;
use crate::text::{
    UiFontEngine, clamp_char_boundary, next_word_boundary, prev_word_boundary, word_bounds_at,
};
use crate::theme::ThemePalette;

/// Keyboard modifier flags supplied to text input handlers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InputModifiers {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

/// Canonical editing keys processed by [`TextField`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextInputKey {
    Left,
    Right,
    Home,
    End,
    Backspace,
    Delete,
    Enter,
    Escape,
    Char(char),
}

/// Outcome of a key or text event inside a [`TextField`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextInputEvent {
    Ignored,
    Changed,
    CaretMoved,
    Submitted,
    Cancelled,
}

/// Hit region inside a [`TextField`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextFieldHit {
    Input,
    ClearButton,
}

/// Editable UTF-8 single-line text buffer supporting selection, word navigation, and clipboard.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextBuffer {
    text: String,
    cursor: usize,
    anchor: Option<usize>,
    dragging: bool,
}

impl TextBuffer {
    #[must_use]
    pub fn new(initial: impl Into<String>) -> Self {
        let text = initial.into();
        let cursor = text.len();
        Self {
            text,
            cursor,
            anchor: None,
            dragging: false,
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn set_text(&mut self, value: impl Into<String>) {
        self.text = value.into();
        self.cursor = self.text.len();
        self.anchor = None;
    }

    #[must_use]
    pub const fn cursor(&self) -> usize {
        self.cursor
    }

    /// Returns ordered selection `(start_byte, end_byte)` when a non-empty range is selected.
    #[must_use]
    pub fn selection_range(&self) -> Option<(usize, usize)> {
        let anchor = self.anchor?;
        match anchor.cmp(&self.cursor) {
            std::cmp::Ordering::Equal => None,
            std::cmp::Ordering::Less => Some((anchor, self.cursor)),
            std::cmp::Ordering::Greater => Some((self.cursor, anchor)),
        }
    }

    #[must_use]
    pub fn selected_text(&self) -> Option<&str> {
        let (s, e) = self.selection_range()?;
        self.text.get(s..e)
    }

    pub fn select_all(&mut self) {
        if self.text.is_empty() {
            self.anchor = None;
            self.cursor = 0;
        } else {
            self.anchor = Some(0);
            self.cursor = self.text.len();
        }
    }

    pub fn clear_selection(&mut self) {
        self.anchor = None;
    }

    /// Replaces active selection (or inserts at cursor) and advances cursor cleanly.
    pub fn insert_str(&mut self, s: &str) {
        self.delete_selection_internal();
        let clean: String = s.chars().filter(|ch| *ch != '\r' && *ch != '\n').collect();
        if clean.is_empty() {
            return;
        }
        let pos = clamp_char_boundary(&self.text, self.cursor);
        self.text.insert_str(pos, &clean);
        self.cursor = pos + clean.len();
        self.anchor = None;
    }

    /// Deletes active selection or one char/word before cursor; returns `true` if modified.
    pub fn backspace(&mut self, by_word: bool) -> bool {
        if self.delete_selection_internal() {
            return true;
        }
        if self.cursor == 0 {
            return false;
        }
        let start = if by_word {
            prev_word_boundary(&self.text, self.cursor)
        } else {
            self.text[..self.cursor]
                .char_indices()
                .next_back()
                .map_or(0, |(idx, _)| idx)
        };
        self.text.drain(start..self.cursor);
        self.cursor = start;
        true
    }

    /// Deletes active selection or one char/word after cursor; returns `true` if modified.
    pub fn delete(&mut self, by_word: bool) -> bool {
        if self.delete_selection_internal() {
            return true;
        }
        if self.cursor >= self.text.len() {
            return false;
        }
        let end = if by_word {
            next_word_boundary(&self.text, self.cursor)
        } else {
            self.text[self.cursor..]
                .chars()
                .next()
                .map_or(self.text.len(), |ch| self.cursor + ch.len_utf8())
        };
        self.text.drain(self.cursor..end);
        true
    }

    /// Moves caret horizontally by char or word, extending selection when `select == true`.
    pub fn move_horizontal(&mut self, right: bool, by_word: bool, select: bool) {
        if !select && let Some((s, e)) = self.selection_range() {
            self.cursor = if right { e } else { s };
            self.anchor = None;
            return;
        }
        if select && self.anchor.is_none() {
            self.anchor = Some(self.cursor);
        } else if !select {
            self.anchor = None;
        }

        self.cursor = if right {
            if by_word {
                next_word_boundary(&self.text, self.cursor)
            } else {
                self.text[self.cursor..]
                    .chars()
                    .next()
                    .map_or(self.text.len(), |ch| self.cursor + ch.len_utf8())
            }
        } else if by_word {
            prev_word_boundary(&self.text, self.cursor)
        } else {
            self.text[..self.cursor]
                .char_indices()
                .next_back()
                .map_or(0, |(idx, _)| idx)
        };
        if !select {
            self.anchor = None;
        }
    }

    pub fn move_edge(&mut self, end: bool, select: bool) {
        if select && self.anchor.is_none() {
            self.anchor = Some(self.cursor);
        } else if !select {
            self.anchor = None;
        }
        self.cursor = if end { self.text.len() } else { 0 };
    }

    pub fn copy(&self, clipboard: &mut dyn ClipboardSink) -> bool {
        if let Some(sel) = self.selected_text() {
            clipboard.set_text(sel);
            true
        } else {
            false
        }
    }

    pub fn cut(&mut self, clipboard: &mut dyn ClipboardSink) -> bool {
        if self.copy(clipboard) {
            self.delete_selection_internal()
        } else {
            false
        }
    }

    pub fn paste(&mut self, clipboard: &mut dyn ClipboardSink) -> bool {
        if let Some(text) = clipboard.get_text() {
            let before = self.text.clone();
            self.insert_str(&text);
            self.text != before
        } else {
            false
        }
    }

    fn delete_selection_internal(&mut self) -> bool {
        if let Some((s, e)) = self.selection_range() {
            self.text.drain(s..e);
            self.cursor = s;
            self.anchor = None;
            true
        } else {
            self.anchor = None;
            false
        }
    }
}

/// Complete single-line editable text, password, or search field control (`RUSTYOS-130`).
#[derive(Debug, Clone, PartialEq)]
pub struct TextField {
    pub rect: Rect,
    pub placeholder: String,
    pub buffer: TextBuffer,
    pub corner_radius: f32,
    pub font_size: f32,
    pub padding_x: i32,
    pub masked: bool,
    pub search_style: bool,
    pub clearable: bool,
    pub scroll_x: f32,
    pub caret_blink: CaretBlink,
}

impl TextField {
    #[must_use]
    pub fn new(rect: Rect, placeholder: impl Into<String>) -> Self {
        Self {
            rect,
            placeholder: placeholder.into(),
            buffer: TextBuffer::default(),
            corner_radius: 8.0,
            font_size: 13.0,
            padding_x: 10,
            masked: false,
            search_style: false,
            clearable: false,
            scroll_x: 0.0,
            caret_blink: CaretBlink::new(),
        }
    }

    #[must_use]
    pub fn password(rect: Rect, placeholder: impl Into<String>) -> Self {
        let mut field = Self::new(rect, placeholder);
        field.masked = true;
        field
    }

    #[must_use]
    pub fn search(rect: Rect, placeholder: impl Into<String>) -> Self {
        let mut field = Self::new(rect, placeholder);
        field.corner_radius = (rect.height as f32 * 0.5).max(10.0);
        field.font_size = 12.5;
        field.padding_x = 26;
        field.search_style = true;
        field.clearable = true;
        field
    }

    #[must_use]
    pub fn clear_btn_rect(&self) -> Option<Rect> {
        if !self.clearable || self.buffer.as_str().is_empty() {
            return None;
        }
        let size = 14_i32;
        let rx = self.rect.right() - 8 - size;
        let ry = self.rect.y + (self.rect.height as i32 - size) / 2;
        Some(Rect::new(rx, ry, size as u32, size as u32))
    }

    #[must_use]
    pub fn hit_test(&self, px: f64, py: f64) -> Option<TextFieldHit> {
        if !self.rect.contains_f64(px, py) {
            return None;
        }
        if let Some(clear_rect) = self.clear_btn_rect()
            && clear_rect.contains_f64(px, py)
        {
            return Some(TextFieldHit::ClearButton);
        }
        Some(TextFieldHit::Input)
    }

    fn text_inner_bounds(&self) -> (i32, i32) {
        let left = self.rect.x + self.padding_x;
        let right_pad = if self.clear_btn_rect().is_some() {
            26
        } else {
            10
        };
        let right = (self.rect.right() - right_pad).max(left + 1);
        (left, right)
    }

    fn display_string_and_byte(&self, byte_idx: usize) -> (String, usize) {
        if !self.masked {
            return (self.buffer.as_str().to_owned(), byte_idx);
        }
        let raw = self.buffer.as_str();
        let clamped = clamp_char_boundary(raw, byte_idx);
        let char_pos = raw[..clamped].chars().count();
        let total_chars = raw.chars().count();
        let masked_str: String = std::iter::repeat_n('•', total_chars).collect();
        let mapped_byte = masked_str
            .char_indices()
            .nth(char_pos)
            .map_or(masked_str.len(), |(idx, _)| idx);
        (masked_str, mapped_byte)
    }

    fn sync_scroll_to_caret(&mut self, fonts: &mut UiFontEngine) {
        let (left, right) = self.text_inner_bounds();
        let view_w = (right - left) as f32;
        let (disp, mapped_cursor) = self.display_string_and_byte(self.buffer.cursor());
        let caret_x = fonts.x_for_byte(&disp, mapped_cursor, self.font_size, false);
        let total_w = fonts.measure_width(&disp, self.font_size, false);
        if total_w <= view_w {
            self.scroll_x = 0.0;
            return;
        }
        if caret_x - self.scroll_x > view_w - 2.0 {
            self.scroll_x = (caret_x - view_w + 4.0).max(0.0);
        } else if caret_x - self.scroll_x < 0.0 {
            self.scroll_x = caret_x.max(0.0);
        }
    }

    /// Handles pointer press (`click_count == 1` places caret, `2` selects word, `3` selects all).
    pub fn on_pointer_down(
        &mut self,
        px: f64,
        py: f64,
        click_count: u8,
        now: Duration,
        fonts: &mut UiFontEngine,
    ) -> Option<TextFieldHit> {
        let hit = self.hit_test(px, py)?;
        self.caret_blink.reset(now);
        if hit == TextFieldHit::ClearButton {
            self.buffer.set_text("");
            self.scroll_x = 0.0;
            return Some(TextFieldHit::ClearButton);
        }
        let (left, _) = self.text_inner_bounds();
        let local_x = (px as f32 - left as f32) + self.scroll_x;
        let (disp, _) = self.display_string_and_byte(0);
        let disp_byte = fonts.byte_for_x(&disp, local_x, self.font_size, false);
        let target_byte = if self.masked {
            let char_idx = disp[..disp_byte].chars().count();
            self.buffer
                .as_str()
                .char_indices()
                .nth(char_idx)
                .map_or(self.buffer.as_str().len(), |(i, _)| i)
        } else {
            disp_byte
        };

        if click_count >= 3 {
            self.buffer.select_all();
        } else if click_count == 2 && !self.masked {
            let (ws, we) = word_bounds_at(self.buffer.as_str(), target_byte);
            self.buffer.anchor = Some(ws);
            self.buffer.cursor = we;
        } else {
            self.buffer.anchor = Some(target_byte);
            self.buffer.cursor = target_byte;
            self.buffer.dragging = true;
        }
        self.sync_scroll_to_caret(fonts);
        Some(TextFieldHit::Input)
    }

    pub fn on_pointer_move(&mut self, px: f64, fonts: &mut UiFontEngine) -> bool {
        if !self.buffer.dragging {
            return false;
        }
        let (left, _) = self.text_inner_bounds();
        let local_x = (px as f32 - left as f32) + self.scroll_x;
        let (disp, _) = self.display_string_and_byte(0);
        let disp_byte = fonts.byte_for_x(&disp, local_x, self.font_size, false);
        let target_byte = if self.masked {
            let char_idx = disp[..disp_byte].chars().count();
            self.buffer
                .as_str()
                .char_indices()
                .nth(char_idx)
                .map_or(self.buffer.as_str().len(), |(i, _)| i)
        } else {
            disp_byte
        };
        if target_byte != self.buffer.cursor {
            self.buffer.cursor = target_byte;
            self.sync_scroll_to_caret(fonts);
            return true;
        }
        false
    }

    pub const fn on_pointer_up(&mut self) {
        self.buffer.dragging = false;
    }

    /// Processes a keyboard command (`Ctrl+A/C/X/V`, cursor arrows, `Backspace`/`Delete`, text char).
    pub fn handle_key(
        &mut self,
        key: TextInputKey,
        mods: InputModifiers,
        now: Duration,
        fonts: &mut UiFontEngine,
        clipboard: &mut dyn ClipboardSink,
    ) -> TextInputEvent {
        self.caret_blink.reset(now);
        let event = match key {
            TextInputKey::Escape => return TextInputEvent::Cancelled,
            TextInputKey::Enter => return TextInputEvent::Submitted,
            TextInputKey::Char(ch) if mods.ctrl => match ch.to_ascii_lowercase() {
                'a' => {
                    self.buffer.select_all();
                    TextInputEvent::CaretMoved
                }
                'c' => {
                    if !self.masked && self.buffer.copy(clipboard) {
                        TextInputEvent::CaretMoved
                    } else {
                        TextInputEvent::Ignored
                    }
                }
                'x' => {
                    if !self.masked && self.buffer.cut(clipboard) {
                        TextInputEvent::Changed
                    } else {
                        TextInputEvent::Ignored
                    }
                }
                'v' => {
                    if self.buffer.paste(clipboard) {
                        TextInputEvent::Changed
                    } else {
                        TextInputEvent::Ignored
                    }
                }
                _ => TextInputEvent::Ignored,
            },
            TextInputKey::Char(ch) if !ch.is_control() => {
                let mut buf = [0_u8; 4];
                self.buffer.insert_str(ch.encode_utf8(&mut buf));
                TextInputEvent::Changed
            }
            TextInputKey::Backspace => {
                if self.buffer.backspace(mods.ctrl) {
                    TextInputEvent::Changed
                } else {
                    TextInputEvent::Ignored
                }
            }
            TextInputKey::Delete => {
                if self.buffer.delete(mods.ctrl) {
                    TextInputEvent::Changed
                } else {
                    TextInputEvent::Ignored
                }
            }
            TextInputKey::Left => {
                self.buffer.move_horizontal(false, mods.ctrl, mods.shift);
                TextInputEvent::CaretMoved
            }
            TextInputKey::Right => {
                self.buffer.move_horizontal(true, mods.ctrl, mods.shift);
                TextInputEvent::CaretMoved
            }
            TextInputKey::Home => {
                self.buffer.move_edge(false, mods.shift);
                TextInputEvent::CaretMoved
            }
            TextInputKey::End => {
                self.buffer.move_edge(true, mods.shift);
                TextInputEvent::CaretMoved
            }
            TextInputKey::Char(_) => TextInputEvent::Ignored,
        };
        if event != TextInputEvent::Ignored {
            self.sync_scroll_to_caret(fonts);
        }
        event
    }

    /// Records rendering commands for this text field into `painter`.
    #[allow(clippy::too_many_arguments)]
    pub fn paint(
        &self,
        painter: &mut UiPainter<'_>,
        fonts: &mut UiFontEngine,
        pal: &ThemePalette,
        focused: bool,
        focus_alpha: f32,
        now: Duration,
    ) {
        self.paint_box(painter, pal, focused, focus_alpha);
        self.paint_contents(painter, fonts, pal, focused, now);
        self.paint_clear_button(painter, pal);
    }

    fn paint_box(
        &self,
        painter: &mut UiPainter<'_>,
        pal: &ThemePalette,
        focused: bool,
        focus_alpha: f32,
    ) {
        let (bg, border) = if self.search_style {
            if pal.is_dark {
                (
                    if focused {
                        [0x2A, 0x2A, 0x2E, 255]
                    } else {
                        [0x22, 0x22, 0x25, 255]
                    },
                    [255, 255, 255, if focused { 46 } else { 22 }],
                )
            } else {
                (
                    if focused {
                        [0xE6, 0xE8, 0xEC, 255]
                    } else {
                        [0xE9, 0xEB, 0xEF, 255]
                    },
                    [0, 0, 0, if focused { 28 } else { 14 }],
                )
            }
        } else if pal.is_dark {
            ([0x24, 0x24, 0x27, 255], [255, 255, 255, 28])
        } else {
            ([0xFF, 0xFF, 0xFF, 255], [0, 0, 0, 26])
        };

        painter.rounded_rect_styled(self.rect, self.corner_radius, bg, 1.0, border, false);
        if focused {
            let alpha = if focus_alpha > 0.01 { focus_alpha } else { 1.0 };
            painter.draw_focus_ring(self.rect, self.corner_radius, pal.accent, alpha);
        }

        if self.search_style {
            let lcx = self.rect.x as f32 + 13.0;
            let lcy = self.rect.y as f32 + self.rect.height as f32 * 0.5 - 1.0;
            painter.stroke_circle(lcx, lcy, 4.0, 1.3, pal.text_muted);
            painter.draw_line(
                lcx + 2.6,
                lcy + 2.6,
                lcx + 5.6,
                lcy + 5.6,
                1.4,
                pal.text_muted,
            );
        }
    }

    fn paint_contents(
        &self,
        painter: &mut UiPainter<'_>,
        fonts: &mut UiFontEngine,
        pal: &ThemePalette,
        focused: bool,
        now: Duration,
    ) {
        let (left, right) = self.text_inner_bounds();
        let clip_rect = Rect::new(
            left,
            self.rect.y + 2,
            (right - left).max(1) as u32,
            self.rect.height.saturating_sub(4).max(1),
        );
        painter.push_clip(clip_rect);

        let text_x = left as f32 - self.scroll_x;
        let baseline =
            self.rect.y as f32 + f32::midpoint(self.rect.height as f32, self.font_size * 0.72);
        let (disp, mapped_cursor) = self.display_string_and_byte(self.buffer.cursor());

        if focused && let Some((sel_s, sel_e)) = self.buffer.selection_range() {
            let (_, ms) = self.display_string_and_byte(sel_s);
            let (_, me) = self.display_string_and_byte(sel_e);
            let sx0 = text_x + fonts.x_for_byte(&disp, ms, self.font_size, false);
            let sx1 = text_x + fonts.x_for_byte(&disp, me, self.font_size, false);
            let sel_w = (sx1 - sx0).ceil().max(2.0) as u32;
            let sel_h = (self.font_size * 1.25).round().max(12.0) as u32;
            let sel_y = self.rect.y + (self.rect.height as i32 - sel_h as i32) / 2;
            let [ar, ag, ab, _] = pal.accent;
            painter.fill_rounded_rect(
                Rect::new(sx0.floor() as i32, sel_y, sel_w, sel_h),
                3.0,
                [ar, ag, ab, 72],
            );
        }

        if disp.is_empty() {
            painter.draw_text(
                self.placeholder.clone(),
                left as f32,
                baseline,
                self.font_size,
                false,
                pal.text_muted,
            );
        } else {
            painter.draw_text(
                disp.clone(),
                text_x,
                baseline,
                self.font_size,
                false,
                pal.text_primary,
            );
        }

        if focused && self.caret_blink.is_visible(now) {
            let cx = text_x + fonts.x_for_byte(&disp, mapped_cursor, self.font_size, false);
            let ch = (self.font_size * 1.28).round().max(13.0) as u32;
            let cy = self.rect.y + (self.rect.height as i32 - ch as i32) / 2;
            painter.fill_rect(Rect::new(cx.round() as i32, cy, 2, ch), pal.accent);
        }
        painter.pop_clip();
    }

    fn paint_clear_button(&self, painter: &mut UiPainter<'_>, pal: &ThemePalette) {
        if let Some(cr) = self.clear_btn_rect() {
            let (ccx, ccy) = cr.center_f32();
            painter.fill_circle(ccx, ccy, 6.0, pal.text_muted);
            let ink = if pal.is_dark {
                [0x22, 0x22, 0x25, 255]
            } else {
                [255, 255, 255, 255]
            };
            painter.draw_line(ccx - 2.3, ccy - 2.3, ccx + 2.3, ccy + 2.3, 1.2, ink);
            painter.draw_line(ccx + 2.3, ccy - 2.3, ccx - 2.3, ccy + 2.3, 1.2, ink);
        }
    }
}
