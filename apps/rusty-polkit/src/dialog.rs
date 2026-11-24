//! Liquid Glass styled modal `PolicyKit` authentication dialog state and rendering.

use std::time::Duration;

use rusty_services::{PolkitPromptRequest, SecretText};
use rusty_ui::{
    ButtonStyle, FocusScope, InputModifiers, PushButton, Rect, TextField, TextInputEvent,
    TextInputKey, ThemePalette, UiFontEngine, UiPainter, WidgetId,
};

pub(crate) fn focus_id_password() -> WidgetId {
    WidgetId::from_tag("polkit_password")
}

pub(crate) fn focus_id_cancel() -> WidgetId {
    WidgetId::from_tag("polkit_cancel")
}

pub(crate) fn focus_id_submit() -> WidgetId {
    WidgetId::from_tag("polkit_submit")
}

/// Canonical action triggered by user interaction in the Polkit dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DialogAction {
    /// User submitted passphrase for verification.
    Submit(SecretText),
    /// User cancelled or dismissed the dialog.
    Cancel,
}

/// Active UI state of the Polkit authentication dialog.
#[derive(Debug, Clone)]
pub(crate) struct PolkitDialogState {
    pub(crate) cookie: String,
    #[allow(dead_code)]
    pub(crate) action_id: String,
    pub(crate) message: String,
    pub(crate) username: String,
    #[allow(dead_code)]
    pub(crate) uid: u32,
    pub(crate) pass_field: TextField,
    pub(crate) status_msg: Option<String>,
    pub(crate) is_error: bool,
    pub(crate) checking: bool,
    pub(crate) tries: u32,
    pub(crate) dirty: bool,
    pub(crate) cancel_rect: Rect,
    pub(crate) submit_rect: Rect,
    pub(crate) focus: FocusScope,
}

impl PolkitDialogState {
    /// Creates dialog state populated from an incoming [`PolkitPromptRequest`].
    #[must_use]
    pub(crate) fn new(req: PolkitPromptRequest) -> Self {
        let field_rect = Rect::new(36, 136, 348, 32);
        let pass_field = TextField::password(field_rect, "Пароль администратора");
        let mut focus = FocusScope::new();
        focus.begin_frame();
        focus.register(focus_id_password());
        focus.register(focus_id_cancel());
        focus.register(focus_id_submit());
        focus.set_focus(focus_id_password(), Duration::ZERO, true);

        Self {
            cookie: req.cookie,
            action_id: req.action_id,
            message: req.message,
            username: req.username,
            uid: req.uid,
            pass_field,
            status_msg: None,
            is_error: false,
            checking: false,
            tries: 0,
            dirty: true,
            cancel_rect: Rect::new(208, 196, 90, 30),
            submit_rect: Rect::new(308, 196, 96, 30),
            focus,
        }
    }

    /// Cycles keyboard focus across Password -> Cancel -> Confirm (`Tab` / `Shift+Tab`).
    pub(crate) fn cycle_focus(&mut self, backwards: bool) {
        let _ = self.focus.cycle_focus(backwards, Duration::ZERO, true);
        self.dirty = true;
    }

    /// Activates the currently focused control (`Enter` or `Space` on buttons).
    pub(crate) fn activate_focused(&self) -> Option<DialogAction> {
        if self.checking {
            return None;
        }
        if self.focus.is_focused(focus_id_cancel()) {
            return Some(DialogAction::Cancel);
        }
        let pass = self.pass_field.buffer.as_str().trim();
        if pass.is_empty() {
            None
        } else {
            Some(DialogAction::Submit(SecretText::from_slice(pass)))
        }
    }

