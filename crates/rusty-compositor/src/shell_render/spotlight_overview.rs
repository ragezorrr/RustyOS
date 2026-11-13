//! Workspace Overview, Spotlight launcher, and `Alt+Tab` window switcher rendering & hit-testing.

use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
use smithay::backend::renderer::element::solid::SolidColorRenderElement;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::{Point, Rectangle, Size};

use super::dock::DockWindowEntry;
use super::tokens::{
    COLOR_ACCENT, COLOR_ACCENT_MUTED, COLOR_BORDER, COLOR_BORDER_HIGHLIGHT, COLOR_MUTED,
    COLOR_SUBTLE, COLOR_SURFACE, COLOR_SURFACE_ELEVATED, COLOR_SURFACE_OVERLAY, COLOR_TEXT,
    DOCK_HEIGHT, DOCK_MARGIN_BOTTOM, TOP_BAR_HEIGHT, anim_offset, make_solid_element,
    rect_contains, truncate_ascii,
};

/// Miniature workspace summary rendered in the top Spaces strip inside Overview (`RUSTYOS-051`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OverviewWorkspaceSpace {
    /// Workspace number (`1..=9`).
    pub id: u8,
    /// Total windows assigned to this workspace.
    pub window_count: usize,
    /// True when this is the currently active workspace.
    pub active: bool,
    /// Normalized `(x, y, w, h)` mini-window rectangles within `0.0..=1.0` (`w == 0` marks unused slot).
    pub preview_rects: [(f32, f32, f32, f32); 4],
}

/// Live data for the Workspace Overview / Expose card grid (`RUSTYOS-023`).
#[derive(Debug, Clone)]
pub struct OverviewRenderState<'a> {
    /// Active workspace number (`1..=9`).
    pub active_workspace: u8,
    /// Active layout mode string (`"floating"` or `"master-stack"`).
    pub layout_mode: &'a str,
    /// Open visible windows on the active workspace in stable order.
    pub windows: &'a [DockWindowEntry],
    /// Miniature workspace strip summaries shown at the top of Overview (`RUSTYOS-051`).
    pub spaces: &'a [OverviewWorkspaceSpace],
    /// Animated reveal progress (`0.0` closed -> `1.0` open) (`RUSTYOS-025`).
    pub progress: f64,
}

/// Interactive click target while Workspace Overview is open (`RUSTYOS-023` / `RUSTYOS-051`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverviewHit {
    /// Focus and raise the window at card index `usize`.
    Card(usize),
    /// Switch to workspace `u8` (`1..=9`) from the top Spaces strip (`RUSTYOS-051`).
    SwitchWorkspace(u8),
    /// Dismiss Overview without changing window focus.
    Dismiss,
}

/// Single window card inside the `Alt+Tab` window switcher HUD (`RUSTYOS-033`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowSwitcherCard<'a> {
    /// Window title displayed at card bottom.
    pub title: &'a str,
    /// Fallback 1-2 char monogram if no PNG icon resolves.
    pub badge: &'a str,
    /// Managed window identifier used as the GPU SHM icon cache key (`RUSTYOS-047`).
    pub window_id: rusty_wm::WindowId,
    /// Optional resolved XDG icon name / PNG path.
    pub icon_name: Option<&'a str>,
    /// Optional client-provided `xdg_toplevel_icon_v1` icon (`RUSTYOS-047`).
    pub toplevel_icon: Option<&'a crate::icons::ShmToplevelIcon>,
    /// True if minimized to the Dock.
    pub minimized: bool,
}

/// Active `Alt+Tab` window switcher HUD render state (`RUSTYOS-033`).
#[derive(Debug, Clone, Copy)]
pub struct WindowSwitcherRenderState<'a> {
    /// Candidate window cards in presentation order.
    pub cards: &'a [WindowSwitcherCard<'a>],
    /// Currently highlighted card index.
    pub selected: usize,
}

/// Computes centered top Spaces miniature strip rectangles for `count` workspaces (`RUSTYOS-051`).
#[must_use]
pub fn overview_space_rects(
    screen_size: Size<i32, smithay::utils::Physical>,
    count: usize,
) -> Vec<Rectangle<i32, smithay::utils::Physical>> {
    if count == 0 {
        return Vec::new();
    }
    let visible = count.min(9);
    let width = screen_size.w.max(320);
    let count_i32 = i32::try_from(visible).unwrap_or(1);
    let gap = 10;
    let avail_w = (width - 64).max(180);
    let thumb_w = ((avail_w - gap * (count_i32 - 1)) / count_i32).clamp(52, 108);
    let thumb_h = ((thumb_w * 58) / 100).clamp(34, 62);
    let total_w = count_i32 * thumb_w + (count_i32 - 1) * gap;
    let start_x = (width - total_w) / 2;
    let start_y = TOP_BAR_HEIGHT + 16;

    (0..visible)
        .map(|idx| {
            let sx = start_x + i32::try_from(idx).unwrap_or(0) * (thumb_w + gap);
            Rectangle::new(Point::from((sx, start_y)), Size::from((thumb_w, thumb_h)))
        })
        .collect()
}

