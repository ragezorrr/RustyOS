//! Bottom floating Dock layout, hit-testing, tooltips, and solid fallback rendering.

use smithay::backend::renderer::element::solid::SolidColorRenderElement;
use smithay::utils::{Point, Rectangle, Size};

use super::tokens::{
    COLOR_ACCENT, COLOR_ACCENT_HOVER, COLOR_ACCENT_MUTED, COLOR_BORDER_HIGHLIGHT, COLOR_MUTED,
    COLOR_SUBTLE, COLOR_SURFACE_ELEVATED, DOCK_DIVIDER_GAP, DOCK_HEIGHT, DOCK_ITEM_GAP,
    DOCK_ITEM_SIZE, DOCK_MARGIN_BOTTOM, DOCK_PADDING, DOCK_PINNED_COUNT, DOCK_UTILITY_COUNT,
    TOP_BAR_HEIGHT, make_solid_element, truncate_ascii,
};

/// Metadata for a single managed window entry rendered in Dock and Workspace Overview (`RUSTYOS-023`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockWindowEntry {
    /// Window manager identifier.
    pub id: rusty_wm::WindowId,
    /// Human-readable title displayed in Overview cards.
    pub title: String,
    /// Compact 1-2 character monogram displayed inside the Dock tile.
    pub badge: String,
    /// Optional resolved XDG icon name or PNG path (`RUSTYOS-033`).
    pub icon_name: Option<String>,
    /// Optional client-provided `xdg_toplevel_icon_v1` icon (`RUSTYOS-047`).
    pub toplevel_icon: Option<crate::icons::ShmToplevelIcon>,
    /// Dimensions label (e.g. `"800x600"`).
    pub dimensions: String,
    /// Whether this window is currently focused.
    pub focused: bool,
    /// Whether this window is currently minimized to the Dock (`RUSTYOS-032`).
    pub minimized: bool,
}

/// Computes a concise 2-character badge from a window title (`RUSTYOS-023`).
#[must_use]
pub fn make_window_badge(title: &str) -> String {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return "Wn".to_owned();
    }
    let words: Vec<&str> = trimmed
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    if words.len() >= 2 {
        let mut badge = String::new();
        if let Some(c1) = words[0].chars().next() {
            badge.push(c1.to_ascii_uppercase());
        }
        if let Some(c2) = words[1].chars().next() {
            badge.push(c2.to_ascii_uppercase());
        }
        if !badge.is_empty() {
            return badge;
        }
    }
    trimmed.chars().take(2).collect()
}

/// Extracts a clean application name from a window title (`RUSTYOS-038`).
#[must_use]
pub fn extract_app_name(title: &str) -> String {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return "RustyOS".to_owned();
    }
    // Check for standard XDG window title delimiters " — " or " - "
    if let Some((_doc, app)) = trimmed.rsplit_once(" — ") {
        let app_trimmed = app.trim();
        if !app_trimmed.is_empty() {
            return truncate_ascii(app_trimmed, 14);
        }
    }
    if let Some((_doc, app)) = trimmed.rsplit_once(" - ") {
        let app_trimmed = app.trim();
        if !app_trimmed.is_empty() {
            return truncate_ascii(app_trimmed, 14);
        }
    }
    truncate_ascii(trimmed, 14)
}

/// Dock view state describing open window tiles, hovered slot, and overview toggle highlight (`RUSTYOS-023`).
#[derive(Debug, Clone, Copy)]
pub struct DockViewState<'a> {
    /// Optional XDG icon names for pinned `[Terminal, Files]` Dock slots (`RUSTYOS-033`).
    pub pinned_icons: [Option<&'a str>; 2],
    /// Open visible windows on the active workspace in stable slot order.
    pub windows: &'a [DockWindowEntry],
    /// Currently hovered Dock slot index (`0..DOCK_PINNED_COUNT + windows.len() + DOCK_UTILITY_COUNT`), if any.
    pub hover_slot: Option<usize>,
    /// Whether Workspace Overview is active (highlights Dock Overview squircle tile).
    pub overview_active: bool,
    /// Smooth 120 ms hover lift factor (`0.0..=1.0`) (`RUSTYOS-048`).
    pub hover_progress: f64,
    /// Prefer instant subtle lift without animated spring (`RUSTYOS-048`).
    pub reduced_motion: bool,
}

