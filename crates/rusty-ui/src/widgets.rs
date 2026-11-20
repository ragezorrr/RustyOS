//! Standard reusable UI components and controls for RustyOS.

use crate::canvas::UiCanvas;
use crate::geometry::Rect;
use crate::theme::ThemePalette;

/// Visual and interactive state of a button or control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WidgetState {
    #[default]
    Normal,
    Hover,
    Pressed,
    Disabled,
}

/// Standard toggle switch control (compact 38×20 or full-size 48×28).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToggleSwitch {
    pub rect: Rect,
    pub active: bool,
    pub state: WidgetState,
}

impl ToggleSwitch {
    #[must_use]
    pub const fn new(rect: Rect, active: bool) -> Self {
        Self {
            rect,
            active,
            state: WidgetState::Normal,
        }
    }

    /// Checks if point `(px, py)` falls inside the switch track.
    #[must_use]
    pub fn is_hovered(&self, px: f64, py: f64) -> bool {
        self.rect.contains_f64(px, py)
    }

    /// Draws the toggle switch onto a `UiCanvas`.
    pub fn draw(&self, canvas: &mut UiCanvas<'_>, pal: &ThemePalette) {
        let r = self.rect.height as f32 * 0.5;
        let bg = if self.active {
            match self.state {
                WidgetState::Pressed => [
                    ((pal.accent[0] as u16 * 8) / 10) as u8,
                    ((pal.accent[1] as u16 * 8) / 10) as u8,
                    ((pal.accent[2] as u16 * 8) / 10) as u8,
                    pal.accent[3],
                ],
                WidgetState::Hover => [
                    ((pal.accent[0] as u16 * 9) / 10) as u8,
                    ((pal.accent[1] as u16 * 9) / 10) as u8,
                    ((pal.accent[2] as u16 * 9) / 10) as u8,
                    pal.accent[3],
                ],
                _ => pal.accent,
            }
        } else if pal.is_dark {
            match self.state {
                WidgetState::Pressed => [0x30, 0x30, 0x33, 255],
                WidgetState::Hover => [0x42, 0x42, 0x46, 255],
                _ => [0x39, 0x39, 0x3D, 255],
            }
        } else {
            match self.state {
                WidgetState::Pressed => [0xD0, 0xD0, 0xD4, 255],
                WidgetState::Hover => [0xDF, 0xE0, 0xE4, 255],
                _ => [0xE9, 0xE9, 0xEB, 255],
            }
        };

        canvas.fast_rounded_rect(self.rect, r, bg);

        let knob_r = (r - 2.0).max(4.0);
        let knob_cx = if self.active {
            self.rect.right() as f32 - r
        } else {
            self.rect.x as f32 + r
        };
        let knob_cy = self.rect.y as f32 + r;

        // Subtle knob drop shadow
        canvas.draw_circle_aa(knob_cx, knob_cy + 0.5, knob_r + 0.5, [0, 0, 0, 35]);
        // White knob
        canvas.draw_circle_aa(knob_cx, knob_cy, knob_r, [255, 255, 255, 255]);
    }
}

/// Visual style of a push button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonStyle {
    #[default]
    Primary,
    Secondary,
    Destructive,
}

/// Standard push / pill action button.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PushButton {
    pub rect: Rect,
    pub style: ButtonStyle,
    pub state: WidgetState,
    pub corner_radius: f32,
}

impl PushButton {
    #[must_use]
    pub const fn new(rect: Rect, style: ButtonStyle) -> Self {
        Self {
            rect,
            style,
            state: WidgetState::Normal,
            corner_radius: 8.0,
        }
    }

    #[must_use]
    pub const fn with_corner_radius(mut self, radius: f32) -> Self {
        self.corner_radius = radius;
        self
    }

    /// Checks if point `(px, py)` falls inside the button.
    #[must_use]
    pub fn is_hovered(&self, px: f64, py: f64) -> bool {
        self.rect.contains_f64(px, py)
    }

