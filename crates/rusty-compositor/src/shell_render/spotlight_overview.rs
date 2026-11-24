//! Workspace Overview, Spotlight launcher, and `Alt+Tab` window switcher rendering & hit-testing.

use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
use smithay::backend::renderer::element::solid::SolidColorRenderElement;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::{Point, Rectangle, Size};

use super::dock::DockWindowEntry;
use super::tokens::{
    COLOR_ACCENT, COLOR_ACCENT_MUTED, COLOR_BORDER_HIGHLIGHT, COLOR_MUTED, COLOR_SUBTLE,
    COLOR_SURFACE_ELEVATED, COLOR_TEXT, DOCK_HEIGHT, DOCK_MARGIN_BOTTOM, TOP_BAR_HEIGHT,
    anim_offset, make_solid_element, rect_contains, truncate_ascii,
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

/// Live data for the Workspace Overview / Expose card grid (`RUSTYOS-023`, `RUSTYOS-167`).
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
    /// Optional App Exposé filter banner (e.g. `"App Expose · Files"`) (`RUSTYOS-073`).
    pub filter_label: Option<&'a str>,
    /// Card index currently hovered by the pointer (`RUSTYOS-167`).
    pub hovered_card: Option<usize>,
    /// Card index whose close (`×`) button is currently hovered (`RUSTYOS-167`).
    pub hovered_close: Option<usize>,
    /// Workspace number (`1..=9`) currently hovered in the top Spaces strip (`RUSTYOS-167`).
    pub hovered_space: Option<u8>,
    /// Keyboard-selected card index inside Overview (`RUSTYOS-167`).
    pub selected_card: Option<usize>,
    /// Workspace number (`1..=9`) highlighted as drag-to-workspace drop target (`RUSTYOS-167`).
    pub drag_target_workspace: Option<u8>,
    /// Active dragged card `(card_index, pointer_offset)` when dragging a card onto Spaces (`RUSTYOS-167`).
    pub dragging_card: Option<(usize, Point<i32, smithay::utils::Physical>)>,
}

/// Interactive click target while Workspace Overview is open (`RUSTYOS-023` / `RUSTYOS-051` / `RUSTYOS-167`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverviewHit {
    /// Focus and raise the window at card index `usize`.
    Card(usize),
    /// Close the window at card index `usize` via the card corner button (`RUSTYOS-167`).
    CloseCard(usize),
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

/// Interactive hit target inside the `Alt+Tab` window switcher HUD (`RUSTYOS-167`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitcherHit {
    /// Clicked or hovered on card `usize`.
    Card(usize),
    /// Clicked or hovered on the close (`×`) button of card `usize`.
    CloseCard(usize),
    /// Outside all switcher cards.
    Outside,
}

/// Active `Alt+Tab` window switcher HUD render state (`RUSTYOS-033`, `RUSTYOS-167`).
#[derive(Debug, Clone, Copy)]
pub struct WindowSwitcherRenderState<'a> {
    /// Candidate window cards in presentation order.
    pub cards: &'a [WindowSwitcherCard<'a>],
    /// Currently highlighted card index.
    pub selected: usize,
    /// Card index currently hovered by the pointer, if any (`RUSTYOS-167`).
    pub hovered: Option<usize>,
    /// Card index whose close (`×`) button is currently hovered, if any (`RUSTYOS-167`).
    pub hovered_close: Option<usize>,
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

/// Returns the number of grid columns used by Workspace Overview for `count` cards (`RUSTYOS-167`).
#[must_use]
pub const fn overview_grid_cols(count: usize) -> usize {
    match count {
        0 | 1 => 1,
        2..=4 => 2,
        _ => 3,
    }
}

