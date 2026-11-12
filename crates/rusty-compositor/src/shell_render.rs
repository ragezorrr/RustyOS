//! Visual shell elements rendering (`TopBar`, `Dock`, Desktop background) (`RUSTYOS-012`, ADR-0007).
//!
//! Renders macOS-inspired top status bar and bottom floating dock using Smithay's
//! solid color render elements, adhering to design tokens in `DESIGN.md`.

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

/// Colors from `DESIGN.md`.
pub const COLOR_SURFACE: Color32F = Color32F::new(0.086, 0.063, 0.137, 0.92); // #161023
pub const COLOR_ACCENT: Color32F = Color32F::new(0.647, 0.431, 1.0, 1.0); // #A56EFF
pub const COLOR_MUTED: Color32F = Color32F::new(0.651, 0.608, 0.710, 0.4); // #A69BB5
pub const COLOR_ACTIVE_TAB: Color32F = Color32F::new(0.647, 0.431, 1.0, 0.35);
pub const COLOR_DOCK_ITEM: Color32F = Color32F::new(0.18, 0.15, 0.25, 0.95);
pub const COLOR_DOCK_ACTIVE: Color32F = Color32F::new(0.647, 0.431, 1.0, 0.9);

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

    // 1. TopBar Background (full width, height 32)
    let top_bar_geo = Rectangle::new(Point::from((0, 0)), Size::from((width, TOP_BAR_HEIGHT)));
    elements.push(make_solid_element(top_bar_geo, COLOR_SURFACE));

    // 2. Workspaces indicators in TopBar (1..=9)
    let ws_btn_width = 24;
    let ws_btn_height = 20;
    let ws_y = 6;
    let mut start_x = 12;

    for ws in 1..=9 {
        let is_active = ws == active_workspace;
        let color = if is_active { COLOR_ACCENT } else { COLOR_MUTED };
        let ws_geo = Rectangle::new(
            Point::from((start_x, ws_y)),
            Size::from((ws_btn_width, ws_btn_height)),
        );
        elements.push(make_solid_element(ws_geo, color));
        start_x += ws_btn_width + 4;
    }

    // 3. Layout indicator in TopBar
    let layout_color = if layout_mode == "master-stack" {
        COLOR_ACCENT
    } else {
        COLOR_MUTED
    };
    let layout_geo = Rectangle::new(
        Point::from((start_x + 12, ws_y)),
        Size::from((48, ws_btn_height)),
    );
    elements.push(make_solid_element(layout_geo, layout_color));

    // 4. Status pill on the right side of TopBar (battery/network/time indicator)
    let status_pill_width = 120;
    let status_pill_geo = Rectangle::new(
        Point::from((width - status_pill_width - 12, ws_y)),
        Size::from((status_pill_width, ws_btn_height)),
    );
    elements.push(make_solid_element(status_pill_geo, COLOR_ACTIVE_TAB));

    // 5. Dock Background (floating centered pill at the bottom)
    let dock_items_count = (2 + window_count).max(2); // 2 pinned apps + open windows
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let dock_width = ((dock_items_count as i32) * 48 + 24).min(width - 48);
    let dock_x = (width - dock_width) / 2;
    let dock_y = height - DOCK_HEIGHT - DOCK_MARGIN_BOTTOM;

    let dock_geo = Rectangle::new(
        Point::from((dock_x, dock_y)),
        Size::from((dock_width, DOCK_HEIGHT)),
    );
    elements.push(make_solid_element(dock_geo, COLOR_SURFACE));

    // 6. Dock Item slots
    let mut item_x = dock_x + 12;
    let item_y = dock_y + 8;
    let item_size = 40;

    for i in 0..dock_items_count {
        let is_active_window = i == 2 && has_focused_window;
        let color = if is_active_window {
            COLOR_DOCK_ACTIVE
        } else {
            COLOR_DOCK_ITEM
        };
        let item_geo = Rectangle::new(
            Point::from((item_x, item_y)),
            Size::from((item_size, item_size)),
        );
        elements.push(make_solid_element(item_geo, color));
        item_x += item_size + 8;
    }

    elements
}

/// Checks if a mouse click at `(x, y)` was inside the `TopBar` workspaces area and returns the target workspace (1..=9).
#[must_use]
pub fn check_top_bar_workspace_click(x: f64, y: f64) -> Option<u8> {
    if y > f64::from(TOP_BAR_HEIGHT) {
        return None;
    }
    let start_x = 12.0;
    let btn_width = 28.0;
    if x < start_x || x > start_x + btn_width * 9.0 {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let idx = ((x - start_x) / btn_width).floor() as u8 + 1;
    if (1..=9).contains(&idx) {
        Some(idx)
    } else {
        None
    }
}