    /// Creates dummy dialog state for `--demo` preview or automated tests.
    #[must_use]
    pub(crate) fn demo() -> Self {
        Self::new(PolkitPromptRequest {
            cookie: "demo-cookie-12345".to_owned(),
            action_id: "org.freedesktop.policykit.exec".to_owned(),
            message:
                "Приложение запрашивает права администратора для изменения системных параметров."
                    .to_owned(),
            icon_name: "dialog-password".to_owned(),
            username: std::env::var("USER").unwrap_or_else(|_| "admin".to_owned()),
            uid: 1000,
            caller_pid: Some(1234),
            prompt_label: "Password:".to_owned(),
            echo_on: false,
        })
    }

    /// Recalculates layout rects based on logical canvas bounds `(w, h)`.
    pub(crate) fn update_layout(&mut self, w: u32, _h: u32) {
        let width = w.max(360);
        let field_w = width.saturating_sub(72);
        self.pass_field.rect = Rect::new(36, 136, field_w, 32);
        let btn_y = 194;
        let submit_w = 100_u32;
        let cancel_w = 90_u32;
        let submit_x = i32::try_from(width.saturating_sub(36 + submit_w)).unwrap_or(284);
        let cancel_x = submit_x - i32::try_from(cancel_w + 10).unwrap_or(100);
        self.submit_rect = Rect::new(submit_x, btn_y, submit_w, 30);
        self.cancel_rect = Rect::new(cancel_x, btn_y, cancel_w, 30);
    }

    /// Updates error status message and increments failed attempt counter.
    pub(crate) fn set_error(&mut self, error_text: impl Into<String>) {
        self.status_msg = Some(error_text.into());
        self.is_error = true;
        self.checking = false;
        self.tries = self.tries.saturating_add(1);
        self.pass_field.buffer.set_text("");
        self.dirty = true;
    }

    /// Sets checking in-progress indicator.
    pub(crate) fn set_checking(&mut self, checking: bool) {
        self.checking = checking;
        if checking {
            self.status_msg = Some("Проверка пароля...".to_owned());
            self.is_error = false;
        } else if !self.is_error {
            self.status_msg = None;
        }
        self.dirty = true;
    }

    /// Handles keyboard event and returns an action when submitted or cancelled.
    pub(crate) fn on_key(
        &mut self,
        key: TextInputKey,
        modifiers: InputModifiers,
        fonts: &mut UiFontEngine,
        clipboard: &mut dyn rusty_ui::ClipboardSink,
    ) -> Option<DialogAction> {
        if self.checking {
            return None;
        }

        match key {
            TextInputKey::Escape => {
                return Some(DialogAction::Cancel);
            }
            TextInputKey::Enter => {
                let text = self.pass_field.buffer.as_str().trim();
                if !text.is_empty() {
                    return Some(DialogAction::Submit(SecretText::from_slice(text)));
                }
            }
            _ => {
                let ev =
                    self.pass_field
                        .handle_key(key, modifiers, Duration::ZERO, fonts, clipboard);
                if ev != TextInputEvent::Ignored {
                    self.dirty = true;
                    if self.is_error {
                        self.is_error = false;
                        self.status_msg = None;
                    }
                }
            }
        }
        None
    }

    /// Handles pointer click and triggers submit/cancel when matching buttons.
    pub(crate) fn on_pointer_click(&mut self, x: i32, y: i32) -> Option<DialogAction> {
        if self.checking {
            return None;
        }

        if self.cancel_rect.contains(x, y) {
            return Some(DialogAction::Cancel);
        }

        if self.submit_rect.contains(x, y) {
            let text = self.pass_field.buffer.as_str().trim();
            if !text.is_empty() {
                return Some(DialogAction::Submit(SecretText::from_slice(text)));
            }
        }

        None
    }