/// Interactive click target inside the bottom floating Dock (`RUSTYOS-023` / `RUSTYOS-050`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockHit {
    /// Pinned Files launcher (`Files`).
    PinnedFiles,
    /// Workspace Overview / App Grid toggle (`Overview`).
    ToggleOverview,
    /// Pinned Terminal launcher (`Terminal`).
    PinnedTerminal,
    /// Pinned Browser launcher (`Browser`).
    PinnedBrowser,
    /// Pinned Notes launcher (`Notes`).
    PinnedNotes,
    /// Pinned System Settings launcher (`Settings`).
    PinnedSettings,
    /// Running window tile at `index` inside `ordered_visible_windows`.
    WindowSlot(usize),
    /// Utility folder shortcut (`Downloads`).
    PinnedDownloads,
    /// Utility wastebasket shortcut (`Trash`).
    PinnedTrash,
}

#[must_use]
pub(super) fn dock_layout_bounds(
    width: i32,
    height: i32,
    window_count: usize,
) -> (i32, i32, i32, usize) {
    let items_count = DOCK_PINNED_COUNT + window_count + DOCK_UTILITY_COUNT;
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let count_i32 = items_count as i32;
    let divider_extra = DOCK_DIVIDER_GAP;
    let raw_width = count_i32 * DOCK_ITEM_SIZE
        + (count_i32 - 1) * DOCK_ITEM_GAP
        + DOCK_PADDING * 2
        + divider_extra;
    let dock_width = raw_width.min((width - 48).max(160));
    let dock_x = (width - dock_width) / 2;
    let dock_y = height - DOCK_HEIGHT - DOCK_MARGIN_BOTTOM;
    (dock_x, dock_y, dock_width, items_count)
}

#[must_use]
pub(super) fn dock_slot_rect(
    width: i32,
    height: i32,
    window_count: usize,
    slot_idx: usize,
) -> Option<Rectangle<i32, smithay::utils::Physical>> {
    let (dock_x, dock_y, dock_width, items_count) = dock_layout_bounds(width, height, window_count);
    if slot_idx >= items_count {
        return None;
    }
    let left_count = DOCK_PINNED_COUNT + window_count;
    let item_size = DOCK_ITEM_SIZE;
    let slot_i32 = i32::try_from(slot_idx).ok()?;
    let divider_shift = if slot_idx >= left_count {
        DOCK_DIVIDER_GAP
    } else {
        0
    };
    let item_x = dock_x + DOCK_PADDING + slot_i32 * (item_size + DOCK_ITEM_GAP) + divider_shift;
    if item_x + item_size > dock_x + dock_width {
        return None;
    }
    Some(Rectangle::new(
        Point::from((item_x, dock_y + DOCK_PADDING)),
        Size::from((item_size, item_size)),
    ))
}

#[must_use]
pub(super) fn dock_tooltip_label(dock_view: &DockViewState<'_>, slot_idx: usize) -> Option<String> {
    let left_count = DOCK_PINNED_COUNT + dock_view.windows.len();
    match slot_idx {
        0 => Some("Files".to_string()),
        1 => Some("Overview".to_string()),
        2 => Some("Terminal".to_string()),
        3 => Some("Browser".to_string()),
        4 => Some("Notes".to_string()),
        5 => Some("Settings".to_string()),
        n if n < left_count => dock_view
            .windows
            .get(n - DOCK_PINNED_COUNT)
            .map(|w| truncate_ascii(&w.title, 26)),
        n if n == left_count => Some("Downloads".to_string()),
        n if n == left_count + 1 => Some("Trash".to_string()),
        _ => None,
    }
}

#[must_use]
pub(super) fn dock_tooltip_rect_and_label(
    width: i32,
    height: i32,
    dock_view: &DockViewState<'_>,
) -> Option<(Rectangle<i32, smithay::utils::Physical>, String)> {
    let slot_idx = dock_view.hover_slot?;
    let slot = dock_slot_rect(width, height, dock_view.windows.len(), slot_idx)?;
    let label = dock_tooltip_label(dock_view, slot_idx)?;
    let text_w = crate::text::measure_ui_text_width(&label, 12.0, crate::text::FontWeight::Bold);
    let pill_w = (text_w + 22).clamp(48, 220);
    let pill_h = 26;
    let slot_center_x = slot.loc.x + slot.size.w / 2;
    let pill_x = (slot_center_x - pill_w / 2).clamp(12, (width - pill_w - 12).max(12));
    let pill_y = (height - DOCK_HEIGHT - DOCK_MARGIN_BOTTOM - pill_h - 14).max(TOP_BAR_HEIGHT + 8);
    Some((
        Rectangle::new(Point::from((pill_x, pill_y)), Size::from((pill_w, pill_h))),
        label,
    ))
}

