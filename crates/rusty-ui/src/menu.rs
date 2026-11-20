//! Standard desktop and context menu metrics, layout, and rendering.

use crate::canvas::UiCanvas;
use crate::geometry::Rect;
use crate::theme::ThemePalette;

/// Standard menu row height (26 px).
pub const ROW_HEIGHT: i32 = 26;

/// Standard hairline separator row height (12 px).
pub const SEPARATOR_HEIGHT: i32 = 12;

/// Standard menu outer corner radius (14 px).
pub const MENU_RADIUS: i32 = 14;

/// Floating hover pill corner radius (9 px = 14 px - 5 px margin).
pub const HOVER_PILL_RADIUS: i32 = 9;

/// Horizontal margin from menu border to content (5 px).
pub const CONTENT_MARGIN: i32 = 5;

/// Vertical margin from menu border to content (5 px).
pub const VERTICAL_CONTENT_MARGIN: i32 = 5;

/// Horizontal padding inside a row (8 px).
pub const ROW_PADDING: i32 = 8;

/// Text label left padding when check column is inactive (4 px).
pub const LABEL_PADDING: i32 = 4;

/// Horizontal spacing between glyph/icon and label (10 px).
pub const ITEM_SPACING: i32 = 10;

/// Spacing before right-aligned shortcut column (26 px).
pub const SHORTCUT_SPACING: i32 = 26;

/// Standard checkmark size (12 px).
pub const CHECK_SIZE: i32 = 12;

/// Spacing after checkmark (7 px).
pub const CHECK_SPACING: i32 = 7;

/// Leading icon size (22 px).
pub const ICON_SIZE: i32 = 22;

/// Leading vector microglyph size (16 px).
pub const GLYPH_SIZE: i32 = 16;

/// Trailing submenu chevron size (20 px).
pub const ARROW_SIZE: i32 = 20;

/// Cascade submenu overlap over parent menu frame (2 px).
pub const SUBMENU_OVERLAP: i32 = 2;

/// Submenu hover dismissal backstop delay (50 ms).
pub const SUBMENU_DISMISS_DELAY_MS: u64 = 50;

/// Screen edge margin backstop (6 px).
pub const SCREEN_MARGIN: i32 = 6;

/// Minimum menu popover width (158 px).
pub const MIN_MENU_WIDTH: i32 = 158;

/// Drop-down scroll indicator width (3 px).
pub const SCROLL_INDICATOR_WIDTH: i32 = 3;

/// Menu item specification.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct MenuItem<T: Clone = ()> {
    pub text: String,
    pub shortcut: Option<String>,
    pub icon_glyph: Option<&'static str>,
    pub checked: bool,
    pub enabled: bool,
    pub danger: bool,
    pub has_submenu: bool,
    pub payload: T,
}

impl<T: Clone> MenuItem<T> {
    #[must_use]
    pub fn new(text: impl Into<String>, payload: T) -> Self {
        Self {
            text: text.into(),
            shortcut: None,
            icon_glyph: None,
            checked: false,
            enabled: true,
            danger: false,
            has_submenu: false,
            payload,
        }
    }

    #[must_use]
    pub fn with_shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    #[must_use]
    pub fn with_glyph(mut self, glyph: &'static str) -> Self {
        self.icon_glyph = Some(glyph);
        self
    }

    #[must_use]
    pub fn with_checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    #[must_use]
    pub fn with_danger(mut self, danger: bool) -> Self {
        self.danger = danger;
        self
    }

    #[must_use]
    pub fn with_submenu(mut self, has_submenu: bool) -> Self {
        self.has_submenu = has_submenu;
        self
    }
}

/// Menu entry variant: interactive item or horizontal separator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuEntry<T: Clone = ()> {
    Item(MenuItem<T>),
    Separator,
}

/// Calculated geometry for a single menu row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuRowLayout {
    pub index: usize,
    pub rect: Rect,
    pub is_separator: bool,
    pub is_enabled: bool,
    pub has_submenu: bool,
}

/// Layout and geometry of an open menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuLayout {
    pub rect: Rect,
    pub rows: Vec<MenuRowLayout>,
    pub has_check_column: bool,
}

impl MenuLayout {
    /// Computes pixel-accurate menu layout for a list of entries anchored at `(anchor_x, anchor_y)`.
    #[must_use]
    pub fn compute<T: Clone>(
        anchor_x: i32,
        anchor_y: i32,
        min_width: i32,
        entries: &[MenuEntry<T>],
    ) -> Self {
        let width = min_width.max(MIN_MENU_WIDTH);
        let mut total_h = VERTICAL_CONTENT_MARGIN * 2;
        let mut has_check_column = false;

        for entry in entries {
            match entry {
                MenuEntry::Item(item) => {
                    total_h += ROW_HEIGHT;
                    if item.checked {
                        has_check_column = true;
                    }
                }
                MenuEntry::Separator => {
                    total_h += SEPARATOR_HEIGHT;
                }
            }
        }

        let rect = Rect::new(anchor_x, anchor_y, width as u32, total_h as u32);
        let mut rows = Vec::with_capacity(entries.len());
        let mut cursor_y = anchor_y + VERTICAL_CONTENT_MARGIN;

        for (idx, entry) in entries.iter().enumerate() {
            match entry {
                MenuEntry::Item(item) => {
                    let row_rect = Rect::new(
                        anchor_x + CONTENT_MARGIN,
                        cursor_y,
                        (width - CONTENT_MARGIN * 2) as u32,
                        ROW_HEIGHT as u32,
                    );
                    rows.push(MenuRowLayout {
                        index: idx,
                        rect: row_rect,
                        is_separator: false,
                        is_enabled: item.enabled,
                        has_submenu: item.has_submenu,
                    });
                    cursor_y += ROW_HEIGHT;
                }
                MenuEntry::Separator => {
                    let sep_rect = Rect::new(
                        anchor_x + CONTENT_MARGIN,
                        cursor_y,
                        (width - CONTENT_MARGIN * 2) as u32,
                        SEPARATOR_HEIGHT as u32,
                    );
                    rows.push(MenuRowLayout {
                        index: idx,
                        rect: sep_rect,
                        is_separator: true,
                        is_enabled: false,
                        has_submenu: false,
                    });
                    cursor_y += SEPARATOR_HEIGHT;
                }
            }
        }

        Self {
            rect,
            rows,
            has_check_column,
        }
    }

