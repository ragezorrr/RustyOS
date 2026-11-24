//! Floating capsule toolbar with Liquid Glass styling and auto-hide behavior for `rusty-view`.

use std::time::{Duration, Instant};

use rusty_ui::{Rect, ThemePalette, UiFontEngine, UiPainter};

pub(crate) const TOOLBAR_WIDTH: u32 = 330;
pub(crate) const TOOLBAR_HEIGHT: u32 = 42;
pub(crate) const TOOLBAR_RADIUS: f32 = 21.0;
pub(crate) const AUTO_HIDE_TIMEOUT: Duration = Duration::from_millis(2500);

/// Actions triggered by the floating toolbar buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolbarAction {
    Prev,
    ZoomOut,
    ZoomReset,
    ZoomIn,
    FitToWindow,
    RotateCw,
    ToggleInfo,
    Next,
}

/// Floating capsule toolbar managing button slots, hover/pressed state, and idle auto-hide.
#[derive(Debug)]
pub(crate) struct Toolbar {
    pub(crate) hovered_action: Option<ToolbarAction>,
    pub(crate) pressed_action: Option<ToolbarAction>,
    pub(crate) last_motion: Instant,
    pub(crate) opacity: f32,
    pub(crate) is_pointer_inside: bool,
}

impl Default for Toolbar {
    fn default() -> Self {
        Self {
            hovered_action: None,
            pressed_action: None,
            last_motion: Instant::now(),
            opacity: 1.0,
            is_pointer_inside: false,
        }
    }
}

impl Toolbar {
    /// Computes the bounding rectangle of the floating toolbar capsule.
    #[must_use]
    pub(crate) fn compute_rect(win_w: u32, win_h: u32) -> Rect {
        let x = ((win_w as i32) - (TOOLBAR_WIDTH as i32)) / 2;
        let y = (win_h as i32) - (TOOLBAR_HEIGHT as i32) - 18;
        Rect::new(x.max(8), y.max(8), TOOLBAR_WIDTH, TOOLBAR_HEIGHT)
    }

    /// Records pointer movement to wake up the toolbar.
    pub(crate) fn on_pointer_moved(&mut self, px: f64, py: f64, win_w: u32, win_h: u32) {
        self.last_motion = Instant::now();
        self.opacity = 1.0;

        let rect = Self::compute_rect(win_w, win_h);
        self.is_pointer_inside = rect.contains(px as i32, py as i32);
        self.hovered_action = if self.is_pointer_inside {
            Self::hit_test_action(px, py, rect)
        } else {
            None
        };
    }

    /// Handles pointer click down on the toolbar.
    pub(crate) fn on_pointer_down(
        &mut self,
        px: f64,
        py: f64,
        win_w: u32,
        win_h: u32,
    ) -> Option<ToolbarAction> {
        let rect = Self::compute_rect(win_w, win_h);
        if rect.contains(px as i32, py as i32) {
            let action = Self::hit_test_action(px, py, rect);
            self.pressed_action = action;
            action
        } else {
            None
        }
    }

    /// Handles pointer click release on the toolbar.
    pub(crate) fn on_pointer_up(
        &mut self,
        px: f64,
        py: f64,
        win_w: u32,
        win_h: u32,
    ) -> Option<ToolbarAction> {
        let rect = Self::compute_rect(win_w, win_h);
        let pressed = self.pressed_action.take();
        if rect.contains(px as i32, py as i32) {
            let hit = Self::hit_test_action(px, py, rect);
            if hit == pressed {
                return hit;
            }
        }
        None
    }

    /// Updates toolbar auto-hide state based on elapsed idle time. Returns true if visibility changed.
    pub(crate) fn update_visibility(&mut self) -> bool {
        if self.is_pointer_inside {
            let changed = (self.opacity - 1.0).abs() > 0.01;
            self.opacity = 1.0;
            return changed;
        }

        let elapsed = self.last_motion.elapsed();
        let target_opacity = if elapsed > AUTO_HIDE_TIMEOUT {
            0.0
        } else if elapsed > AUTO_HIDE_TIMEOUT.saturating_sub(Duration::from_millis(400)) {
            let remain = AUTO_HIDE_TIMEOUT.saturating_sub(elapsed).as_millis() as f32;
            (remain / 400.0).clamp(0.0, 1.0)
        } else {
            1.0
        };

        let diff = (self.opacity - target_opacity).abs();
        if diff > 0.02 {
            self.opacity = target_opacity;
            true
        } else {
            false
        }
    }

