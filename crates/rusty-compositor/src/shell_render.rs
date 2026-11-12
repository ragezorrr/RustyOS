//! Visual shell elements rendering (`TopBar`, `Dock`, Desktop background) (`RUSTYOS-012`, ADR-0007).
//!
//! Renders macOS-inspired top status bar and bottom floating dock using Smithay's
//! solid color render elements, adhering to design tokens in `DESIGN.md` (Version 2.0 "RustyOS Purple Glass").

use smithay::backend::renderer::Color32F;
use smithay::backend::renderer::element::solid::SolidColorRenderElement;
use smithay::backend::renderer::element::{Id, Kind};
use smithay::backend::renderer::utils::CommitCounter;
use smithay::utils::{Point, Rectangle, Size};

/// `TopBar` height in logical pixels.
pub const TOP_BAR_HEIGHT: i32 = 32;

/// Dock height in logical pixels.
pub const DOCK_HEIGHT: i32 = 56;

/// Dock bottom margin from screen edge.
pub const DOCK_MARGIN_BOTTOM: i32 = 12;

/// Workspace button width and stride for topbar indicators and click handling.
pub const WS_BTN_WIDTH: i32 = 26;
pub const WS_BTN_GAP: i32 = 4;
pub const WS_BTN_STRIDE: i32 = WS_BTN_WIDTH + WS_BTN_GAP;
pub const WS_START_X: i32 = 12;

// Design tokens from `DESIGN.md` (Version 2.0 "RustyOS Purple Glass")
pub const COLOR_BACKGROUND: Color32F = Color32F::new(0.059, 0.039, 0.098, 1.0); // #0F0A19
pub const COLOR_SURFACE: Color32F = Color32F::new(0.086, 0.063, 0.137, 1.0); // #161023
pub const COLOR_SURFACE_ELEVATED: Color32F = Color32F::new(0.125, 0.094, 0.180, 0.96); // #20182E
pub const COLOR_SURFACE_OVERLAY: Color32F = Color32F::new(0.161, 0.125, 0.216, 0.98); // #292037

pub const COLOR_ACCENT: Color32F = Color32F::new(0.647, 0.431, 1.0, 1.0); // #A56EFF
pub const COLOR_ACCENT_HOVER: Color32F = Color32F::new(0.725, 0.541, 1.0, 1.0); // #B98AFF
pub const COLOR_ACCENT_MUTED: Color32F = Color32F::new(0.463, 0.314, 0.710, 1.0); // #7650B5

pub const COLOR_TEXT: Color32F = Color32F::new(0.957, 0.941, 0.980, 1.0); // #F4F0FA
pub const COLOR_MUTED: Color32F = Color32F::new(0.651, 0.608, 0.710, 1.0); // #A69BB5
pub const COLOR_SUBTLE: Color32F = Color32F::new(0.463, 0.420, 0.522, 1.0); // #766B85

pub const COLOR_DANGER: Color32F = Color32F::new(0.898, 0.420, 0.459, 1.0); // #E56B75
pub const COLOR_SUCCESS: Color32F = Color32F::new(0.514, 0.839, 0.627, 1.0); // #83D6A0
pub const COLOR_WARNING: Color32F = Color32F::new(0.910, 0.769, 0.467, 1.0); // #E8C477

pub const COLOR_BORDER: Color32F = Color32F::new(1.0, 1.0, 1.0, 0.12); // #FFFFFF12 border
pub const COLOR_BORDER_HIGHLIGHT: Color32F = Color32F::new(1.0, 1.0, 1.0, 0.22); // outer glass rim

/// Computes the usable work area rectangle for client windows, reserving space for `TopBar` and `Dock`.
#[must_use]
pub fn usable_work_area(screen_size: Size<i32, smithay::utils::Physical>) -> rusty_wm::Rect {
    let top_margin = TOP_BAR_HEIGHT;
    let bottom_margin = DOCK_HEIGHT + DOCK_MARGIN_BOTTOM * 2;
    let h = (screen_size.h - top_margin - bottom_margin).max(100);
    rusty_wm::Rect::new(0, top_margin, screen_size.w, h)
}

fn make_solid_element(
    geo: Rectangle<i32, smithay::utils::Physical>,
    color: Color32F,
) -> SolidColorRenderElement {
    SolidColorRenderElement::new(
        Id::new(),
        geo,
        CommitCounter::default(),
        color,
        Kind::Unspecified,
    )
}