    /// Evaluates which row index is hit at `(px, py)`.
    #[must_use]
    pub fn hit_test(&self, px: f64, py: f64) -> Option<usize> {
        if !self.rect.contains_f64(px, py) {
            return None;
        }
        for row in &self.rows {
            if !row.is_separator && row.is_enabled && row.rect.contains_f64(px, py) {
                return Some(row.index);
            }
        }
        None
    }

    /// Clamps popover coordinates to stay at least `SCREEN_MARGIN` pixels inside `(screen_w, screen_h)`.
    #[must_use]
    pub fn clamp_to_screen(&self, screen_w: i32, screen_h: i32) -> Rect {
        let max_x = screen_w - self.rect.width as i32 - SCREEN_MARGIN;
        let max_y = screen_h - self.rect.height as i32 - SCREEN_MARGIN;
        let clamped_x = self.rect.x.clamp(SCREEN_MARGIN, max_x.max(SCREEN_MARGIN));
        let clamped_y = self.rect.y.clamp(SCREEN_MARGIN, max_y.max(SCREEN_MARGIN));
        Rect::new(clamped_x, clamped_y, self.rect.width, self.rect.height)
    }

    /// Calculates cascade submenu anchor position with 2 px overlap.
    #[must_use]
    pub fn cascade_submenu_anchor(&self, parent_row_idx: usize) -> (i32, i32) {
        if let Some(row) = self.rows.iter().find(|r| r.index == parent_row_idx) {
            let right_x = self.rect.right() - SUBMENU_OVERLAP;
            let sub_y = row.rect.y - VERTICAL_CONTENT_MARGIN;
            (right_x, sub_y)
        } else {
            (self.rect.right() - SUBMENU_OVERLAP, self.rect.y)
        }
    }
}

/// Renders the menu background surface with soft shadow and rounded corners.
pub fn draw_menu_background(
    canvas: &mut UiCanvas,
    rect: Rect,
    pal: &ThemePalette,
    draw_shadow: bool,
) {
    if draw_shadow {
        // Multi-stage soft drop shadow (dx=0, dy=2..4)
        for (dx, dy, g, a) in [(0, 3, 4, 16), (0, 2, 2, 28), (0, 1, 1, 40)] {
            canvas.fill_rounded_rect(
                Rect::new(
                    rect.x - g + dx,
                    rect.y - g + dy,
                    rect.width + (g * 2) as u32,
                    rect.height + (g * 2) as u32,
                ),
                MENU_RADIUS as u32 + g as u32,
                [0, 0, 0, a],
            );
        }
    }

    // Menu translucent background
    let bg = if pal.is_dark {
        [0x2C, 0x2C, 0x2D, 240]
    } else {
        [0xFF, 0xFF, 0xFF, 245]
    };
    canvas.fill_rounded_rect(rect, MENU_RADIUS as u32, bg);

    // Subtle 1px glass rim border
    let rim = if pal.is_dark {
        [0xFF, 0xFF, 0xFF, 30]
    } else {
        [0x00, 0x00, 0x00, 25]
    };
    canvas.stroke_rounded_rect(rect, MENU_RADIUS as u32, rim);
}

/// Renders a single row in the menu with floating hover pill if hovered.
pub fn draw_menu_row(
    canvas: &mut UiCanvas,
    row: &MenuRowLayout,
    is_hovered: bool,
    pal: &ThemePalette,
) {
    if row.is_separator {
        let sep_y = row.rect.y + row.rect.height as i32 / 2;
        let sep_color = if pal.is_dark {
            [0xFF, 0xFF, 0xFF, 35]
        } else {
            [0x00, 0x00, 0x00, 26]
        };
        let left = row.rect.x + 12;
        let width = row.rect.width.saturating_sub(24);
        if width > 0 {
            canvas.fill_rect(Rect::new(left, sep_y, width, 1), sep_color);
        }
        return;
    }

    if is_hovered && row.is_enabled {
        // Inset floating hover pill (concentric radius 9 px)
        let pill = Rect::new(
            row.rect.x,
            row.rect.y + 1,
            row.rect.width,
            row.rect.height.saturating_sub(2),
        );
        let hover_bg = if pal.is_dark {
            [pal.accent[0], pal.accent[1], pal.accent[2], 210] // alpha 0.82
        } else {
            [pal.accent[0], pal.accent[1], pal.accent[2], 230] // alpha 0.90
        };
        canvas.fill_rounded_rect(pill, HOVER_PILL_RADIUS as u32, hover_bg);
    }
}
