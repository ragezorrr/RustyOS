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
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
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
        crate::shadow::draw_elevation_shadow(
            canvas,
            rect,
            MENU_RADIUS as u32,
            crate::shadow::ShadowElevation::MenuOrTooltip,
            pal,
        );
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

/// Keyboard- and pointer-driven context menu host state (`RUSTYOS-130`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextMenuSession<T: Clone> {
    pub layout: Option<MenuLayout>,
    pub entries: Vec<MenuEntry<T>>,
    pub hovered_idx: Option<usize>,
    pub compositor_glass: bool,
}

impl<T: Clone> Default for ContextMenuSession<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone> ContextMenuSession<T> {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            layout: None,
            entries: Vec::new(),
            hovered_idx: None,
            compositor_glass: false,
        }
    }

    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.layout.is_some()
    }

    pub fn open(
        &mut self,
        anchor_x: i32,
        anchor_y: i32,
        min_width: i32,
        entries: Vec<MenuEntry<T>>,
    ) {
        let layout = MenuLayout::compute(anchor_x, anchor_y, min_width, &entries);
        self.hovered_idx = None;
        self.entries = entries;
        self.layout = Some(layout);
    }

    pub fn open_clamped(
        &mut self,
        anchor_x: i32,
        anchor_y: i32,
        min_width: i32,
        screen_w: i32,
        screen_h: i32,
        entries: Vec<MenuEntry<T>>,
    ) {
        let mut layout = MenuLayout::compute(anchor_x, anchor_y, min_width, &entries);
        let clamped = layout.clamp_to_screen(screen_w, screen_h);
        if clamped != layout.rect {
            layout = MenuLayout::compute(clamped.x, clamped.y, min_width, &entries);
        }
        self.hovered_idx = None;
        self.entries = entries;
        self.layout = Some(layout);
    }

    pub fn close(&mut self) {
        self.layout = None;
        self.entries.clear();
        self.hovered_idx = None;
    }

    pub fn on_pointer_move(&mut self, px: f64, py: f64) -> bool {
        let next = self.layout.as_ref().and_then(|l| l.hit_test(px, py));
        let changed = self.hovered_idx != next;
        self.hovered_idx = next;
        changed
    }

    /// Clicks an enabled menu item or dismisses when clicking outside; returns triggered payload.
    pub fn on_pointer_down(&mut self, px: f64, py: f64) -> Option<T> {
        let hit_idx = self.layout.as_ref()?.hit_test(px, py);
        if let Some(idx) = hit_idx
            && let Some(MenuEntry::Item(item)) = self.entries.get(idx)
            && item.enabled
        {
            let payload = item.payload.clone();
            self.close();
            return Some(payload);
        }
        self.close();
        None
    }

    /// Moves keyboard highlight to the previous (`-1`) or next (`+1`) enabled entry.
    pub fn move_hover(&mut self, step: isize) -> Option<usize> {
        let len = self.entries.len() as isize;
        if len == 0 {
            return None;
        }
        let start = self
            .hovered_idx
            .map_or(if step >= 0 { -1 } else { 0 }, |i| i as isize);
        for offset in 1..=len {
            let cand = ((start + step.signum() * offset).rem_euclid(len)) as usize;
            if let Some(MenuEntry::Item(item)) = self.entries.get(cand)
                && item.enabled
            {
                self.hovered_idx = Some(cand);
                return Some(cand);
            }
        }
        None
    }

    /// Activates the currently highlighted menu entry and closes the menu.
    pub fn activate_hovered(&mut self) -> Option<T> {
        let idx = self.hovered_idx?;
        if let Some(MenuEntry::Item(item)) = self.entries.get(idx)
            && item.enabled
        {
            let payload = item.payload.clone();
            self.close();
            return Some(payload);
        }
        None
    }

    pub fn paint(
        &self,
        painter: &mut crate::draw::UiPainter<'_>,
        fonts: &mut crate::text::UiFontEngine,
        pal: &ThemePalette,
    ) {
        if let Some(layout) = &self.layout {
            paint_menu_with_painter(
                painter,
                fonts,
                layout,
                &self.entries,
                self.hovered_idx,
                pal,
                self.compositor_glass,
            );
        }
    }
}