fn render_top_bar(
    elements: &mut Vec<SolidColorRenderElement>,
    width: i32,
    active_workspace: u8,
    layout_mode: &str,
) {
    // 1. TopBar Background (full width, height 32, elevated surface)
    let top_bar_geo = Rectangle::new(Point::from((0, 0)), Size::from((width, TOP_BAR_HEIGHT)));
    elements.push(make_solid_element(top_bar_geo, COLOR_SURFACE_ELEVATED));

    // 1b. TopBar 1px bottom border (subtle translucency)
    let top_bar_border =
        Rectangle::new(Point::from((0, TOP_BAR_HEIGHT - 1)), Size::from((width, 1)));
    elements.push(make_solid_element(top_bar_border, COLOR_BORDER));

    // 2. Workspaces indicators in TopBar (1..=9)
    let ws_btn_height = 20;
    let ws_y = 6;
    let mut start_x = WS_START_X;

    for ws in 1..=9 {
        let is_active = ws == active_workspace;
        let ws_geo = Rectangle::new(
            Point::from((start_x, ws_y)),
            Size::from((WS_BTN_WIDTH, ws_btn_height)),
        );

        if is_active {
            // Bright neon violet pill with a text-colored highlight pip
            elements.push(make_solid_element(ws_geo, COLOR_ACCENT));
            let pip_geo = Rectangle::new(
                Point::from((start_x + (WS_BTN_WIDTH - 8) / 2, ws_y + ws_btn_height - 3)),
                Size::from((8, 2)),
            );
            elements.push(make_solid_element(pip_geo, COLOR_TEXT));
        } else {
            // Contrast elevated overlay pill with subtle inner indicator
            elements.push(make_solid_element(ws_geo, COLOR_SURFACE_OVERLAY));
            let pip_geo = Rectangle::new(
                Point::from((
                    start_x + (WS_BTN_WIDTH - 6) / 2,
                    ws_y + (ws_btn_height - 2) / 2,
                )),
                Size::from((6, 2)),
            );
            elements.push(make_solid_element(pip_geo, COLOR_SUBTLE));
        }
        start_x += WS_BTN_STRIDE;
    }

    // 3. Layout indicator in TopBar
    let layout_bg = Rectangle::new(
        Point::from((start_x + 8, ws_y)),
        Size::from((56, ws_btn_height)),
    );
    elements.push(make_solid_element(layout_bg, COLOR_SURFACE_OVERLAY));

    // Left accent strip for layout indicator
    let layout_accent = if layout_mode == "master-stack" {
        COLOR_ACCENT
    } else {
        COLOR_SUBTLE
    };
    let layout_strip = Rectangle::new(
        Point::from((start_x + 8, ws_y)),
        Size::from((3, ws_btn_height)),
    );
    elements.push(make_solid_element(layout_strip, layout_accent));

    // 4. Status area on the right side of TopBar (battery, network, clock indicator)
    let status_pill_width = 130;
    let status_x = width - status_pill_width - 12;
    let status_pill_geo = Rectangle::new(
        Point::from((status_x, ws_y)),
        Size::from((status_pill_width, ws_btn_height)),
    );
    elements.push(make_solid_element(status_pill_geo, COLOR_SURFACE_OVERLAY));

    // Top subtle highlight on status pill
    let status_highlight = Rectangle::new(
        Point::from((status_x, ws_y)),
        Size::from((status_pill_width, 1)),
    );
    elements.push(make_solid_element(status_highlight, COLOR_BORDER));

    // Green status dot (system healthy / online)
    let dot_geo = Rectangle::new(Point::from((status_x + 8, ws_y + 6)), Size::from((8, 8)));
    elements.push(make_solid_element(dot_geo, COLOR_SUCCESS));

    // Accent status bars (simulating clock and controls)
    let bar1_geo = Rectangle::new(Point::from((status_x + 24, ws_y + 8)), Size::from((40, 4)));
    elements.push(make_solid_element(bar1_geo, COLOR_MUTED));
    let bar2_geo = Rectangle::new(Point::from((status_x + 72, ws_y + 6)), Size::from((46, 8)));
    elements.push(make_solid_element(bar2_geo, COLOR_ACCENT_MUTED));
}