    /// Hit-tests individual buttons within the toolbar bounds.
    #[must_use]
    fn hit_test_action(px: f64, py: f64, rect: Rect) -> Option<ToolbarAction> {
        let slots = compute_button_slots(rect);
        let x = px as i32;
        let y = py as i32;

        for (action, slot_rect) in slots {
            if slot_rect.contains(x, y) {
                return Some(action);
            }
        }
        None
    }

    /// Renders the floating Liquid Glass capsule toolbar with icons and zoom display.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn paint(
        &self,
        painter: &mut UiPainter<'_>,
        fonts: &mut UiFontEngine,
        pal: &ThemePalette,
        win_w: u32,
        win_h: u32,
        zoom_text: &str,
    ) {
        if self.opacity < 0.02 {
            return;
        }

        let rect = Self::compute_rect(win_w, win_h);
        let alpha_u8 = (self.opacity * 255.0).round() as u8;

        let bg_alpha = ((if pal.is_dark { 210.0 } else { 230.0 }) * self.opacity).round() as u8;
        let bg_color = if pal.is_dark {
            [26, 28, 34, bg_alpha]
        } else {
            [248, 250, 254, bg_alpha]
        };
        let border_alpha = ((if pal.is_dark { 40.0 } else { 30.0 }) * self.opacity).round() as u8;
        let border_color = if pal.is_dark {
            [255, 255, 255, border_alpha]
        } else {
            [0, 0, 0, border_alpha]
        };

        painter.fill_rounded_rect(rect, TOOLBAR_RADIUS, bg_color);
        painter.stroke_rounded_rect(rect, TOOLBAR_RADIUS, 1.0, border_color);

        let slots = compute_button_slots(rect);
        let text_color = if pal.is_dark {
            [235, 238, 246, alpha_u8]
        } else {
            [30, 34, 44, alpha_u8]
        };
        let hover_bg = if pal.is_dark {
            [255, 255, 255, (28.0 * self.opacity).round() as u8]
        } else {
            [0, 0, 0, (18.0 * self.opacity).round() as u8]
        };
        let press_bg = if pal.is_dark {
            [255, 255, 255, (48.0 * self.opacity).round() as u8]
        } else {
            [0, 0, 0, (36.0 * self.opacity).round() as u8]
        };

        for (action, slot_rect) in slots {
            let is_pressed = self.pressed_action == Some(action);
            let is_hovered = self.hovered_action == Some(action);

            if is_pressed {
                painter.fill_rounded_rect(slot_rect, 16.0, press_bg);
            } else if is_hovered {
                painter.fill_rounded_rect(slot_rect, 16.0, hover_bg);
            }

            match action {
                ToolbarAction::Prev => {
                    painter.draw_text_centered(fonts, "◀", slot_rect, 13.0, true, text_color);
                }
                ToolbarAction::ZoomOut => {
                    painter.draw_text_centered(fonts, "−", slot_rect, 16.0, true, text_color);
                }
                ToolbarAction::ZoomReset => {
                    painter.draw_text_centered(fonts, zoom_text, slot_rect, 11.5, true, text_color);
                }
                ToolbarAction::ZoomIn => {
                    painter.draw_text_centered(fonts, "+", slot_rect, 15.0, true, text_color);
                }
                ToolbarAction::FitToWindow => {
                    painter.draw_text_centered(fonts, "⛶", slot_rect, 14.0, false, text_color);
                }
                ToolbarAction::RotateCw => {
                    painter.draw_text_centered(fonts, "↷", slot_rect, 16.0, true, text_color);
                }
                ToolbarAction::ToggleInfo => {
                    painter.draw_text_centered(fonts, "ℹ", slot_rect, 13.0, true, text_color);
                }
                ToolbarAction::Next => {
                    painter.draw_text_centered(fonts, "▶", slot_rect, 13.0, true, text_color);
                }
            }
        }
    }
}

fn compute_button_slots(bar: Rect) -> Vec<(ToolbarAction, Rect)> {
    let btn_h = 32;
    let y = bar.y + ((bar.height as i32 - btn_h as i32) / 2);

    let items = [
        (ToolbarAction::Prev, 34),
        (ToolbarAction::ZoomOut, 32),
        (ToolbarAction::ZoomReset, 54),
        (ToolbarAction::ZoomIn, 32),
        (ToolbarAction::FitToWindow, 36),
        (ToolbarAction::RotateCw, 34),
        (ToolbarAction::ToggleInfo, 32),
        (ToolbarAction::Next, 34),
    ];

    let mut slots = Vec::with_capacity(items.len());
    let mut cur_x = bar.x + 8;

    for (act, width) in items {
        slots.push((act, Rect::new(cur_x, y, width, btn_h)));
        cur_x += (width as i32) + 5;
    }

    slots
}
