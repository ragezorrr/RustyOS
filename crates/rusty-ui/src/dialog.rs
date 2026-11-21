//! Unified modal dialog sheet (`DialogSheet`) for alerts, confirmations, and text prompts (`RUSTYOS-130`).

use std::time::Duration;

use crate::draw::UiPainter;
use crate::geometry::{Rect, WINDOW_RADIUS};
use crate::shadow::ShadowElevation;
use crate::text::UiFontEngine;
use crate::text_input::TextField;
use crate::theme::ThemePalette;
use crate::widgets::{ButtonStyle, WidgetState};

/// Outcome of dialog sheet interaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogAction {
    Cancel,
    Confirm,
}

/// Modal dialog configuration matching RustyOS sheets (`r = 14`, scrim + `ShadowElevation::Popup`).
#[derive(Debug, Clone, PartialEq)]
pub struct DialogSheet {
    pub title: String,
    pub message: String,
    pub primary_label: String,
    pub cancel_label: Option<String>,
    pub destructive: bool,
    pub width: u32,
    pub input: Option<TextField>,
    pub hovered_btn: Option<DialogAction>,
    pub pressed_btn: Option<DialogAction>,
}

/// Computed screen geometry of a [`DialogSheet`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogLayout {
    pub card_rect: Rect,
    pub input_rect: Option<Rect>,
    pub cancel_rect: Option<Rect>,
    pub primary_rect: Rect,
}

impl DialogSheet {
    #[must_use]
    pub fn confirm(
        title: impl Into<String>,
        message: impl Into<String>,
        primary_label: impl Into<String>,
    ) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            primary_label: primary_label.into(),
            cancel_label: Some(String::from("Cancel")),
            destructive: false,
            width: 360,
            input: None,
            hovered_btn: None,
            pressed_btn: None,
        }
    }

    #[must_use]
    pub fn prompt(
        title: impl Into<String>,
        message: impl Into<String>,
        primary_label: impl Into<String>,
        placeholder: impl Into<String>,
        initial_text: impl Into<String>,
    ) -> Self {
        let mut sheet = Self::confirm(title, message, primary_label);
        let mut tf = TextField::new(Rect::new(0, 0, 312, 32), placeholder);
        tf.buffer.set_text(initial_text);
        tf.buffer.select_all();
        sheet.input = Some(tf);
        sheet
    }

    #[must_use]
    pub const fn with_destructive(mut self, destructive: bool) -> Self {
        self.destructive = destructive;
        self
    }

    #[must_use]
    pub fn layout(&self, win_w: u32, win_h: u32) -> DialogLayout {
        let w = self.width.min(win_w.saturating_sub(32)).max(240);
        let has_msg = !self.message.is_empty();
        let has_input = self.input.is_some();
        let mut h = 20_u32 + 22;
        if has_msg {
            h += 24;
        }
        if has_input {
            h += 44;
        }
        h += 54;
        let x = (win_w as i32 - w as i32) / 2;
        let y = (win_h as i32 - h as i32) / 2;
        let card_rect = Rect::new(x, y, w, h);

        let input_y = y + 48 + if has_msg { 22 } else { 0 };
        let input_rect = has_input.then(|| Rect::new(x + 20, input_y, w - 40, 32));
        let btn_y = card_rect.bottom() - 20 - 30;
        let primary_w = 96_u32;
        let primary_rect = Rect::new(
            card_rect.right() - 20 - primary_w as i32,
            btn_y,
            primary_w,
            30,
        );
        let cancel_rect = self
            .cancel_label
            .as_ref()
            .map(|_| Rect::new(primary_rect.x - 10 - 84, btn_y, 84, 30));
        DialogLayout {
            card_rect,
            input_rect,
            cancel_rect,
            primary_rect,
        }
    }

    /// Ensures internal `TextField` rectangle matches current dialog layout before pointer/paint calls.
    pub fn sync_input_rect(&mut self, win_w: u32, win_h: u32) -> DialogLayout {
        let layout = self.layout(win_w, win_h);
        if let (Some(tf), Some(ir)) = (&mut self.input, layout.input_rect) {
            tf.rect = ir;
        }
        layout
    }

    #[must_use]
    pub fn hit_test(&self, px: f64, py: f64, win_w: u32, win_h: u32) -> Option<DialogAction> {
        let layout = self.layout(win_w, win_h);
        if layout.primary_rect.contains_f64(px, py) {
            return Some(DialogAction::Confirm);
        }
        if layout.cancel_rect.is_some_and(|r| r.contains_f64(px, py)) {
            return Some(DialogAction::Cancel);
        }
        None
    }

    pub fn on_pointer_move(&mut self, px: f64, py: f64, win_w: u32, win_h: u32) -> bool {
        let next = self.hit_test(px, py, win_w, win_h);
        let changed = self.hovered_btn != next;
        self.hovered_btn = next;
        changed
    }

    pub fn on_pointer_down(
        &mut self,
        px: f64,
        py: f64,
        win_w: u32,
        win_h: u32,
    ) -> Option<DialogAction> {
        self.pressed_btn = self.hit_test(px, py, win_w, win_h);
        self.pressed_btn
    }

    pub fn on_pointer_up(
        &mut self,
        px: f64,
        py: f64,
        win_w: u32,
        win_h: u32,
    ) -> Option<DialogAction> {
        let prev = self.pressed_btn.take()?;
        (self.hit_test(px, py, win_w, win_h) == Some(prev)).then_some(prev)
    }

    pub fn paint(
        &self,
        painter: &mut UiPainter<'_>,
        fonts: &mut UiFontEngine,
        win_w: u32,
        win_h: u32,
        pal: &ThemePalette,
        now: Duration,
    ) {
        painter.fill_rect(Rect::new(0, 0, win_w, win_h), [0, 0, 0, 88]);
        let layout = self.layout(win_w, win_h);
        painter.draw_shadow(
            layout.card_rect,
            WINDOW_RADIUS as i32,
            ShadowElevation::Popup,
            pal.is_dark,
        );
        let border = if pal.is_dark {
            [255, 255, 255, 28]
        } else {
            [0, 0, 0, 22]
        };
        painter.rounded_rect_styled(
            layout.card_rect,
            WINDOW_RADIUS as f32,
            pal.card_bg,
            1.0,
            border,
            true,
        );

        let tx = layout.card_rect.x as f32 + 20.0;
        let max_tw = (layout.card_rect.width as f32 - 40.0).max(20.0);
        let title_fit = fonts.ellipsize(&self.title, max_tw, 15.0, true);
        painter.draw_text(
            title_fit,
            tx,
            layout.card_rect.y as f32 + 34.0,
            15.0,
            true,
            pal.text_primary,
        );

        if !self.message.is_empty() {
            let msg_fit = fonts.ellipsize(&self.message, max_tw, 12.5, false);
            painter.draw_text(
                msg_fit,
                tx,
                layout.card_rect.y as f32 + 56.0,
                12.5,
                false,
                pal.text_secondary,
            );
        }

        if let (Some(tf), Some(ir)) = (&self.input, layout.input_rect) {
            let mut synced = tf.clone();
            synced.rect = ir;
            synced.paint(painter, fonts, pal, true, 1.0, now);
        }

        if let (Some(cancel_r), Some(label)) = (layout.cancel_rect, &self.cancel_label) {
            let st = button_state(
                self.hovered_btn == Some(DialogAction::Cancel),
                self.pressed_btn == Some(DialogAction::Cancel),
            );
            paint_button_rect(
                painter,
                fonts,
                cancel_r,
                label,
                ButtonStyle::Secondary,
                st,
                pal,
            );
        }

        let p_style = if self.destructive {
            ButtonStyle::Destructive
        } else {
            ButtonStyle::Primary
        };
        let p_st = button_state(
            self.hovered_btn == Some(DialogAction::Confirm),
            self.pressed_btn == Some(DialogAction::Confirm),
        );
        paint_button_rect(
            painter,
            fonts,
            layout.primary_rect,
            &self.primary_label,
            p_style,
            p_st,
            pal,
        );
    }
}