fn render_dock(
    elements: &mut Vec<SolidColorRenderElement>,
    width: i32,
    height: i32,
    window_count: usize,
    has_focused_window: bool,
) {
    // 5. Dock Background (floating centered pill at the bottom with outer rim)
    let dock_items_count = (3 + window_count).max(3); // 3 pinned apps + open windows
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let dock_width = ((dock_items_count as i32) * 52 + 16).min(width - 48);
    let dock_x = (width - dock_width) / 2;
    let dock_y = height - DOCK_HEIGHT - DOCK_MARGIN_BOTTOM;

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
    let mut item_x = dock_x + 8;
    let item_y = dock_y + 8;
    let item_size = 40;

    for i in 0..dock_items_count {
        let is_running_window = i >= 3;
        let is_focused = is_running_window && has_focused_window;

        // Base icon tile
        let item_geo = Rectangle::new(
            Point::from((item_x, item_y)),
            Size::from((item_size, item_size)),
        );
        elements.push(make_solid_element(item_geo, COLOR_SURFACE_OVERLAY));

        // Inner app glyph plate
        let inner_size = 28;
        let inner_offset = (item_size - inner_size) / 2;
        let inner_geo = Rectangle::new(
            Point::from((item_x + inner_offset, item_y + inner_offset)),
            Size::from((inner_size, inner_size)),
        );
        let inner_color = match i {
            0 => COLOR_ACCENT,       // Terminal / Core
            1 => COLOR_ACCENT_MUTED, // Files
            2 => COLOR_SUBTLE,       // Browser / Settings
            _ if is_focused => COLOR_ACCENT_HOVER,
            _ => COLOR_MUTED,
        };
        elements.push(make_solid_element(inner_geo, inner_color));

        // Active running indicator pip under running windows
        if is_running_window || i == 0 {
            let pip_width = 8;
            let pip_geo = Rectangle::new(
                Point::from((
                    item_x + (item_size - pip_width) / 2,
                    dock_y + DOCK_HEIGHT - 5,
                )),
                Size::from((pip_width, 2)),
            );
            elements.push(make_solid_element(pip_geo, COLOR_ACCENT));
        }

        item_x += item_size + 12;
    }
}

/// Generates custom GLES render elements for `TopBar` and `Dock`.
#[must_use]
pub fn generate_shell_render_elements(
    screen_size: Size<i32, smithay::utils::Physical>,
    active_workspace: u8,
    layout_mode: &str,
    window_count: usize,
    has_focused_window: bool,
) -> Vec<SolidColorRenderElement> {
    let mut elements = Vec::new();
    let width = screen_size.w;
    let height = screen_size.h;

    render_top_bar(&mut elements, width, active_workspace, layout_mode);
    render_dock(
        &mut elements,
        width,
        height,
        window_count,
        has_focused_window,
    );

    elements
}

/// Checks if a mouse click at `(x, y)` was inside the `TopBar` workspaces area and returns the target workspace (1..=9).
#[must_use]
pub fn check_top_bar_workspace_click(x: f64, y: f64) -> Option<u8> {
    if y > f64::from(TOP_BAR_HEIGHT) {
        return None;
    }
    let start_x = f64::from(WS_START_X);
    let stride = f64::from(WS_BTN_STRIDE);
    let total_width = stride * 9.0;

    if x < start_x || x >= start_x + total_width {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let idx = ((x - start_x) / stride).floor() as u8 + 1;
    if (1..=9).contains(&idx) {
        Some(idx)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usable_work_area_reserves_top_and_bottom_margins() {
        let size = Size::from((1920, 1080));
        let work = usable_work_area(size);
        assert_eq!(work.x, 0);
        assert_eq!(work.y, TOP_BAR_HEIGHT);
        assert_eq!(work.width, 1920);
        let expected_h = 1080 - TOP_BAR_HEIGHT - (DOCK_HEIGHT + DOCK_MARGIN_BOTTOM * 2);
        assert_eq!(work.height, expected_h);
    }

    #[test]
    fn generate_shell_render_elements_produces_non_empty_layout() {
        let size = Size::from((1280, 800));
        let elements = generate_shell_render_elements(size, 1, "master-stack", 2, true);
        assert!(!elements.is_empty());
    }

    #[test]
    fn workspace_click_mapping_accurately_detects_all_nine_workspaces() {
        for ws in 1u8..=9u8 {
            let x = f64::from(WS_START_X) + f64::from(i32::from(ws - 1) * WS_BTN_STRIDE) + 5.0;
            let detected = check_top_bar_workspace_click(x, 15.0);
            assert_eq!(detected, Some(ws));
        }

        // Clicks outside top bar height or beyond bounds return None
        assert_eq!(
            check_top_bar_workspace_click(15.0, f64::from(TOP_BAR_HEIGHT + 10)),
            None
        );
        assert_eq!(check_top_bar_workspace_click(2.0, 10.0), None);
    }
}