/// Computes centered Workspace Overview preview card rectangles for `count` windows (`RUSTYOS-023`).
#[must_use]
pub fn overview_card_rects(
    screen_size: Size<i32, smithay::utils::Physical>,
    count: usize,
) -> Vec<Rectangle<i32, smithay::utils::Physical>> {
    if count == 0 {
        return Vec::new();
    }
    let width = screen_size.w.max(320);
    let height = screen_size.h.max(240);
    let area_top = TOP_BAR_HEIGHT + 108;
    let area_bottom = (height - DOCK_HEIGHT - DOCK_MARGIN_BOTTOM - 22).max(area_top + 120);
    let avail_w = (width - 64).max(200);
    let avail_h = (area_bottom - area_top).max(120);

    let cols = match count {
        1 => 1,
        2..=4 => 2,
        _ => 3,
    };
    let rows = count.div_ceil(cols);
    let gap = 18;
    let cols_i32 = i32::try_from(cols).unwrap_or(1);
    let rows_i32 = i32::try_from(rows).unwrap_or(1);

    let card_w = ((avail_w - gap * (cols_i32 - 1)) / cols_i32).clamp(140, 340);
    let card_h = ((avail_h - gap * (rows_i32 - 1)) / rows_i32).clamp(96, 210);

    let total_grid_h = rows_i32 * card_h + (rows_i32 - 1) * gap;
    let start_y = area_top + ((avail_h - total_grid_h) / 2).max(0);

    let mut rects = Vec::with_capacity(count);
    for idx in 0..count {
        let row = idx / cols;
        let col = idx % cols;
        let items_in_row = if row + 1 == rows {
            count - row * cols
        } else {
            cols
        };
        let row_cols_i32 = i32::try_from(items_in_row).unwrap_or(1);
        let row_w = row_cols_i32 * card_w + (row_cols_i32 - 1) * gap;
        let row_start_x = (width - row_w) / 2;

        let cx = row_start_x + i32::try_from(col).unwrap_or(0) * (card_w + gap);
        let cy = start_y + i32::try_from(row).unwrap_or(0) * (card_h + gap);
        rects.push(Rectangle::new(
            Point::from((cx, cy)),
            Size::from((card_w, card_h)),
        ));
    }
    rects
}

/// Hit-tests `(x, y)` inside Workspace Overview, including Spaces strip and window cards (`RUSTYOS-023` / `RUSTYOS-051`).
#[must_use]
pub fn check_overview_click_with_spaces(
    screen_size: Size<i32, smithay::utils::Physical>,
    spaces: &[OverviewWorkspaceSpace],
    window_count: usize,
    x: f64,
    y: f64,
) -> OverviewHit {
    if !spaces.is_empty() {
        let space_rects = overview_space_rects(screen_size, spaces.len());
        for (space, rect) in spaces.iter().zip(space_rects) {
            let hit_rect = Rectangle::new(rect.loc, Size::from((rect.size.w, rect.size.h + 18)));
            if rect_contains(hit_rect, x, y) {
                return OverviewHit::SwitchWorkspace(space.id);
            }
        }
    }
    let cards = overview_card_rects(screen_size, window_count);
    for (idx, rect) in cards.into_iter().enumerate() {
        if rect_contains(rect, x, y) {
            return OverviewHit::Card(idx);
        }
    }
    OverviewHit::Dismiss
}

/// Hit-tests `(x, y)` against Workspace Overview window cards (`RUSTYOS-023`).
#[must_use]
pub fn check_overview_click(
    screen_size: Size<i32, smithay::utils::Physical>,
    window_count: usize,
    x: f64,
    y: f64,
) -> OverviewHit {
    check_overview_click_with_spaces(screen_size, &[], window_count, x, y)
}