/// Hit-tests `(x, y)` against Dock slots and returns the hovered/clicked slot index (`RUSTYOS-023`).
#[must_use]
pub fn check_dock_slot(
    screen_size: Size<i32, smithay::utils::Physical>,
    window_count: usize,
    x: f64,
    y: f64,
) -> Option<usize> {
    let items_count = DOCK_PINNED_COUNT + window_count + DOCK_UTILITY_COUNT;
    for idx in 0..items_count {
        if let Some(rect) = dock_slot_rect(screen_size.w, screen_size.h, window_count, idx) {
            let rx = f64::from(rect.loc.x);
            let ry = f64::from(rect.loc.y);
            let rw = f64::from(rect.size.w);
            let rh = f64::from(rect.size.h);
            if x >= rx && x < rx + rw && y >= ry && y < ry + rh {
                return Some(idx);
            }
        }
    }
    None
}

/// Hit-tests a pointer click at `(x, y)` against the bottom Dock (`RUSTYOS-023` / `RUSTYOS-050`).
#[must_use]
pub fn check_dock_click(
    screen_size: Size<i32, smithay::utils::Physical>,
    window_count: usize,
    x: f64,
    y: f64,
) -> Option<DockHit> {
    let slot = check_dock_slot(screen_size, window_count, x, y)?;
    let left_count = DOCK_PINNED_COUNT + window_count;
    match slot {
        0 => Some(DockHit::PinnedFiles),
        1 => Some(DockHit::ToggleOverview),
        2 => Some(DockHit::PinnedTerminal),
        3 => Some(DockHit::PinnedBrowser),
        4 => Some(DockHit::PinnedNotes),
        5 => Some(DockHit::PinnedSettings),
        n if n < left_count => Some(DockHit::WindowSlot(n - DOCK_PINNED_COUNT)),
        n if n == left_count => Some(DockHit::PinnedDownloads),
        n if n == left_count + 1 => Some(DockHit::PinnedTrash),
        _ => None,
    }
}

pub(super) fn render_dock(
    elements: &mut Vec<SolidColorRenderElement>,
    width: i32,
    height: i32,
    window_count: usize,
    has_focused_window: bool,
) {
    // 5. Dock Background (floating centered pill at the bottom with outer rim)
    let (dock_x, dock_y, dock_width, dock_items_count) =
        dock_layout_bounds(width, height, window_count);
    let left_count = DOCK_PINNED_COUNT + window_count;

    // 5a. Outer glass highlight rim (1px around dock)
    let dock_rim_geo = Rectangle::new(
        Point::from((dock_x - 1, dock_y - 1)),
        Size::from((dock_width + 2, DOCK_HEIGHT + 2)),
    );
    elements.push(make_solid_element(dock_rim_geo, COLOR_BORDER_HIGHLIGHT));

    // 5b. Dock body
    let dock_geo = Rectangle::new(
        Point::from((dock_x, dock_y)),
        Size::from((dock_width, DOCK_HEIGHT)),
    );
    elements.push(make_solid_element(dock_geo, COLOR_SURFACE_ELEVATED));

    // 6. Dock Item slots
    for i in 0..dock_items_count {
        let Some(slot_rect) = dock_slot_rect(width, height, window_count, i) else {
            continue;
        };
        let is_running_window = i >= DOCK_PINNED_COUNT && i < left_count;
        let is_focused = is_running_window && has_focused_window;

        let inner_size = 40;
        let inner_offset = (slot_rect.size.w - inner_size) / 2;
        let inner_geo = Rectangle::new(
            Point::from((
                slot_rect.loc.x + inner_offset,
                slot_rect.loc.y + inner_offset - 2,
            )),
            Size::from((inner_size, inner_size)),
        );
        let inner_color = match i {
            0 => COLOR_ACCENT_MUTED, // Files
            1 => COLOR_SUBTLE,       // Overview
            2 => COLOR_ACCENT,       // Terminal
            3..=5 => COLOR_ACCENT_HOVER,
            _ if is_focused => COLOR_ACCENT_HOVER,
            _ => COLOR_MUTED,
        };
        elements.push(make_solid_element(inner_geo, inner_color));

        // Active running indicator pip under Files / running windows
        if is_running_window || i == 0 {
            let pip_geo = Rectangle::new(
                Point::from((
                    slot_rect.loc.x + (slot_rect.size.w - 4) / 2,
                    dock_y + DOCK_HEIGHT - 7,
                )),
                Size::from((4, 4)),
            );
            elements.push(make_solid_element(pip_geo, COLOR_ACCENT));
        }
    }
}