    /// Paints the complete Liquid Glass styled card onto [`UiPainter`].
    pub(crate) fn paint(
        &mut self,
        painter: &mut UiPainter,
        fonts: &mut UiFontEngine,
        pal: &ThemePalette,
        w: u32,
        h: u32,
    ) {
        let card_rect = Rect::new(0, 0, w, h);
        let card_bg = if pal.is_dark {
            [28, 30, 36, 255]
        } else {
            [248, 249, 252, 255]
        };
        painter.fill_rounded_rect(card_rect, 18.0, card_bg);

        let border_color = if pal.is_dark {
            [255, 255, 255, 34]
        } else {
            [0, 0, 0, 24]
        };
        painter.stroke_rounded_rect(card_rect, 18.0, 1.0, border_color);

        // Header icon badge: Lock / Shield
        let icon_rect = Rect::new(i32::try_from(w / 2 - 18).unwrap_or(192), 16, 36, 36);
        let icon_badge_bg = if pal.is_dark {
            [38, 42, 52, 255]
        } else {
            [232, 236, 244, 255]
        };
        painter.fill_rounded_rect(icon_rect, 18.0, icon_badge_bg);
        painter.stroke_rounded_rect(icon_rect, 18.0, 1.0, border_color);

        // Draw lock glyph inside badge
        let lock_glyph_rect = Rect::new(icon_rect.x + 10, icon_rect.y + 10, 16, 16);
        painter.draw_text_centered(
            fonts,
            "🔒",
            lock_glyph_rect,
            13.0,
            false,
            [255, 255, 255, 255],
        );

        // Title
        let title_rect = Rect::new(20, 58, w.saturating_sub(40), 22);
        painter.draw_text_centered(
            fonts,
            "Аутентификация",
            title_rect,
            15.0,
            true,
            pal.text_primary,
        );

        // Action explanation message
        let msg_rect = Rect::new(24, 82, w.saturating_sub(48), 24);
        let display_msg = if self.message.is_empty() {
            "Требуются привилегии администратора.".to_owned()
        } else {
            self.message.clone()
        };
        painter.draw_text_centered(
            fonts,
            display_msg,
            msg_rect,
            11.5,
            false,
            pal.text_secondary,
        );

        // User pill: User name
        let user_label = format!("Пользователь: {}", self.username);
        let user_rect = Rect::new(24, 108, w.saturating_sub(48), 18);
        painter.draw_text_centered(
            fonts,
            user_label,
            user_rect,
            11.0,
            false,
            if pal.is_dark {
                [180, 186, 200, 255]
            } else {
                [90, 96, 110, 255]
            },
        );

        // Password input field
        let pass_focused = self.focus.is_focused(focus_id_password());
        self.pass_field.paint(
            painter,
            fonts,
            pal,
            pass_focused,
            if pass_focused { 1.0 } else { 0.0 },
            Duration::from_millis(500),
        );

        // Status or Error message
        if let Some(status) = &self.status_msg {
            let status_rect = Rect::new(36, 170, w.saturating_sub(72), 18);
            let text_color = if self.is_error {
                [239, 68, 68, 255] // Red
            } else {
                pal.text_secondary
            };
            painter.draw_text_centered(fonts, status.clone(), status_rect, 11.0, false, text_color);
        }

        // Action buttons
        self.paint_buttons(painter, fonts, pal);
    }

    fn paint_buttons(&self, painter: &mut UiPainter, fonts: &mut UiFontEngine, pal: &ThemePalette) {
        let cancel_btn = PushButton::new(self.cancel_rect, ButtonStyle::Secondary);
        cancel_btn.paint_labeled(painter, fonts, "Отмена", pal, false);
        if self.focus.is_focused(focus_id_cancel()) {
            painter.stroke_rounded_rect(self.cancel_rect.inset(-2, -2), 8.0, 2.0, pal.accent);
        }

        let submit_disabled = self.pass_field.buffer.as_str().trim().is_empty() || self.checking;
        let submit_btn = PushButton::new(self.submit_rect, ButtonStyle::Primary);
        submit_btn.paint_labeled(
            painter,
            fonts,
            if self.checking {
                "Проверка..."
            } else {
                "Подтвердить"
            },
            pal,
            submit_disabled,
        );
        if self.focus.is_focused(focus_id_submit()) {
            painter.stroke_rounded_rect(self.submit_rect.inset(-2, -2), 8.0, 2.0, pal.accent);
        }
    }
}