/// Computes `(panel_rect, card_rects)` for the centered `Alt+Tab` window switcher HUD (`RUSTYOS-033`).
#[must_use]
pub fn switcher_layout(
    screen_size: Size<i32, smithay::utils::Physical>,
    count: usize,
) -> (
    Rectangle<i32, smithay::utils::Physical>,
    Vec<Rectangle<i32, smithay::utils::Physical>>,
) {
    let width = screen_size.w.max(320);
    let height = screen_size.h.max(240);
    if count == 0 {
        let rect = Rectangle::new(Point::from((width / 2, height / 2)), Size::from((0, 0)));
        return (rect, Vec::new());
    }

    let pad = 16;
    let gap = 12;
    let count_i32 = i32::try_from(count).unwrap_or(1);
    let max_panel_w = (width - 48).max(200);
    let raw_card_w = (max_panel_w - pad * 2 - gap * (count_i32 - 1)) / count_i32;
    let card_w = raw_card_w.clamp(76, 128);
    let card_h = 104;
    let panel_w = (pad * 2 + count_i32 * card_w + (count_i32 - 1) * gap).min(max_panel_w);
    let panel_h = card_h + pad * 2;
    let panel_x = (width - panel_w) / 2;
    let panel_y = ((height - panel_h) / 2).max(TOP_BAR_HEIGHT + 16);

    let mut cards = Vec::with_capacity(count);
    for idx in 0..count {
        let i = i32::try_from(idx).unwrap_or(0);
        let cx = panel_x + pad + i * (card_w + gap);
        if cx + card_w > panel_x + panel_w - pad + 2 {
            break;
        }
        cards.push(Rectangle::new(
            Point::from((cx, panel_y + pad)),
            Size::from((card_w, card_h)),
        ));
    }

    (
        Rectangle::new(
            Point::from((panel_x, panel_y)),
            Size::from((panel_w, panel_h)),
        ),
        cards,
    )
}

/// Hit-tests `(x, y)` against visible `Alt+Tab` window switcher cards (`RUSTYOS-033`).
#[must_use]
pub fn check_switcher_click(
    screen_size: Size<i32, smithay::utils::Physical>,
    count: usize,
    x: f64,
    y: f64,
) -> Option<usize> {
    let (_panel, cards) = switcher_layout(screen_size, count);
    for (idx, rect) in cards.into_iter().enumerate() {
        let rx = f64::from(rect.loc.x);
        let ry = f64::from(rect.loc.y);
        let rw = f64::from(rect.size.w);
        let rh = f64::from(rect.size.h);
        if x >= rx && x < rx + rw && y >= ry && y < ry + rh {
            return Some(idx);
        }
    }
    None
}

/// Live Spotlight view state passed into shell rendering (`RUSTYOS-018b`).
#[derive(Debug, Clone)]
pub struct SpotlightRenderState<'a> {
    /// Current search string entered by the user.
    pub query: &'a str,
    /// Highlighted row offset within `items` (`0..3`).
    pub selected_in_window: usize,
    /// Horizontal offset of the cursor relative to search text origin.
    pub cursor_offset_x: i32,
    /// Up to 3 visible filtered candidates to render.
    pub items: &'a [crate::launcher::LauncherCandidate],
    /// Animated reveal progress (`0.0` closed -> `1.0` open) (`RUSTYOS-025`).
    pub progress: f64,
}

/// Interactive click target inside or outside the Spotlight modal (`RUSTYOS-018b`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpotlightHit {
    /// Clicked one of the visible result rows (`0..3`).
    Row(usize),
    /// Clicked inside the modal frame/search box (keep Spotlight open).
    InsideModal,
    /// Clicked outside the modal (dismiss Spotlight).
    OutsideModal,
}

#[must_use]
pub(super) fn spotlight_modal_rect(width: i32, height: i32) -> (i32, i32, i32, i32) {
    let modal_width = 600.min(width - 48).max(320);
    let modal_height = 350.min(height - 100).max(180);
    let modal_x = (width - modal_width) / 2;
    let modal_y = (height - modal_height) / 4;
    (modal_x, modal_y, modal_width, modal_height)
}

/// Hit-tests `(x, y)` against the open Spotlight modal and its up to 5 visible rows (`RUSTYOS-018b`, `RUSTYOS-039`).
#[must_use]
pub fn check_spotlight_click(
    screen_size: Size<i32, smithay::utils::Physical>,
    x: f64,
    y: f64,
) -> SpotlightHit {
    let (modal_x, modal_y, modal_width, modal_height) =
        spotlight_modal_rect(screen_size.w, screen_size.h);
    let mx = f64::from(modal_x);
    let my = f64::from(modal_y);
    let mw = f64::from(modal_width);
    let mh = f64::from(modal_height);

    if x < mx || x >= mx + mw || y < my || y >= my + mh {
        return SpotlightHit::OutsideModal;
    }

    let row_h = 46;
    let start_row_y = modal_y + 80;
    let row_x_min = f64::from(modal_x + 16);
    let row_x_max = f64::from(modal_x + modal_width - 16);
    if x >= row_x_min && x < row_x_max {
        for i in 0..5_i32 {
            let ry = f64::from(start_row_y + i * (row_h + 4));
            if y >= ry && y < ry + f64::from(row_h) {
                return SpotlightHit::Row(usize::try_from(i).unwrap_or(0));
            }
        }
    }

    SpotlightHit::InsideModal
}