const fn button_state(hovered: bool, pressed: bool) -> WidgetState {
    if pressed {
        WidgetState::Pressed
    } else if hovered {
        WidgetState::Hover
    } else {
        WidgetState::Normal
    }
}

fn paint_button_rect(
    painter: &mut UiPainter<'_>,
    fonts: &mut UiFontEngine,
    rect: Rect,
    label: &str,
    style: ButtonStyle,
    state: WidgetState,
    pal: &ThemePalette,
) {
    let (bg, border, text_col) = match style {
        ButtonStyle::Primary => (
            match state {
                WidgetState::Pressed => [
                    ((pal.accent[0] as u16 * 8) / 10) as u8,
                    ((pal.accent[1] as u16 * 8) / 10) as u8,
                    ((pal.accent[2] as u16 * 8) / 10) as u8,
                    255,
                ],
                WidgetState::Hover => [
                    ((pal.accent[0] as u16 * 9) / 10) as u8,
                    ((pal.accent[1] as u16 * 9) / 10) as u8,
                    ((pal.accent[2] as u16 * 9) / 10) as u8,
                    255,
                ],
                _ => pal.accent,
            },
            None,
            [255, 255, 255, 255],
        ),
        ButtonStyle::Destructive => (
            match state {
                WidgetState::Pressed => [0xCC, 0x2F, 0x26, 255],
                WidgetState::Hover => [0xE6, 0x35, 0x2B, 255],
                _ => [0xFF, 0x3B, 0x30, 255],
            },
            None,
            [255, 255, 255, 255],
        ),
        ButtonStyle::Secondary => {
            let bg = if pal.is_dark {
                match state {
                    WidgetState::Pressed => [0x38, 0x38, 0x3C, 255],
                    WidgetState::Hover => [0x46, 0x46, 0x4A, 255],
                    _ => [0x3A, 0x3A, 0x3E, 255],
                }
            } else {
                match state {
                    WidgetState::Pressed => [0xD8, 0xD9, 0xDD, 255],
                    WidgetState::Hover => [0xE4, 0xE5, 0xE9, 255],
                    _ => [0xEE, 0xEF, 0xF2, 255],
                }
            };
            (bg, Some(pal.divider), pal.text_primary)
        }
    };
    painter.rounded_rect_styled(
        rect,
        8.0,
        bg,
        if border.is_some() { 1.0 } else { 0.0 },
        border.unwrap_or([0, 0, 0, 0]),
        false,
    );
    painter.draw_text_centered(
        fonts,
        label.to_owned(),
        rect,
        13.0,
        style != ButtonStyle::Secondary,
        text_col,
    );
}