/// Computes the top-right close (`×`) button rectangle for an Overview card (`RUSTYOS-167`).
#[must_use]
pub fn overview_card_close_rect(
    card: Rectangle<i32, smithay::utils::Physical>,
) -> Rectangle<i32, smithay::utils::Physical> {
    Rectangle::new(
        Point::from((card.loc.x + card.size.w - 28, card.loc.y + 6)),
        Size::from((22, 22)),
    )
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

    let cols = overview_grid_cols(count);
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

/// Hit-tests `(x, y)` inside Workspace Overview, including Spaces strip, close buttons, and window cards (`RUSTYOS-023` / `RUSTYOS-051` / `RUSTYOS-167`).
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
        let close_rect = overview_card_close_rect(rect);
        if rect_contains(close_rect, x, y) {
            return OverviewHit::CloseCard(idx);
        }
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

/// Computes `(panel_rect, card_rects)` for the centered `Alt+Tab` window switcher HUD (`RUSTYOS-033`, `RUSTYOS-167`).
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

    let pad = 18;
    let gap = 12;
    let count_i32 = i32::try_from(count).unwrap_or(1);
    let max_panel_w = (width - 48).max(220);
    let raw_card_w = (max_panel_w - pad * 2 - gap * (count_i32 - 1)) / count_i32;
    let card_w = raw_card_w.clamp(136, 208);
    let card_h = 160;
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

/// Computes the `16:10` live GPU thumbnail slot inside a switcher card (`RUSTYOS-167`).
#[must_use]
pub fn switcher_card_thumb_rect(
    card: Rectangle<i32, smithay::utils::Physical>,
) -> Rectangle<i32, smithay::utils::Physical> {
    Rectangle::new(
        Point::from((card.loc.x + 10, card.loc.y + 10)),
        Size::from(((card.size.w - 20).max(40), (card.size.h - 46).max(32))),
    )
}

/// Computes the top-right close (`×`) button hitbox inside a switcher card (`RUSTYOS-167`).
#[must_use]
pub fn switcher_card_close_rect(
    card: Rectangle<i32, smithay::utils::Physical>,
) -> Rectangle<i32, smithay::utils::Physical> {
    Rectangle::new(
        Point::from((card.loc.x + card.size.w - 26, card.loc.y + 6)),
        Size::from((20, 20)),
    )
}

/// Hit-tests `(x, y)` against visible `Alt+Tab` window switcher cards and close buttons (`RUSTYOS-167`).
#[must_use]
pub fn hit_test_switcher(
    screen_size: Size<i32, smithay::utils::Physical>,
    count: usize,
    x: f64,
    y: f64,
) -> SwitcherHit {
    let (_panel, cards) = switcher_layout(screen_size, count);
    for (idx, rect) in cards.into_iter().enumerate() {
        let close_rect = switcher_card_close_rect(rect);
        if rect_contains(close_rect, x, y) {
            return SwitcherHit::CloseCard(idx);
        }
        if rect_contains(rect, x, y) {
            return SwitcherHit::Card(idx);
        }
    }
    SwitcherHit::Outside
}

/// Hit-tests `(x, y)` against visible `Alt+Tab` window switcher cards (`RUSTYOS-033`).
#[must_use]
pub fn check_switcher_click(
    screen_size: Size<i32, smithay::utils::Physical>,
    count: usize,
    x: f64,
    y: f64,
) -> Option<usize> {
    match hit_test_switcher(screen_size, count, x, y) {
        SwitcherHit::Card(idx) | SwitcherHit::CloseCard(idx) => Some(idx),
        SwitcherHit::Outside => None,
    }
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

const SPOTLIGHT_SEARCH_HEIGHT: i32 = 54;
const SPOTLIGHT_CAPSULE_GAP: i32 = 10;
const SPOTLIGHT_RESULTS_ROW_START_OFFSET: i32 = 78;
const SPOTLIGHT_RESULTS_ROW_HEIGHT: i32 = 46;
const SPOTLIGHT_RESULTS_ROW_STRIDE: i32 = 50;

#[must_use]
pub(super) fn spotlight_modal_rect(width: i32, height: i32) -> (i32, i32, i32, i32) {
    let modal_width = 620.min(width - 48).max(320);
    let modal_height = 350.min(height - 100).max(180);
    let modal_x = (width - modal_width) / 2;
    let modal_y = (height - modal_height) / 4;
    (modal_x, modal_y, modal_width, modal_height)
}

#[must_use]
pub(super) fn spotlight_search_pill_rect(width: i32, height: i32) -> (i32, i32, i32, i32) {
    let (mx, my, mw, _) = spotlight_modal_rect(width, height);
    (mx, my, mw, SPOTLIGHT_SEARCH_HEIGHT)
}

#[must_use]
pub(super) fn spotlight_results_rect(width: i32, height: i32) -> (i32, i32, i32, i32) {
    let (mx, my, mw, mh) = spotlight_modal_rect(width, height);
    let ry = my + SPOTLIGHT_SEARCH_HEIGHT + SPOTLIGHT_CAPSULE_GAP;
    let rh = (mh - SPOTLIGHT_SEARCH_HEIGHT - SPOTLIGHT_CAPSULE_GAP).max(80);
    (mx, ry, mw, rh)
}

fn tuple_rect_contains(rect: (i32, i32, i32, i32), x: f64, y: f64) -> bool {
    rect_contains(
        Rectangle::new(Point::from((rect.0, rect.1)), Size::from((rect.2, rect.3))),
        x,
        y,
    )
}

/// Hit-tests `(x, y)` against the open dual-capsule Spotlight search bar, detached results card, and 5 rows (`RUSTYOS-018b`, `RUSTYOS-070`).
#[must_use]
pub fn check_spotlight_click(
    screen_size: Size<i32, smithay::utils::Physical>,
    x: f64,
    y: f64,
) -> SpotlightHit {
    let search_rect = spotlight_search_pill_rect(screen_size.w, screen_size.h);
    if tuple_rect_contains(search_rect, x, y) {
        return SpotlightHit::InsideModal;
    }

    let results_rect = spotlight_results_rect(screen_size.w, screen_size.h);
    if !tuple_rect_contains(results_rect, x, y) {
        return SpotlightHit::OutsideModal;
    }

    let (modal_x, modal_y, modal_width, _) = spotlight_modal_rect(screen_size.w, screen_size.h);
    let start_row_y = modal_y + SPOTLIGHT_RESULTS_ROW_START_OFFSET;
    let row_x_min = f64::from(modal_x + 12);
    let row_x_max = f64::from(modal_x + modal_width - 12);
    if x >= row_x_min && x < row_x_max {
        for i in 0..5_i32 {
            let ry = f64::from(start_row_y + i * SPOTLIGHT_RESULTS_ROW_STRIDE);
            if y >= ry && y < ry + f64::from(SPOTLIGHT_RESULTS_ROW_HEIGHT) {
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
    let (sx, sy, sw, sh) = spotlight_search_pill_rect(width, height);
    let (rx, ry, rw, rh) = spotlight_results_rect(width, height);

    elements.push(make_solid_element(
        Rectangle::new(Point::from((sx - 1, sy - 1)), Size::from((sw + 2, sh + 2))),
        COLOR_BORDER_HIGHLIGHT,
    ));
    elements.push(make_solid_element(
        Rectangle::new(Point::from((sx, sy)), Size::from((sw, sh))),
        COLOR_SURFACE_ELEVATED,
    ));
    elements.push(make_solid_element(
        Rectangle::new(Point::from((sx + 20, sy + 15)), Size::from((24, 24))),
        COLOR_ACCENT,
    ));
    let cursor_x = (sx + 50 + spotlight.cursor_offset_x).min(sx + sw - 28);
    elements.push(make_solid_element(
        Rectangle::new(Point::from((cursor_x, sy + 15)), Size::from((2, 24))),
        COLOR_TEXT,
    ));
    if !has_text {
        elements.push(make_solid_element(
            Rectangle::new(Point::from((sx + 54, sy + 23)), Size::from((160, 8))),
            COLOR_MUTED,
        ));
    }

    elements.push(make_solid_element(
        Rectangle::new(Point::from((rx - 1, ry - 1)), Size::from((rw + 2, rh + 2))),
        COLOR_BORDER_HIGHLIGHT,
    ));
    elements.push(make_solid_element(
        Rectangle::new(Point::from((rx, ry)), Size::from((rw, rh))),
        COLOR_SURFACE_ELEVATED,
    ));

    let row_h = SPOTLIGHT_RESULTS_ROW_HEIGHT;
    let start_row_y = sy + SPOTLIGHT_RESULTS_ROW_START_OFFSET;
    let row_count = spotlight.items.len().min(5);
    for idx in 0..row_count {
        let i = i32::try_from(idx).unwrap_or(0);
        let row_y = start_row_y + i * SPOTLIGHT_RESULTS_ROW_STRIDE;
        let is_selected = idx == spotlight.selected_in_window;

        let row_bg = Rectangle::new(Point::from((rx + 12, row_y)), Size::from((rw - 24, row_h)));
        if is_selected {
            elements.push(make_solid_element(row_bg, COLOR_ACCENT_MUTED));
        }
        if is_selected {
            let select_indicator = Rectangle::new(
                Point::from((rx + 14, row_y + 8)),
                Size::from((3, row_h - 16)),
            );
            elements.push(make_solid_element(select_indicator, COLOR_ACCENT));
        }

        let app_icon = Rectangle::new(Point::from((rx + 22, row_y + 9)), Size::from((28, 28)));
        let icon_color = if is_selected {
            COLOR_ACCENT
        } else if idx == 1 {
            COLOR_ACCENT_MUTED
        } else {
            COLOR_SUBTLE
        };
        elements.push(make_solid_element(app_icon, icon_color));

        if !has_text {
            let title_bar = Rectangle::new(
                Point::from((rx + 60, row_y + 12)),
                Size::from((120 + i * 20, 8)),
            );
            elements.push(make_solid_element(title_bar, COLOR_TEXT));

            let sub_bar = Rectangle::new(
                Point::from((rx + 60, row_y + 26)),
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
    let pal = super::tokens::ThemePalette::active();
    let text_bright = pal.popover_text_primary;
    let text_muted = pal.popover_text_muted;
    let (_panel, cards) = switcher_layout(screen_size, sw.cards.len());
    for (idx, (card, rect)) in sw.cards.iter().zip(cards).enumerate() {
        let is_sel = idx == sw.selected;
        let is_hov = sw.hovered == Some(idx);
        let is_close_hov = sw.hovered_close == Some(idx);
        if is_sel || is_hov {
            let close_rect = switcher_card_close_rect(rect);
            let close_rect_logical = Rectangle::new(
                Point::from((close_rect.loc.x, close_rect.loc.y)),
                Size::from((close_rect.size.w, close_rect.size.h)),
            );
            let close_fg = if is_close_hov {
                [255, 255, 255, 255]
            } else {
                pal.popover_text_primary
            };
            if let Some(el) =
                text_cache.render_label_centered(renderer, "x", 10.0, close_fg, close_rect_logical)
            {
                elements.push(el);
            }
        }

        let has_thumb = shape_cache.has_lifecycle_snapshot(card.window_id.0);
        let sw_key = format!("win-{}", card.window_id.0);
        if !has_thumb {
            let has_icon = shape_cache.icons.has_window_icon(
                renderer,
                &sw_key,
                card.toplevel_icon,
                card.icon_name,
                44,
            );
            if !has_icon {
                let thumb = switcher_card_thumb_rect(rect);
                let icon_box = Rectangle::new(
                    Point::from((
                        thumb.loc.x + (thumb.size.w - 44) / 2,
                        thumb.loc.y + (thumb.size.h - 44) / 2,
                    )),
                    Size::from((44, 44)),
                );
                let badge_fg = if is_sel {
                    [255, 255, 255, 255]
                } else {
                    pal.popover_text_primary
                };
                if let Some(el) =
                    text_cache.render_label_centered(renderer, card.badge, 15.0, badge_fg, icon_box)
                {
                    elements.push(el);
                }
            }
        }
        let has_badge_icon = shape_cache.icons.has_window_icon(
            renderer,
            &sw_key,
            card.toplevel_icon,
            card.icon_name,
            20,
        );
        let raw_title = if card.minimized {
            format!("Min: {}", card.title)
        } else {
            card.title.to_owned()
        };
        let title_x = if has_badge_icon {
            rect.loc.x + 36
        } else {
            rect.loc.x + 10
        };
        let title_w = (rect.loc.x + rect.size.w - 10 - title_x).max(40);
        let max_chars = usize::try_from((title_w / 7).clamp(8, 26)).unwrap_or(14);
        let title_str = truncate_ascii(&raw_title, max_chars);
        let title_bounds = Rectangle::new(
            Point::from((title_x, rect.loc.y + rect.size.h - 29)),
            Size::from((title_w, 20)),
        );
        if let Some(el) = text_cache.render_label(
            renderer,
            &title_str,
            11.0,
            if is_sel || is_hov {
                text_bright
            } else {
                text_muted
            },
            title_bounds.loc,
        ) {
            elements.push(el);
        }
    }
}

#[allow(clippy::too_many_lines)]
pub(super) fn render_overview_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    ov: &OverviewRenderState<'_>,
    has_live_thumb: impl Fn(u64) -> bool,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let text_bright = [242, 242, 247, 255];
    let text_muted = [180, 180, 190, 255];
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

    if let Some(filter) = ov.filter_label
        && !filter.is_empty()
    {
        let pill_bounds = Rectangle::new(
            Point::from((32, super::tokens::TOP_BAR_HEIGHT + 86 + heading_dy)),
            Size::from((width - 64, 24)),
        );
        if let Some(el) = text_cache.render_label_centered_weighted(
            renderer,
            filter,
            crate::text::FontWeight::Bold,
            13.0,
            text_bright,
            pill_bounds,
        ) {
            elements.push(el);
        }
    }

    if ov.windows.is_empty() {
        let empty_msg = ov.filter_label.map_or_else(
            || format!("Desktop {} is empty", ov.active_workspace),
            |label| format!("No open windows for {label}"),
        );
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
    for (idx, (win, base_card)) in ov.windows.iter().zip(cards).enumerate() {
        let card = if let Some((drag_idx, delta)) = ov.dragging_card
            && drag_idx == idx
        {
            Rectangle::new(
                Point::from((base_card.loc.x + delta.x, base_card.loc.y + delta.y)),
                base_card.size,
            )
        } else {
            base_card
        };
        let is_hov = ov.hovered_card == Some(idx);
        let is_sel = ov.selected_card == Some(idx);
        if (is_hov || is_sel) && ov.dragging_card.is_none() {
            let close_rect = overview_card_close_rect(Rectangle::new(
                Point::from((card.loc.x, card.loc.y + card_dy)),
                card.size,
            ));
            let close_rect_logical = Rectangle::new(
                Point::from((close_rect.loc.x, close_rect.loc.y)),
                Size::from((close_rect.size.w, close_rect.size.h)),
            );
            if let Some(el) = text_cache.render_label_centered(
                renderer,
                "x",
                10.5,
                [255, 255, 255, 255],
                close_rect_logical,
            ) {
                elements.push(el);
            }
        }
        let label = truncate_ascii(&win.title, 26);
        let footer_bounds = Rectangle::new(
            Point::from((card.loc.x + 8, card.loc.y + card.size.h + 5 + card_dy)),
            Size::from((card.size.w - 16, 20)),
        );
        if let Some(el) = text_cache.render_label_centered(
            renderer,
            &label,
            11.5,
            if win.focused || is_hov || is_sel {
                text_bright
            } else {
                text_muted
            },
            footer_bounds,
        ) {
            elements.push(el);
        }
        if !has_live_thumb(win.id.0) {
            let body_bounds = Rectangle::new(
                Point::from((card.loc.x + 12, card.loc.y + 12 + card_dy)),
                Size::from((card.size.w - 24, card.size.h - 24)),
            );
            let center_badge = format!("{}  •  {}", win.badge, win.dimensions);
            if let Some(el) = text_cache.render_label_centered(
                renderer,
                &center_badge,
                11.5,
                if win.focused || is_hov || is_sel {
                    text_bright
                } else {
                    text_muted
                },
                body_bounds,
            ) {
                elements.push(el);
            }
        }
    }
}