pub(super) fn render_spotlight_modal(
    elements: &mut Vec<SolidColorRenderElement>,
    width: i32,
    height: i32,
    spotlight: &SpotlightRenderState<'_>,
    has_text: bool,
) {
    let (modal_x, modal_y, modal_width, modal_height) = spotlight_modal_rect(width, height);

    // 7. Spotlight Glass Box - Outer glass highlight rim
    let rim_geo = Rectangle::new(
        Point::from((modal_x - 1, modal_y - 1)),
        Size::from((modal_width + 2, modal_height + 2)),
    );
    elements.push(make_solid_element(rim_geo, COLOR_BORDER_HIGHLIGHT));

    // 7a. Modal elevated body
    let modal_geo = Rectangle::new(
        Point::from((modal_x, modal_y)),
        Size::from((modal_width, modal_height)),
    );
    elements.push(make_solid_element(modal_geo, COLOR_SURFACE_ELEVATED));

    // 7b. Search input bar plate (macOS 27 Liquid Glass style, RUSTYOS-039)
    let search_bar_h = 52;
    let search_bar_geo = Rectangle::new(
        Point::from((modal_x + 16, modal_y + 14)),
        Size::from((modal_width - 32, search_bar_h)),
    );
    elements.push(make_solid_element(search_bar_geo, COLOR_SURFACE_OVERLAY));

    // 7c. Search bar accent indicator
    let search_icon_geo = Rectangle::new(
        Point::from((modal_x + 28, modal_y + 28)),
        Size::from((24, 24)),
    );
    elements.push(make_solid_element(search_icon_geo, COLOR_ACCENT));

    // 7d. Dynamic text input cursor placed after typed query
    let cursor_x = (modal_x + 60 + spotlight.cursor_offset_x).min(modal_x + modal_width - 28);
    let cursor_geo = Rectangle::new(Point::from((cursor_x, modal_y + 28)), Size::from((2, 24)));
    elements.push(make_solid_element(cursor_geo, COLOR_TEXT));

    if !has_text {
        // 7e. Placeholder text indicator when no font renderer is active
        let placeholder_geo = Rectangle::new(
            Point::from((modal_x + 64, modal_y + 36)),
            Size::from((160, 8)),
        );
        elements.push(make_solid_element(placeholder_geo, COLOR_MUTED));
    }

    // 7f. Search divider
    let divider_geo = Rectangle::new(
        Point::from((modal_x + 16, modal_y + 72)),
        Size::from((modal_width - 32, 1)),
    );
    elements.push(make_solid_element(divider_geo, COLOR_BORDER));

    // 7g. Search results (up to 5 visible matching applications/commands)
    let row_h = 46;
    let start_row_y = modal_y + 80;
    let row_count = spotlight.items.len().min(5);
    for idx in 0..row_count {
        let i = i32::try_from(idx).unwrap_or(0);
        let row_y = start_row_y + i * (row_h + 4);
        let is_selected = idx == spotlight.selected_in_window;

        let row_bg = Rectangle::new(
            Point::from((modal_x + 16, row_y)),
            Size::from((modal_width - 32, row_h)),
        );
        let bg_color = if is_selected {
            COLOR_ACCENT_MUTED
        } else {
            COLOR_SURFACE
        };
        elements.push(make_solid_element(row_bg, bg_color));

        if is_selected {
            let select_indicator = Rectangle::new(
                Point::from((modal_x + 18, row_y + 8)),
                Size::from((3, row_h - 16)),
            );
            elements.push(make_solid_element(select_indicator, COLOR_ACCENT));
        }

        // App icon badge
        let app_icon = Rectangle::new(Point::from((modal_x + 28, row_y + 9)), Size::from((28, 28)));
        let icon_color = if is_selected {
            COLOR_ACCENT
        } else if idx == 1 {
            COLOR_ACCENT_MUTED
        } else {
            COLOR_SUBTLE
        };
        elements.push(make_solid_element(app_icon, icon_color));

        if !has_text {
            // App title bar fallback
            let title_bar = Rectangle::new(
                Point::from((modal_x + 66, row_y + 12)),
                Size::from((120 + i * 20, 8)),
            );
            elements.push(make_solid_element(title_bar, COLOR_TEXT));

            // App subtitle bar fallback
            let sub_bar = Rectangle::new(
                Point::from((modal_x + 66, row_y + 26)),
                Size::from((80 + i * 15, 6)),
            );
            elements.push(make_solid_element(sub_bar, COLOR_MUTED));
        }
    }
}