/// Renders a menu card, dividers, concentric hover pill, and labels via [`crate::draw::UiPainter`].
///
/// When `compositor_glass` is `true`, the outer card background remains transparent (`wl_shm`)
/// so the compositor supplies `GlassStyle::Popover` backdrop blur (`DESIGN.md` §6).
pub fn paint_menu_with_painter<T: Clone>(
    painter: &mut crate::draw::UiPainter<'_>,
    fonts: &mut crate::text::UiFontEngine,
    layout: &MenuLayout,
    entries: &[MenuEntry<T>],
    hovered_idx: Option<usize>,
    pal: &ThemePalette,
    compositor_glass: bool,
) {
    if !compositor_glass {
        painter.draw_shadow(
            layout.rect,
            MENU_RADIUS,
            crate::shadow::ShadowElevation::MenuOrTooltip,
            pal.is_dark,
        );
        let bg = if pal.is_dark {
            [0x2A, 0x2B, 0x2F, 242]
        } else {
            [0xF6, 0xF7, 0xFA, 244]
        };
        let border = if pal.is_dark {
            [255, 255, 255, 24]
        } else {
            [0, 0, 0, 18]
        };
        painter.rounded_rect_styled(layout.rect, MENU_RADIUS as f32, bg, 1.0, border, true);
    }

    let has_icon_col = entries
        .iter()
        .any(|e| matches!(e, MenuEntry::Item(it) if it.icon_glyph.is_some()));

    for (row_layout, entry) in layout.rows.iter().zip(entries.iter()) {
        match entry {
            MenuEntry::Separator => {
                let sep_y = row_layout.rect.y + row_layout.rect.height as i32 / 2;
                let sep_color = if pal.is_dark {
                    [255, 255, 255, 18]
                } else {
                    [0, 0, 0, 14]
                };
                let left = row_layout.rect.x + 12;
                let width = row_layout.rect.width.saturating_sub(24);
                if width > 0 {
                    painter.fill_rect(Rect::new(left, sep_y, width, 1), sep_color);
                }
            }
            MenuEntry::Item(item) => {
                paint_menu_item_row(
                    painter,
                    fonts,
                    layout,
                    row_layout,
                    item,
                    hovered_idx == Some(row_layout.index) && item.enabled,
                    has_icon_col,
                    pal,
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_menu_item_row<T: Clone>(
    painter: &mut crate::draw::UiPainter<'_>,
    fonts: &mut crate::text::UiFontEngine,
    layout: &MenuLayout,
    row_layout: &MenuRowLayout,
    item: &MenuItem<T>,
    hovered: bool,
    has_icon_col: bool,
    pal: &ThemePalette,
) {
    let danger_rgb = if pal.is_dark {
        crate::theme::ACCENT_DARK_PALETTE[1]
    } else {
        crate::theme::ACCENT_PALETTE[1]
    };
    if hovered {
        let pill = Rect::new(
            row_layout.rect.x,
            row_layout.rect.y + 1,
            row_layout.rect.width,
            row_layout.rect.height.saturating_sub(2),
        );
        let hover_col = if item.danger {
            [danger_rgb[0], danger_rgb[1], danger_rgb[2], 232]
        } else if pal.is_dark {
            [pal.accent[0], pal.accent[1], pal.accent[2], 210]
        } else {
            [pal.accent[0], pal.accent[1], pal.accent[2], 230]
        };
        painter.fill_rounded_rect(pill, HOVER_PILL_RADIUS as f32, hover_col);
    }

    let text_col = if hovered {
        [255, 255, 255, 255]
    } else if !item.enabled {
        pal.text_muted
    } else if item.danger {
        danger_rgb
    } else {
        pal.text_primary
    };

    let text_x = layout.rect.x as f32
        + if layout.has_check_column || has_icon_col {
            30.0
        } else {
            14.0
        };
    let baseline =
        row_layout.rect.y as f32 + f32::midpoint(row_layout.rect.height as f32, 13.0 * 0.72);

    if item.checked {
        let cx = layout.rect.x as f32 + 14.0;
        let cy = row_layout.rect.y as f32 + row_layout.rect.height as f32 * 0.5;
        painter.draw_line(cx, cy, cx + 2.5, cy + 3.0, 1.5, text_col);
        painter.draw_line(cx + 2.5, cy + 3.0, cx + 7.0, cy - 3.0, 1.5, text_col);
    } else if let Some(glyph) = item.icon_glyph {
        painter.draw_text(
            glyph.to_owned(),
            layout.rect.x as f32 + 12.0,
            baseline,
            12.0,
            false,
            text_col,
        );
    }

    painter.draw_text(item.text.clone(), text_x, baseline, 13.0, false, text_col);

    if item.has_submenu {
        let cx = layout.rect.right() as f32 - 16.0;
        let cy = row_layout.rect.y as f32 + row_layout.rect.height as f32 * 0.5;
        painter.draw_line(cx - 2.5, cy - 3.5, cx + 1.5, cy, 1.5, text_col);
        painter.draw_line(cx + 1.5, cy, cx - 2.5, cy + 3.5, 1.5, text_col);
    } else if let Some(sc) = &item.shortcut {
        let sw = fonts.measure_width(sc, 12.0, false);
        let sx = layout.rect.right() as f32 - 14.0 - sw;
        let sc_col = if hovered {
            [255, 255, 255, 235]
        } else {
            pal.text_secondary
        };
        painter.draw_text(sc.clone(), sx, baseline, 12.0, false, sc_col);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw::{UiDrawCmd, UiDrawList, UiPainter};

    #[test]
    fn menu_painter_renders_shadow_glyph_chevron_and_respects_with_enabled() {
        let entries = vec![
            MenuEntry::Item(
                MenuItem::new("Disabled Item", 1)
                    .with_enabled(false)
                    .with_glyph("•"),
            ),
            MenuEntry::Item(MenuItem::new("Open With", 2).with_submenu(true)),
        ];
        let mut session = ContextMenuSession::new();
        session.open_clamped(40, 40, 200, 800, 600, entries);
        assert_eq!(session.move_hover(1), Some(1)); // skips disabled index 0

        let mut dl = UiDrawList::new();
        let mut painter = UiPainter::new(&mut dl);
        let mut fonts = crate::text::UiFontEngine::new();
        let pal = ThemePalette::new(true, 0);
        session.paint(&mut painter, &mut fonts, &pal);

        assert!(
            dl.commands
                .iter()
                .any(|c| matches!(c, UiDrawCmd::Shadow { .. }))
        );
        assert!(
            dl.commands
                .iter()
                .filter(|c| matches!(c, UiDrawCmd::Line { .. }))
                .count()
                >= 2
        );
    }
}