    /// Draws the button background and border onto a `UiCanvas`.
    pub fn draw(&self, canvas: &mut UiCanvas<'_>, pal: &ThemePalette) {
        let (bg, border) = match self.style {
            ButtonStyle::Primary => {
                let base = pal.accent;
                match self.state {
                    WidgetState::Pressed => (
                        [
                            ((base[0] as u16 * 8) / 10) as u8,
                            ((base[1] as u16 * 8) / 10) as u8,
                            ((base[2] as u16 * 8) / 10) as u8,
                            base[3],
                        ],
                        None,
                    ),
                    WidgetState::Hover => (
                        [
                            ((base[0] as u16 * 9) / 10) as u8,
                            ((base[1] as u16 * 9) / 10) as u8,
                            ((base[2] as u16 * 9) / 10) as u8,
                            base[3],
                        ],
                        None,
                    ),
                    WidgetState::Disabled => ([base[0], base[1], base[2], 100], None),
                    WidgetState::Normal => (base, None),
                }
            }
            ButtonStyle::Secondary => {
                if pal.is_dark {
                    match self.state {
                        WidgetState::Pressed => ([0x38, 0x38, 0x3C, 255], Some(pal.divider)),
                        WidgetState::Hover => ([0x48, 0x48, 0x4C, 255], Some(pal.divider)),
                        WidgetState::Disabled => ([0x2C, 0x2C, 0x2E, 100], None),
                        WidgetState::Normal => ([0x3A, 0x3A, 0x3C, 255], Some(pal.divider)),
                    }
                } else {
                    match self.state {
                        WidgetState::Pressed => ([0xD8, 0xD9, 0xDD, 255], Some(pal.divider)),
                        WidgetState::Hover => ([0xE4, 0xE5, 0xE9, 255], Some(pal.divider)),
                        WidgetState::Disabled => ([0xEA, 0xEB, 0xEF, 100], None),
                        WidgetState::Normal => ([0xEE, 0xEF, 0xF2, 255], Some(pal.divider)),
                    }
                }
            }
            ButtonStyle::Destructive => {
                let base = [0xFF, 0x3B, 0x30, 255];
                match self.state {
                    WidgetState::Pressed => ([0xCC, 0x2F, 0x26, 255], None),
                    WidgetState::Hover => ([0xE6, 0x35, 0x2B, 255], None),
                    WidgetState::Disabled => ([0xFF, 0x3B, 0x30, 100], None),
                    WidgetState::Normal => (base, None),
                }
            }
        };

        canvas.fast_rounded_rect(self.rect, self.corner_radius, bg);
        if let Some(border_col) = border {
            // Subtle 1 px boundary line
            canvas.stroke_line_aa(
                self.rect.x as f32,
                self.rect.bottom() as f32 - 0.5,
                self.rect.right() as f32,
                self.rect.bottom() as f32 - 0.5,
                1.0,
                border_col,
            );
        }
    }
}

/// Standard grouped container card for settings rows, lists, or dialog content.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GroupCard {
    pub rect: Rect,
    pub corner_radius: f32,
}

impl GroupCard {
    #[must_use]
    pub const fn new(rect: Rect) -> Self {
        Self {
            rect,
            corner_radius: 12.0,
        }
    }

    #[must_use]
    pub const fn with_corner_radius(mut self, radius: f32) -> Self {
        self.corner_radius = radius;
        self
    }

    /// Draws the card surface background onto a `UiCanvas`.
    pub fn draw(&self, canvas: &mut UiCanvas<'_>, pal: &ThemePalette) {
        canvas.fast_rounded_rect(self.rect, self.corner_radius, pal.card_bg);
    }

    /// Draws an interior 1 px row divider inside the card, inset from left and right.
    pub fn draw_row_divider(
        &self,
        canvas: &mut UiCanvas<'_>,
        y: i32,
        inset_x: u32,
        pal: &ThemePalette,
    ) {
        if y > self.rect.y && y < self.rect.bottom() {
            let left = self.rect.x + inset_x as i32;
            let right = self.rect.right() - inset_x as i32;
            if right > left {
                canvas.fill_rect(Rect::new(left, y, (right - left) as u32, 1), pal.divider);
            }
        }
    }
}