pub(super) fn render_switcher_text(
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    sw: &WindowSwitcherRenderState<'_>,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let text_bright = [244, 240, 250, 255];
    let text_muted = [166, 155, 181, 255];
    let (_panel, cards) = switcher_layout(screen_size, sw.cards.len());
    for (idx, (card, rect)) in sw.cards.iter().zip(cards).enumerate() {
        let is_sel = idx == sw.selected;
        let sw_key = format!("win-{}", card.window_id.0);
        let has_icon = shape_cache.icons.has_window_icon(
            renderer,
            &sw_key,
            card.toplevel_icon,
            card.icon_name,
            36,
        );
        if !has_icon {
            let icon_box = Rectangle::new(
                Point::from((rect.loc.x + (rect.size.w - 44) / 2, rect.loc.y + 14)),
                Size::from((44, 44)),
            );
            if let Some(el) =
                text_cache.render_label_centered(renderer, card.badge, 14.5, text_bright, icon_box)
            {
                elements.push(el);
            }
        }
        let raw_title = if card.minimized {
            format!("Min: {}", card.title)
        } else {
            card.title.to_owned()
        };
        let title_str = truncate_ascii(&raw_title, 13);
        let title_bounds = Rectangle::new(
            Point::from((rect.loc.x + 6, rect.loc.y + rect.size.h - 30)),
            Size::from((rect.size.w - 12, 22)),
        );
        if let Some(el) = text_cache.render_label_centered(
            renderer,
            &title_str,
            10.5,
            if is_sel { text_bright } else { text_muted },
            title_bounds,
        ) {
            elements.push(el);
        }
    }
}

pub(super) fn render_overview_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    ov: &OverviewRenderState<'_>,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let text_bright = [244, 240, 250, 255];
    let text_muted = [188, 178, 206, 255];
    let width = screen_size.w;
    let heading_dy = anim_offset(ov.progress, 12.0);

    // Top Spaces miniature captions ("Desktop 1", "Desktop 2", ...) matching Mission Control (`RUSTYOS-051`)
    if !ov.spaces.is_empty() {
        let space_rects = overview_space_rects(screen_size, ov.spaces.len());
        for (space, rect) in ov.spaces.iter().zip(space_rects) {
            let label = format!("Desktop {}", space.id);
            let label_bounds = Rectangle::new(
                Point::from((rect.loc.x - 4, rect.loc.y + rect.size.h + 4 + heading_dy)),
                Size::from((rect.size.w + 8, 16)),
            );
            let color = if space.active {
                text_bright
            } else {
                text_muted
            };
            if let Some(el) =
                text_cache.render_label_centered(renderer, &label, 10.5, color, label_bounds)
            {
                elements.push(el);
            }
        }
    }

    if ov.windows.is_empty() {
        let empty_msg = format!("Desktop {} is empty", ov.active_workspace);
        if let Some(el) = text_cache.render_label_centered(
            renderer,
            &empty_msg,
            13.5,
            text_muted,
            Rectangle::new(
                Point::from((32, screen_size.h / 2 - 14 + heading_dy)),
                Size::from((width - 64, 28)),
            ),
        ) {
            elements.push(el);
        }
        return;
    }

    let card_dy = anim_offset(ov.progress, 22.0);
    let cards = overview_card_rects(screen_size, ov.windows.len());
    for (win, card) in ov.windows.iter().zip(cards) {
        let label = truncate_ascii(&win.title, 26);
        let footer_bounds = Rectangle::new(
            Point::from((card.loc.x + 8, card.loc.y + card.size.h + 5 + card_dy)),
            Size::from((card.size.w - 16, 20)),
        );
        if let Some(el) = text_cache.render_label_centered(
            renderer,
            &label,
            11.5,
            if win.focused { text_bright } else { text_muted },
            footer_bounds,
        ) {
            elements.push(el);
        }
        let body_bounds = Rectangle::new(
            Point::from((card.loc.x + 12, card.loc.y + 12 + card_dy)),
            Size::from((card.size.w - 24, card.size.h - 24)),
        );
        let center_badge = format!("{}  •  {}", win.badge, win.dimensions);
        if let Some(el) = text_cache.render_label_centered(
            renderer,
            &center_badge,
            11.5,
            if win.focused { text_bright } else { text_muted },
            body_bounds,
        ) {
            elements.push(el);
        }
    }
}