/// Target hit inside a `SearchField`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchFieldHit {
    Input,
    Clear,
}

/// Standard search field capsule with magnifying lens and clear button.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SearchField {
    pub rect: Rect,
    pub corner_radius: f32,
    pub has_text: bool,
    pub focused: bool,
}

impl SearchField {
    #[must_use]
    pub const fn new(rect: Rect) -> Self {
        Self {
            rect,
            corner_radius: 14.0,
            has_text: false,
            focused: false,
        }
    }

    /// Rectangle for the circular clear button (14×14 px on the right).
    #[must_use]
    pub fn clear_btn_rect(&self) -> Rect {
        let size = 14;
        let right_x = self.rect.right() - 8 - size as i32;
        let center_y = self.rect.y + (self.rect.height as i32 - size as i32) / 2;
        Rect::new(right_x, center_y, size, size)
    }

    /// Hit-tests pointer coordinates against input area or clear button.
    #[must_use]
    pub fn hit_test(&self, px: f64, py: f64) -> Option<SearchFieldHit> {
        if !self.rect.contains_f64(px, py) {
            return None;
        }
        if self.has_text && self.clear_btn_rect().contains_f64(px, py) {
            return Some(SearchFieldHit::Clear);
        }
        Some(SearchFieldHit::Input)
    }

    /// Draws the search field capsule, lens icon, and optional clear button.
    pub fn draw(&self, canvas: &mut UiCanvas<'_>, pal: &ThemePalette) {
        let bg = if pal.is_dark {
            if self.focused {
                [0x2A, 0x2A, 0x2E, 255]
            } else {
                [0x22, 0x22, 0x25, 255]
            }
        } else if self.focused {
            [0xDF, 0xE1, 0xE5, 255]
        } else {
            [0xE5, 0xE7, 0xEB, 255]
        };

        canvas.fast_rounded_rect(self.rect, self.corner_radius, bg);

        if self.focused {
            // Draw focus ring indicator
            let [r, g, b, _] = pal.accent;
            let ring_col = [r, g, b, 120];
            canvas.stroke_line_aa(
                self.rect.x as f32 + self.corner_radius,
                self.rect.y as f32 + 0.5,
                self.rect.right() as f32 - self.corner_radius,
                self.rect.y as f32 + 0.5,
                1.0,
                ring_col,
            );
            canvas.stroke_line_aa(
                self.rect.x as f32 + self.corner_radius,
                self.rect.bottom() as f32 - 0.5,
                self.rect.right() as f32 - self.corner_radius,
                self.rect.bottom() as f32 - 0.5,
                1.0,
                ring_col,
            );
        }

        // Draw left magnifying glass lens icon
        let lens_cx = self.rect.x as f32 + 13.0;
        let lens_cy = self.rect.y as f32 + (self.rect.height as f32 * 0.5) - 1.0;
        let icon_col = pal.text_muted;
        canvas.draw_circle_aa(lens_cx, lens_cy, 4.0, icon_col);
        canvas.draw_circle_aa(lens_cx, lens_cy, 2.5, bg);
        canvas.stroke_line_aa(
            lens_cx + 2.5,
            lens_cy + 2.5,
            lens_cx + 5.5,
            lens_cy + 5.5,
            1.2,
            icon_col,
        );

        // Draw clear button if field contains text
        if self.has_text {
            let clear_rect = self.clear_btn_rect();
            let ccx = clear_rect.center_f32().0;
            let ccy = clear_rect.center_f32().1;
            canvas.draw_circle_aa(ccx, ccy, 6.0, pal.text_muted);
            let x_ink = if pal.is_dark {
                [0x22, 0x22, 0x25, 255]
            } else {
                [0xFF, 0xFF, 0xFF, 255]
            };
            canvas.stroke_line_aa(ccx - 2.5, ccy - 2.5, ccx + 2.5, ccy + 2.5, 1.0, x_ink);
            canvas.stroke_line_aa(ccx + 2.5, ccy - 2.5, ccx - 2.5, ccy + 2.5, 1.0, x_ink);
        }
    }
}
