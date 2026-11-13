//! Visual shell elements rendering (`TopBar`, `Dock`, Desktop background) (`RUSTYOS-012`, ADR-0007).
//!
//! Renders macOS-inspired top status bar and bottom floating dock using Smithay's
//! solid color render elements, adhering to design tokens in `DESIGN.md` (Version 2.0 "RustyOS Purple Glass").

use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
use smithay::backend::renderer::element::solid::SolidColorRenderElement;
use smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement;
use smithay::backend::renderer::element::texture::TextureRenderElement;
use smithay::backend::renderer::element::utils::{RelocateRenderElement, RescaleRenderElement};
use smithay::backend::renderer::gles::{GlesRenderer, GlesTexture};
use smithay::utils::{Point, Rectangle, Size};

smithay::backend::renderer::element::render_elements! {
    /// Unified render element type emitted across shell, SSD frames, text, and Wayland surfaces.
    pub CompositorRenderElement<=GlesRenderer>;
    /// Client Wayland surface element.
    Surface = WaylandSurfaceRenderElement<GlesRenderer>,
    /// Solid or translucent colored rectangle.
    Solid = SolidColorRenderElement,
    /// Scaled solid rectangle mapped from logical shell UI coordinates to HiDPI physical pixels.
    ScaledSolid = RescaleRenderElement<SolidColorRenderElement>,
    /// CPU-rasterized, GPU-cached buffer (text glyphs, rounded SDF corners, circles, shadows).
    Memory = MemoryRenderBufferRenderElement<GlesRenderer>,
    /// Relocated memory buffer aligned to HiDPI physical output coordinates.
    ScaledMemory = RelocateRenderElement<MemoryRenderBufferRenderElement<GlesRenderer>>,
    /// Client-supplied Wayland texture element (e.g. `xdg_toplevel_icon_v1` SHM icon).
    Texture = TextureRenderElement<GlesTexture>,
    /// Relocated Wayland texture element aligned to HiDPI physical output coordinates.
    ScaledTexture = RelocateRenderElement<TextureRenderElement<GlesTexture>>,
}

mod dock;
mod lock_screen;
mod modals;
mod quick_settings;
mod spotlight_overview;
mod ssd;
mod tokens;
mod top_bar;

pub use dock::{
    DockHit, DockViewState, DockWindowEntry, check_dock_click, check_dock_slot, extract_app_name,
    make_window_badge,
};
#[cfg(test)]
pub(crate) use lock_screen::lock_screen_primary_row_center;
pub use lock_screen::{
    LockScreenHit, LockScreenRenderState, check_lock_screen_unlock_click,
    generate_lock_screen_elements, hit_test_lock_screen,
};
pub use modals::{
    PolkitHit, PolkitPromptRenderState, PortalHit, PortalPromptRenderState, PortalSourceRow,
    hit_test_polkit_dialog, hit_test_portal_dialog,
};
pub use quick_settings::{
    QuickSettingsHit, QuickSettingsRenderState, QuickSettingsSliderDrag,
    check_quick_settings_click, check_quick_settings_slider_drag, quick_settings_slider_percent,
};
pub use spotlight_overview::{
    OverviewHit, OverviewRenderState, OverviewWorkspaceSpace, SpotlightHit, SpotlightRenderState,
    WindowSwitcherCard, WindowSwitcherRenderState, check_overview_click,
    check_overview_click_with_spaces, check_spotlight_click, check_switcher_click,
    overview_card_rects, overview_space_rects, switcher_layout,
};
pub use ssd::{
    ResizeEdges, SsdHit, SsdHitWindow, check_window_frame_hit, check_window_ssd_hit,
    check_window_traffic_lights_hover, generate_ssd_title_element, generate_window_shadow_elements,
    generate_window_ssd_composite_elements, generate_window_ssd_composite_elements_with_hover,
    generate_window_ssd_elements,
};
pub(crate) use tokens::scale_glass_rgba;
pub use tokens::{
    COLOR_ACCENT, COLOR_ACCENT_HOVER, COLOR_ACCENT_MUTED, COLOR_BACKGROUND, COLOR_BORDER,
    COLOR_BORDER_HIGHLIGHT, COLOR_DANGER, COLOR_MUTED, COLOR_SUBTLE, COLOR_SUCCESS, COLOR_SURFACE,
    COLOR_SURFACE_ELEVATED, COLOR_SURFACE_OVERLAY, COLOR_TEXT, COLOR_WARNING, DOCK_DIVIDER_GAP,
    DOCK_HEIGHT, DOCK_ICON_SIZE, DOCK_ITEM_GAP, DOCK_ITEM_SIZE, DOCK_MARGIN_BOTTOM, DOCK_PADDING,
    DOCK_PINNED_COUNT, DOCK_RADIUS, DOCK_UTILITY_COUNT, RESIZE_GRAB_BORDER, TITLEBAR_HEIGHT,
    TOP_BAR_APP_X, TOP_BAR_FONT_SIZE, TOP_BAR_HEIGHT, TOP_BAR_ITEM_H, TOP_BAR_ITEM_Y,
    TOP_BAR_LOGO_W, TOP_BAR_LOGO_X, WINDOW_RADIUS, WS_BTN_GAP, WS_BTN_STRIDE, WS_BTN_WIDTH,
    usable_work_area,
};
pub use top_bar::{
    MAX_TOP_BAR_TRAY_ITEMS, MAX_VISIBLE_NOTIFICATIONS, MENU_POPOVER_RADIUS, NotificationToastView,
    OsdRenderState, TopBarLayoutOptions, TopBarLeftLayout, TopBarMenuAction, TopBarMenuEntry,
    TopBarMenuHit, TopBarMenuPopoverClick, TopBarRightLayout, TopBarTelemetry, TopBarTrayItem,
    check_notification_click, check_top_bar_kbd_click, check_top_bar_kbd_click_for_options,
    check_top_bar_kbd_click_for_telemetry, check_top_bar_layout_click,
    check_top_bar_layout_click_for_app, check_top_bar_layout_click_with_options,
    check_top_bar_menu_click, check_top_bar_menu_click_for_app, check_top_bar_menu_popover_click,
    check_top_bar_menu_popover_hover_row, check_top_bar_notification_click,
    check_top_bar_notification_click_for_options, check_top_bar_notification_click_for_telemetry,
    check_top_bar_search_click_for_options, check_top_bar_search_click_for_telemetry,
    check_top_bar_status_click, check_top_bar_status_click_for_options,
    check_top_bar_status_click_for_telemetry, check_top_bar_tray_click,
    check_top_bar_tray_click_for_options, check_top_bar_tray_click_for_telemetry,
    check_top_bar_workspace_click, check_top_bar_workspace_click_for_app,
    check_top_bar_workspace_click_with_options, compute_top_bar_left_layout,
    compute_top_bar_left_layout_with_options, compute_top_bar_right_layout,
    compute_top_bar_right_layout_with_options, generate_osd_elements, notification_toast_rects,
    osd_pill_rect, top_bar_menu_entries, top_bar_menu_items, top_bar_menu_popover_rect,
    tray_slot_rects, tray_slot_rects_with_layout, tray_slot_rects_with_options,
};

use dock::{dock_layout_bounds, dock_slot_rect, dock_tooltip_rect_and_label, render_dock};
use modals::{
    render_pick_color_composite, render_pick_color_text, render_polkit_composite,
    render_polkit_text, render_portal_composite, render_portal_text,
};
use quick_settings::{
    render_quick_settings_composite, render_quick_settings_popover, render_quick_settings_text,
};
use spotlight_overview::{
    render_overview_text, render_spotlight_modal, render_switcher_text, spotlight_modal_rect,
};
use tokens::{
    RGBA_ACCENT, RGBA_ACCENT_MUTED, RGBA_BORDER, RGBA_BORDER_HIGHLIGHT, RGBA_DANGER, RGBA_SURFACE,
    RGBA_SURFACE_ELEVATED, RGBA_SURFACE_OVERLAY, anim_offset, make_solid_element, premul,
    push_liquid_glass_panel, push_rounded_glass_box, push_soft_shadow, resolved_icon_to_element,
    rgba_to_premul_color32f, title_hue_seed, truncate_ascii,
};
use top_bar::{
    MENU_ITEM_HEIGHT, MENU_POPOVER_PAD_Y, MENU_SEPARATOR_HEIGHT, notification_app_icon,
    push_top_bar_right_glyphs, render_top_bar,
};

/// Active modal/popover overlay rendered on top of the compositor shell.
#[derive(Debug, Clone, Copy)]
pub enum ShellOverlayView<'a> {
    /// No overlay open.
    None,
    /// Spotlight application launcher overlay.
    Spotlight(&'a SpotlightRenderState<'a>),
    /// Quick Settings Control Center popover (`RUSTYOS-021`).
    QuickSettings(&'a QuickSettingsRenderState<'a>),
    /// Workspace Overview / Expose window grid (`RUSTYOS-023`).
    Overview(&'a OverviewRenderState<'a>),
    /// Interactive `Alt+Tab` window switcher HUD (`RUSTYOS-033`).
    Switcher(&'a WindowSwitcherRenderState<'a>),
    /// Native `PolicyKit` authentication modal (`RUSTYOS-056`).
    Polkit(&'a PolkitPromptRenderState<'a>),
    /// Native XDG Desktop Portal consent modal (`RUSTYOS-057`).
    Portal(&'a PortalPromptRenderState<'a>),
    /// Non-dimming interactive `PickColor` sampling banner pill (`RUSTYOS-057`).
    PickColor(&'a str),
}

/// Generates custom GLES render elements for `TopBar`, `Dock`, and active shell overlays.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn generate_shell_render_elements(
    screen_size: Size<i32, smithay::utils::Physical>,
    active_workspace: u8,
    layout_mode: &str,
    window_count: usize,
    has_focused_window: bool,
    top_telemetry: &TopBarTelemetry<'_>,
    overlay: ShellOverlayView<'_>,
    has_text: bool,
) -> Vec<SolidColorRenderElement> {
    let mut elements = Vec::new();
    let width = screen_size.w;
    let height = screen_size.h;

    render_top_bar(
        &mut elements,
        width,
        active_workspace,
        layout_mode,
        top_telemetry,
        has_text,
    );
    render_dock(
        &mut elements,
        width,
        height,
        window_count,
        has_focused_window,
    );

    match overlay {
        ShellOverlayView::None
        | ShellOverlayView::Overview(_)
        | ShellOverlayView::Switcher(_)
        | ShellOverlayView::Polkit(_)
        | ShellOverlayView::Portal(_)
        | ShellOverlayView::PickColor(_) => {}
        ShellOverlayView::Spotlight(spot) => {
            render_spotlight_modal(&mut elements, width, height, spot, has_text);
        }
        ShellOverlayView::QuickSettings(qs) => {
            render_quick_settings_popover(&mut elements, width, height, qs);
        }
    }

    // Smithay's render_output expects elements in front-to-back order.
    elements.reverse();
    elements
}

const TEXT_BRIGHT: [u8; 4] = [244, 240, 250, 255]; // #F4F0FA
const TEXT_MUTED: [u8; 4] = [166, 155, 181, 255]; // #A69BB5

const fn notifications_hidden_by_overlay(overlay: ShellOverlayView<'_>) -> bool {
    matches!(
        overlay,
        ShellOverlayView::Spotlight(_)
            | ShellOverlayView::QuickSettings(_)
            | ShellOverlayView::Polkit(_)
            | ShellOverlayView::Portal(_)
    )
}

const fn dock_tooltip_hidden_by_overlay(overlay: ShellOverlayView<'_>) -> bool {
    matches!(
        overlay,
        ShellOverlayView::Spotlight(_)
            | ShellOverlayView::QuickSettings(_)
            | ShellOverlayView::Switcher(_)
            | ShellOverlayView::Polkit(_)
            | ShellOverlayView::Portal(_)
    )
}

#[allow(clippy::too_many_lines)]
fn append_foreground_overlay_text(
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    overlay: ShellOverlayView<'_>,
    notifications: &[NotificationToastView<'_>],
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    if !text_cache.has_font() {
        return;
    }

    let width = screen_size.w;
    let height = screen_size.h;

    match overlay {
        ShellOverlayView::None | ShellOverlayView::Overview(_) => {}
        ShellOverlayView::Spotlight(spot) => {
            let (modal_x, base_modal_y, modal_width, _modal_height) =
                spotlight_modal_rect(width, height);
            let modal_y = base_modal_y + anim_offset(spot.progress, -22.0);

            if spot.query.is_empty() {
                if let Some(el) = text_cache.render_label(
                    renderer,
                    "Spotlight Search",
                    14.5,
                    TEXT_MUTED,
                    Point::from((modal_x + 60, modal_y + 30)),
                ) {
                    elements.push(el);
                }
            } else if let Some(el) = text_cache.render_label(
                renderer,
                spot.query,
                14.5,
                TEXT_BRIGHT,
                Point::from((modal_x + 60, modal_y + 30)),
            ) {
                elements.push(el);
            }

            let start_row_y = modal_y + 80;
            let row_h = 46;
            for (i, item) in spot.items.iter().take(5).enumerate() {
                #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
                let row_y = start_row_y + (i as i32) * (row_h + 4);
                let is_sel = i == spot.selected_in_window;

                let has_icon = item
                    .icon
                    .as_deref()
                    .is_some_and(|name| shape_cache.icons.has_named_icon(name, 28));
                if !has_icon {
                    let badge_bounds = Rectangle::new(
                        Point::from((modal_x + 28, row_y + 9)),
                        Size::from((28, 28)),
                    );
                    if let Some(el) = text_cache.render_label_centered(
                        renderer,
                        &item.badge,
                        11.0,
                        TEXT_BRIGHT,
                        badge_bounds,
                    ) {
                        elements.push(el);
                    }
                }

                if let Some(el) = text_cache.render_label(
                    renderer,
                    &item.name,
                    13.5,
                    TEXT_BRIGHT,
                    Point::from((modal_x + 68, row_y + 7)),
                ) {
                    elements.push(el);
                }
                if let Some(el) = text_cache.render_label(
                    renderer,
                    &item.subtitle,
                    10.5,
                    TEXT_MUTED,
                    Point::from((modal_x + 68, row_y + 26)),
                ) {
                    elements.push(el);
                }

                // macOS Spotlight action / category badge on the right (`RUSTYOS-039`/`051`)
                let cat_label = if is_sel {
                    "Open ↵"
                } else if item.is_custom_command {
                    "Command"
                } else {
                    "Application"
                };
                let cat_bounds = Rectangle::new(
                    Point::from((modal_x + modal_width - 104, row_y + 14)),
                    Size::from((80, 18)),
                );
                let cat_color = if is_sel {
                    [255, 252, 255, 255]
                } else {
                    TEXT_MUTED
                };
                if let Some(el) = text_cache
                    .render_label_centered(renderer, cat_label, 10.5, cat_color, cat_bounds)
                {
                    elements.push(el);
                }
            }
        }
        ShellOverlayView::QuickSettings(qs) => {
            render_quick_settings_text(text_cache, renderer, width, height, qs, elements);
        }
        ShellOverlayView::Switcher(sw) => {
            render_switcher_text(text_cache, shape_cache, renderer, screen_size, sw, elements);
        }
        ShellOverlayView::Polkit(pk) => {
            render_polkit_text(text_cache, renderer, screen_size, pk, elements);
        }
        ShellOverlayView::Portal(portal) => {
            render_portal_text(text_cache, renderer, screen_size, portal, elements);
        }
        ShellOverlayView::PickColor(app_id) => {
            render_pick_color_text(text_cache, renderer, screen_size, app_id, elements);
        }
    }

    if !notifications_hidden_by_overlay(overlay) && !notifications.is_empty() {
        let rects = notification_toast_rects(screen_size, notifications.len());
        for (toast, card) in notifications.iter().zip(rects) {
            let tx = card.loc.x;
            let ty = card.loc.y;

            let title_text = truncate_ascii(toast.title, 28);
            if let Some(el) = text_cache.render_label(
                renderer,
                &title_text,
                12.5,
                TEXT_BRIGHT,
                Point::from((tx + 62, ty + 13)),
            ) {
                elements.push(el);
            }

            let body_text = truncate_ascii(toast.body, 36);
            if let Some(el) = text_cache.render_label(
                renderer,
                &body_text,
                10.5,
                TEXT_MUTED,
                Point::from((tx + 62, ty + 35)),
            ) {
                elements.push(el);
            }
        }
    }
}

/// Generates GPU-cached real font text labels for `TopBar`, `Dock`, `Spotlight`, `QuickSettings`,
/// `Workspace Overview`, and `Notification HUD` (`RUSTYOS-020`/`021`/`023`/`024`).
#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
pub fn generate_shell_text_elements(
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    active_workspace: u8,
    layout_mode: &str,
    dock_view: &DockViewState<'_>,
    top_telemetry: &TopBarTelemetry<'_>,
    overlay: ShellOverlayView<'_>,
    notifications: &[NotificationToastView<'_>],
) -> Vec<MemoryRenderBufferRenderElement<GlesRenderer>> {
    let mut elements = Vec::new();
    if !text_cache.has_font() {
        return elements;
    }

    append_foreground_overlay_text(
        text_cache,
        shape_cache,
        renderer,
        screen_size,
        overlay,
        notifications,
        &mut elements,
    );
    if let ShellOverlayView::Overview(ov) = overlay {
        render_overview_text(text_cache, renderer, screen_size, ov, &mut elements);
    }
    append_chrome_text(
        text_cache,
        shape_cache,
        renderer,
        screen_size,
        active_workspace,
        layout_mode,
        dock_view,
        top_telemetry,
        overlay,
        &mut elements,
    );
    elements
}

#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
fn append_chrome_text(
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    active_workspace: u8,
    layout_mode: &str,
    dock_view: &DockViewState<'_>,
    top_telemetry: &TopBarTelemetry<'_>,
    overlay: ShellOverlayView<'_>,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    if !text_cache.has_font() {
        return;
    }

    let width = screen_size.w;
    let height = screen_size.h;
    let text_bright = TEXT_BRIGHT;
    let text_muted = TEXT_MUTED;

    // 1b. TopBar Left Active App & Global Menus (`RUSTYOS-038`, `RUSTYOS-048`, `RUSTYOS-049`)
    let ws_btn_height = TOP_BAR_ITEM_H;
    let ws_y = TOP_BAR_ITEM_Y;

    let active_app_name = dock_view
        .windows
        .iter()
        .find(|w| w.focused)
        .map_or_else(|| "RustyOS".to_owned(), |w| extract_app_name(&w.title));
    let opts = top_telemetry.layout_options();
    let left_layout =
        compute_top_bar_left_layout_with_options(&active_app_name, top_telemetry.show_workspaces);

    let app_bounds = Rectangle::new(
        Point::from((left_layout.app_x, ws_y)),
        Size::from((left_layout.app_w, ws_btn_height)),
    );
    let pure_white = [252, 250, 255, 255];
    if let Some(el) = text_cache.render_label_centered_weighted(
        renderer,
        &active_app_name,
        crate::text::FontWeight::Bold,
        TOP_BAR_FONT_SIZE,
        pure_white,
        app_bounds,
    ) {
        elements.push(el);
    }

    let text_menu = [245, 242, 252, 245];
    for (menu_kind, name, x, w) in [
        (
            TopBarMenuHit::File,
            "File",
            left_layout.file_x,
            left_layout.file_w,
        ),
        (
            TopBarMenuHit::Edit,
            "Edit",
            left_layout.edit_x,
            left_layout.edit_w,
        ),
        (
            TopBarMenuHit::View,
            "View",
            left_layout.view_x,
            left_layout.view_w,
        ),
        (TopBarMenuHit::Go, "Go", left_layout.go_x, left_layout.go_w),
        (
            TopBarMenuHit::Window,
            "Window",
            left_layout.win_x,
            left_layout.win_w,
        ),
        (
            TopBarMenuHit::Help,
            "Help",
            left_layout.help_x,
            left_layout.help_w,
        ),
    ] {
        let color = if top_telemetry.active_menu == Some(menu_kind) {
            [255, 255, 255, 255]
        } else {
            text_menu
        };
        let bounds = Rectangle::new(Point::from((x, ws_y)), Size::from((w, ws_btn_height)));
        if let Some(el) = text_cache.render_label_centered_weighted(
            renderer,
            name,
            crate::text::FontWeight::Regular,
            TOP_BAR_FONT_SIZE,
            color,
            bounds,
        ) {
            elements.push(el);
        }
    }

    // Active TopBar left dropdown menu labels + right-aligned real shortcuts (`RUSTYOS-048`/`049`/`051`)
    if let Some(open_menu) = top_telemetry.active_menu {
        let (mx, my, mw, _) = top_bar_menu_popover_rect(&active_app_name, open_menu);
        let mut row_y = my + MENU_POPOVER_PAD_Y;
        for (entry_idx, entry) in top_bar_menu_entries(open_menu).iter().enumerate() {
            match entry {
                TopBarMenuEntry::Item {
                    label, shortcut, ..
                } => {
                    let hovered = top_telemetry.menu_hover_row == Some(entry_idx);
                    let fg = if hovered {
                        [255, 252, 255, 255]
                    } else {
                        text_bright
                    };
                    if let Some(el) = text_cache.render_label(
                        renderer,
                        label,
                        12.0,
                        fg,
                        Point::from((mx + 34, row_y + 6)),
                    ) {
                        elements.push(el);
                    }
                    if let Some(hint) = shortcut {
                        let (hint_w, _) = text_cache.measure_text(hint, 11.0);
                        let hint_fg = if hovered {
                            [248, 240, 255, 255]
                        } else {
                            text_muted
                        };
                        if let Some(el) = text_cache.render_label(
                            renderer,
                            hint,
                            11.0,
                            hint_fg,
                            Point::from((mx + mw - 14 - hint_w, row_y + 7)),
                        ) {
                            elements.push(el);
                        }
                    }
                    row_y += MENU_ITEM_HEIGHT;
                }
                TopBarMenuEntry::Separator => {
                    row_y += MENU_SEPARATOR_HEIGHT;
                }
            }
        }
    }

    // 2. Optional TopBar workspace digits & layout mode (`menubar_show_workspaces = true`)
    if top_telemetry.show_workspaces {
        let mut start_x = left_layout.ws_start_x;
        for ws in 1..=9 {
            let color = if ws == active_workspace {
                [255, 255, 255, 255]
            } else {
                [210, 202, 224, 220]
            };
            let bounds = Rectangle::new(
                Point::from((start_x, ws_y)),
                Size::from((WS_BTN_WIDTH, ws_btn_height)),
            );
            if let Some(el) =
                text_cache.render_label_centered(renderer, &ws.to_string(), 10.5, color, bounds)
            {
                elements.push(el);
            }
            start_x += WS_BTN_STRIDE;
        }

        let layout_label = if layout_mode == "master-stack" {
            "Tile"
        } else {
            "Float"
        };
        let layout_bounds = Rectangle::new(
            Point::from((left_layout.layout_x, ws_y)),
            Size::from((left_layout.layout_w, ws_btn_height)),
        );
        if let Some(el) = text_cache.render_label_centered(
            renderer,
            layout_label,
            11.0,
            text_bright,
            layout_bounds,
        ) {
            elements.push(el);
        }
    }

    // 4. Right status cluster live clock and optional kbd / notification badge / tray (`RUSTYOS-049`)
    let right_layout =
        compute_top_bar_right_layout_with_options(width, top_telemetry.clock_hhmm, opts);
    let clock_bounds = Rectangle::new(
        Point::from((right_layout.clock_x, ws_y)),
        Size::from((right_layout.clock_w, ws_btn_height)),
    );
    if let Some(el) = text_cache.render_label_centered_weighted(
        renderer,
        top_telemetry.clock_hhmm,
        crate::text::FontWeight::Regular,
        TOP_BAR_FONT_SIZE,
        pure_white,
        clock_bounds,
    ) {
        elements.push(el);
    }

    if let Some(kbd_x) = right_layout.kbd_x {
        let kbd_bounds = Rectangle::new(
            Point::from((kbd_x, ws_y)),
            Size::from((right_layout.kbd_w, ws_btn_height)),
        );
        if let Some(el) = text_cache.render_label_centered(
            renderer,
            top_telemetry.kbd_layout,
            10.5,
            pure_white,
            kbd_bounds,
        ) {
            elements.push(el);
        }
    }

    if top_telemetry.notification_count > 0 {
        let badge_label = if top_telemetry.notification_count > 99 {
            "N:99+".to_owned()
        } else {
            format!("N:{}", top_telemetry.notification_count)
        };
        let badge_bounds = Rectangle::new(
            Point::from((right_layout.notif_x, ws_y)),
            Size::from((right_layout.notif_w, ws_btn_height)),
        );
        if let Some(el) =
            text_cache.render_label_centered(renderer, &badge_label, 10.5, pure_white, badge_bounds)
        {
            elements.push(el);
        }
    }

    if top_telemetry.show_tray {
        let tray_rects = tray_slot_rects_with_options(
            width,
            top_telemetry.clock_hhmm,
            opts,
            top_telemetry.tray_items.len(),
        );
        for (item, (tx, _ty, tw, _th)) in top_telemetry.tray_items.iter().zip(tray_rects) {
            let has_tray_icon = item.icon_pixmap.is_some()
                || (!item.icon_name.is_empty()
                    && shape_cache.icons.has_named_icon(item.icon_name, 16));
            if has_tray_icon {
                continue;
            }
            let label_color = if item.attention {
                [255, 210, 214, 255]
            } else if item.passive {
                text_muted
            } else {
                pure_white
            };
            let tray_bounds =
                Rectangle::new(Point::from((tx, ws_y)), Size::from((tw, ws_btn_height)));
            if let Some(el) = text_cache.render_label_centered(
                renderer,
                item.label,
                9.5,
                label_color,
                tray_bounds,
            ) {
                elements.push(el);
            }
        }
    }

    // 5. Dock hover tooltip label (`RUSTYOS-050`)
    if !dock_tooltip_hidden_by_overlay(overlay)
        && let Some((tooltip_rect, tooltip_label)) =
            dock_tooltip_rect_and_label(width, height, dock_view)
        && let Some(el) = text_cache.render_label_centered_weighted(
            renderer,
            &tooltip_label,
            crate::text::FontWeight::Bold,
            12.0,
            [252, 250, 255, 255],
            Rectangle::new(
                Point::from((tooltip_rect.loc.x, tooltip_rect.loc.y)),
                Size::from((tooltip_rect.size.w, tooltip_rect.size.h)),
            ),
        )
    {
        elements.push(el);
    }
    let _ = shape_cache;
}

/// Generates foreground system/interactive overlay elements (`Polkit`, `Portal`, `PickColor`,
/// `Switcher`, `Spotlight`, `QuickSettings`, and floating notification toasts) sorted front-to-back (`RUSTYOS-060`).
#[allow(clippy::too_many_arguments)]
pub fn generate_shell_foreground_overlay_elements(
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    top_telemetry: &TopBarTelemetry<'_>,
    overlay: ShellOverlayView<'_>,
    notifications: &[NotificationToastView<'_>],
) -> Vec<CompositorRenderElement> {
    let has_foreground_overlay = !matches!(
        overlay,
        ShellOverlayView::None | ShellOverlayView::Overview(_)
    );
    let show_toasts = !notifications_hidden_by_overlay(overlay) && !notifications.is_empty();
    if !has_foreground_overlay && !show_toasts {
        return Vec::new();
    }

    let mut out = Vec::new();
    let has_text = text_cache.has_font();
    let mut overlay_text = Vec::new();
    append_foreground_overlay_text(
        text_cache,
        shape_cache,
        renderer,
        screen_size,
        overlay,
        notifications,
        &mut overlay_text,
    );
    out.extend(
        overlay_text
            .into_iter()
            .map(CompositorRenderElement::Memory),
    );
    out.extend(build_foreground_overlay_shapes(
        shape_cache,
        renderer,
        screen_size,
        has_text,
        top_telemetry,
        overlay,
        notifications,
    ));
    out
}

/// Generates persistent desktop chrome (`TopBar`, dropdown menu, and `Dock`) sorted front-to-back (`RUSTYOS-060`).
#[allow(clippy::too_many_arguments)]
pub fn generate_shell_chrome_composite_elements(
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    active_workspace: u8,
    layout_mode: &str,
    dock_view: &DockViewState<'_>,
    top_telemetry: &TopBarTelemetry<'_>,
    overlay: ShellOverlayView<'_>,
) -> Vec<CompositorRenderElement> {
    let mut out = Vec::new();
    let mut chrome_text = Vec::new();
    append_chrome_text(
        text_cache,
        shape_cache,
        renderer,
        screen_size,
        active_workspace,
        layout_mode,
        dock_view,
        top_telemetry,
        overlay,
        &mut chrome_text,
    );
    out.extend(chrome_text.into_iter().map(CompositorRenderElement::Memory));
    out.extend(build_chrome_shapes(
        shape_cache,
        renderer,
        screen_size,
        active_workspace,
        layout_mode,
        dock_view,
        top_telemetry,
        overlay,
    ));
    out
}

/// Generates Workspace Overview elements (Spaces strip, preview cards, scrim) sorted front-to-back (`RUSTYOS-060`).
pub fn generate_shell_overview_elements(
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    top_telemetry: &TopBarTelemetry<'_>,
    overlay: ShellOverlayView<'_>,
) -> Vec<CompositorRenderElement> {
    let ShellOverlayView::Overview(ov) = overlay else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut ov_text = Vec::new();
    if text_cache.has_font() {
        render_overview_text(text_cache, renderer, screen_size, ov, &mut ov_text);
    }
    out.extend(ov_text.into_iter().map(CompositorRenderElement::Memory));
    out.extend(build_overview_shapes(
        shape_cache,
        renderer,
        screen_size,
        top_telemetry,
        ov,
    ));
    out
}

/// Generates unified front-to-back shell elements with rounded corners, circular status dots,
/// soft shadows, interactive Dock badges, Overview cards, Notification HUD, and GPU-cached typography (`RUSTYOS-022`/`023`/`024`).
#[allow(clippy::too_many_arguments)]
pub fn generate_shell_composite_elements(
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    active_workspace: u8,
    layout_mode: &str,
    dock_view: &DockViewState<'_>,
    top_telemetry: &TopBarTelemetry<'_>,
    overlay: ShellOverlayView<'_>,
    notifications: &[NotificationToastView<'_>],
) -> Vec<CompositorRenderElement> {
    let mut out = generate_shell_foreground_overlay_elements(
        text_cache,
        shape_cache,
        renderer,
        screen_size,
        top_telemetry,
        overlay,
        notifications,
    );
    out.extend(generate_shell_chrome_composite_elements(
        text_cache,
        shape_cache,
        renderer,
        screen_size,
        active_workspace,
        layout_mode,
        dock_view,
        top_telemetry,
        overlay,
    ));
    out.extend(generate_shell_overview_elements(
        text_cache,
        shape_cache,
        renderer,
        screen_size,
        top_telemetry,
        overlay,
    ));
    out
}

#[allow(clippy::too_many_lines)]
fn build_foreground_overlay_shapes(
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    has_text: bool,
    top_telemetry: &TopBarTelemetry<'_>,
    overlay: ShellOverlayView<'_>,
    notifications: &[NotificationToastView<'_>],
) -> Vec<CompositorRenderElement> {
    let mut out = Vec::new();
    let width = screen_size.w;
    let height = screen_size.h;
    match overlay {
        ShellOverlayView::None | ShellOverlayView::Overview(_) => {}
        ShellOverlayView::Spotlight(spot) => {
            let (mx, base_my, mw, mh) = spotlight_modal_rect(width, height);
            let my = base_my + anim_offset(spot.progress, -22.0);
            let glass_op = top_telemetry.glass_opacity;
            let reduce_tr = top_telemetry.reduce_transparency;

            // SDF Search loupe & cursor inside search input bar (`RUSTYOS-048`)
            if let Some(el) = shape_cache.ui_glyph_element(
                renderer,
                crate::surfaces::UiGlyphKind::Search,
                18,
                [215, 205, 235, 255],
                Point::from((mx + 30, my + 31)),
            ) {
                out.push(CompositorRenderElement::Memory(el));
            }
            let cursor_x = (mx + 60 + spot.cursor_offset_x).min(mx + mw - 28);
            out.push(CompositorRenderElement::Solid(make_solid_element(
                Rectangle::new(Point::from((cursor_x, my + 28)), Size::from((2, 24))),
                COLOR_ACCENT,
            )));
            if !has_text {
                out.push(CompositorRenderElement::Solid(make_solid_element(
                    Rectangle::new(Point::from((mx + 64, my + 36)), Size::from((160, 8))),
                    COLOR_MUTED,
                )));
            }

            // Search input bar plate (`macOS 27` Liquid Glass module, `RUSTYOS-039`/`051`)
            push_liquid_glass_panel(
                &mut out,
                shape_cache,
                renderer,
                Rectangle::new(Point::from((mx + 16, my + 16)), Size::from((mw - 32, 48))),
                14,
                crate::surfaces::GlassPanelStyle::ControlModule,
                glass_op,
                reduce_tr,
                None,
            );
            out.push(CompositorRenderElement::Solid(make_solid_element(
                Rectangle::new(Point::from((mx + 16, my + 72)), Size::from((mw - 32, 1))),
                COLOR_BORDER,
            )));

            let row_h = 46;
            let start_row_y = my + 80;
            for idx in 0..spot.items.len().min(5) {
                let i = i32::try_from(idx).unwrap_or(0);
                let row_y = start_row_y + i * (row_h + 4);
                let is_sel = idx == spot.selected_in_window;

                let row_rgba = if is_sel {
                    [165, 110, 255, 198]
                } else {
                    RGBA_SURFACE
                };
                if is_sel {
                    push_rounded_glass_box(
                        &mut out,
                        shape_cache,
                        renderer,
                        Rectangle::new(Point::from((mx + 14, row_y)), Size::from((mw - 28, row_h))),
                        12,
                        row_rgba,
                        Some([215, 185, 255, 120]),
                    );
                } else {
                    push_rounded_glass_box(
                        &mut out,
                        shape_cache,
                        renderer,
                        Rectangle::new(Point::from((mx + 16, row_y)), Size::from((mw - 32, row_h))),
                        12,
                        row_rgba,
                        None,
                    );
                }
            }

            let modal_rect = Rectangle::new(Point::from((mx, my)), Size::from((mw, mh)));
            push_liquid_glass_panel(
                &mut out,
                shape_cache,
                renderer,
                modal_rect,
                WINDOW_RADIUS,
                crate::surfaces::GlassPanelStyle::Popover,
                glass_op,
                reduce_tr,
                Some((20, 58)),
            );
        }
        ShellOverlayView::QuickSettings(qs) => {
            render_quick_settings_composite(&mut out, shape_cache, renderer, width, height, qs);
        }
        ShellOverlayView::Switcher(sw) => {
            let (panel_rect, cards) = switcher_layout(screen_size, sw.cards.len());
            for (idx, (card_view, card)) in sw.cards.iter().zip(cards).enumerate() {
                let is_sel = idx == sw.selected;
                let icon_loc = Point::from((card.loc.x + (card.size.w - 36) / 2, card.loc.y + 18));
                let sw_key = format!("win-{}", card_view.window_id.0);
                if let Some(icon_el) = shape_cache.icons.window_icon_element(
                    renderer,
                    &sw_key,
                    card_view.toplevel_icon,
                    card_view.icon_name,
                    36,
                    icon_loc,
                ) {
                    out.push(resolved_icon_to_element(icon_el));
                } else {
                    let well_rect = Rectangle::new(
                        Point::from((card.loc.x + (card.size.w - 44) / 2, card.loc.y + 14)),
                        Size::from((44, 44)),
                    );
                    push_rounded_glass_box(
                        &mut out,
                        shape_cache,
                        renderer,
                        well_rect,
                        10,
                        if is_sel {
                            RGBA_ACCENT
                        } else {
                            RGBA_ACCENT_MUTED
                        },
                        None,
                    );
                }

                if card_view.minimized {
                    out.push(CompositorRenderElement::Solid(make_solid_element(
                        Rectangle::new(
                            Point::from((
                                card.loc.x + (card.size.w - 16) / 2,
                                card.loc.y + card.size.h - 6,
                            )),
                            Size::from((16, 2)),
                        ),
                        COLOR_WARNING,
                    )));
                }

                push_rounded_glass_box(
                    &mut out,
                    shape_cache,
                    renderer,
                    card,
                    10,
                    if is_sel {
                        RGBA_SURFACE_OVERLAY
                    } else {
                        RGBA_SURFACE
                    },
                    Some(if is_sel { RGBA_ACCENT } else { RGBA_BORDER }),
                );
            }

            if panel_rect.size.w > 0 {
                push_rounded_glass_box(
                    &mut out,
                    shape_cache,
                    renderer,
                    panel_rect,
                    14,
                    RGBA_SURFACE_ELEVATED,
                    Some(RGBA_BORDER_HIGHLIGHT),
                );
                push_soft_shadow(&mut out, shape_cache, renderer, panel_rect, 14, 14, 52);
            }
        }
        ShellOverlayView::Polkit(pk) => {
            render_polkit_composite(
                &mut out,
                shape_cache,
                renderer,
                screen_size,
                top_telemetry,
                pk,
            );
        }
        ShellOverlayView::Portal(portal) => {
            render_portal_composite(
                &mut out,
                shape_cache,
                renderer,
                screen_size,
                top_telemetry,
                portal,
            );
        }
        ShellOverlayView::PickColor(_) => {
            render_pick_color_composite(
                &mut out,
                shape_cache,
                renderer,
                screen_size,
                top_telemetry,
            );
        }
    }

    // 2b. Floating top-right Notification HUD toasts (`RUSTYOS-024`/`051`)
    if !notifications_hidden_by_overlay(overlay) && !notifications.is_empty() {
        let rects = notification_toast_rects(screen_size, notifications.len());
        for (toast, card) in notifications.iter().zip(rects) {
            let tx = card.loc.x;
            let ty = card.loc.y;
            let tw = card.size.w;

            if toast.hovered {
                if let Some(el) = shape_cache.ui_glyph_element(
                    renderer,
                    crate::surfaces::UiGlyphKind::Close,
                    10,
                    [248, 244, 255, 255],
                    Point::from((tx + tw - 23, ty + 15)),
                ) {
                    out.push(CompositorRenderElement::Memory(el));
                }
                if let Some(el) = shape_cache.circle_element(
                    renderer,
                    20,
                    [255, 255, 255, 42],
                    Some([255, 255, 255, 76]),
                    Point::from((tx + tw - 28, ty + 10)),
                ) {
                    out.push(CompositorRenderElement::Memory(el));
                }
            }

            let app_icon = notification_app_icon(toast.title);
            if let Some(icon_el) = shape_cache.icons.builtin_squircle_icon_element(
                renderer,
                app_icon,
                38,
                Point::from((tx + 14, ty + 15)),
            ) {
                out.push(CompositorRenderElement::Memory(icon_el));
            }

            push_liquid_glass_panel(
                &mut out,
                shape_cache,
                renderer,
                card,
                MENU_POPOVER_RADIUS,
                crate::surfaces::GlassPanelStyle::Popover,
                top_telemetry.glass_opacity,
                top_telemetry.reduce_transparency,
                Some((12, 46)),
            );
        }
    }

    out
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn build_chrome_shapes(
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    active_workspace: u8,
    layout_mode: &str,
    dock_view: &DockViewState<'_>,
    top_telemetry: &TopBarTelemetry<'_>,
    overlay: ShellOverlayView<'_>,
) -> Vec<CompositorRenderElement> {
    let mut out = Vec::new();
    let width = screen_size.w;
    let height = screen_size.h;

    // 3. TopBar macOS Menu Bar SDF glyphs, optional tray/workspaces, menu popover & transparent default (`RUSTYOS-048`, `RUSTYOS-049`)
    let active_app_name = dock_view
        .windows
        .iter()
        .find(|w| w.focused)
        .map_or_else(|| "RustyOS".to_owned(), |w| extract_app_name(&w.title));
    let opts = top_telemetry.layout_options();
    let left_layout =
        compute_top_bar_left_layout_with_options(&active_app_name, top_telemetry.show_workspaces);
    let right_layout =
        compute_top_bar_right_layout_with_options(width, top_telemetry.clock_hhmm, opts);

    let pure_white = [252, 250, 255, 255];
    let logo_loc = Point::from((TOP_BAR_LOGO_X + 1, TOP_BAR_ITEM_Y + 1));
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        crate::surfaces::UiGlyphKind::RustyLogo,
        18,
        pure_white,
        logo_loc,
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    push_top_bar_right_glyphs(
        &mut out,
        shape_cache,
        renderer,
        &right_layout,
        top_telemetry,
    );

    if let Some(open_menu) = top_telemetry.active_menu {
        let (anchor_x, anchor_w) = match open_menu {
            TopBarMenuHit::SystemLogo => (TOP_BAR_LOGO_X - 2, TOP_BAR_LOGO_W + 4),
            TopBarMenuHit::ActiveApp => (left_layout.app_x - 2, left_layout.app_w + 4),
            TopBarMenuHit::File => (left_layout.file_x - 2, left_layout.file_w + 4),
            TopBarMenuHit::Edit => (left_layout.edit_x - 2, left_layout.edit_w + 4),
            TopBarMenuHit::View => (left_layout.view_x - 2, left_layout.view_w + 4),
            TopBarMenuHit::Go => (left_layout.go_x - 2, left_layout.go_w + 4),
            TopBarMenuHit::Window => (left_layout.win_x - 2, left_layout.win_w + 4),
            TopBarMenuHit::Help => (left_layout.help_x - 2, left_layout.help_w + 4),
        };
        push_rounded_glass_box(
            &mut out,
            shape_cache,
            renderer,
            Rectangle::new(
                Point::from((anchor_x, TOP_BAR_ITEM_Y)),
                Size::from((anchor_w, TOP_BAR_ITEM_H)),
            ),
            5,
            [255, 255, 255, 42],
            None,
        );
        let (mx, my, mw, mh) = top_bar_menu_popover_rect(&active_app_name, open_menu);
        let menu_rect = Rectangle::new(Point::from((mx, my)), Size::from((mw, mh)));
        let mut row_y = my + MENU_POPOVER_PAD_Y;
        for (entry_idx, entry) in top_bar_menu_entries(open_menu).iter().enumerate() {
            match entry {
                TopBarMenuEntry::Item { icon, .. } => {
                    let hovered = top_telemetry.menu_hover_row == Some(entry_idx);
                    let icon_tint = if hovered {
                        [255, 252, 255, 255]
                    } else {
                        [218, 208, 238, 240]
                    };
                    if let Some(el) = shape_cache.ui_glyph_element(
                        renderer,
                        *icon,
                        14,
                        icon_tint,
                        Point::from((mx + 14, row_y + 7)),
                    ) {
                        out.push(CompositorRenderElement::Memory(el));
                    }
                    if hovered {
                        push_rounded_glass_box(
                            &mut out,
                            shape_cache,
                            renderer,
                            Rectangle::new(
                                Point::from((mx + 6, row_y + 1)),
                                Size::from((mw - 12, MENU_ITEM_HEIGHT - 2)),
                            ),
                            8,
                            [165, 110, 255, 200],
                            Some([215, 185, 255, 110]),
                        );
                    }
                    row_y += MENU_ITEM_HEIGHT;
                }
                TopBarMenuEntry::Separator => {
                    out.push(CompositorRenderElement::Solid(make_solid_element(
                        Rectangle::new(Point::from((mx + 12, row_y + 4)), Size::from((mw - 24, 1))),
                        COLOR_BORDER,
                    )));
                    row_y += MENU_SEPARATOR_HEIGHT;
                }
            }
        }
        push_liquid_glass_panel(
            &mut out,
            shape_cache,
            renderer,
            menu_rect,
            MENU_POPOVER_RADIUS,
            crate::surfaces::GlassPanelStyle::Popover,
            top_telemetry.glass_opacity,
            top_telemetry.reduce_transparency,
            Some((16, 52)),
        );
    }

    if top_telemetry.show_tray {
        let tray_rects = tray_slot_rects_with_options(
            width,
            top_telemetry.clock_hhmm,
            opts,
            top_telemetry.tray_items.len(),
        );
        for (item, (tx, ty, tw, th)) in top_telemetry.tray_items.iter().zip(tray_rects) {
            if item.attention || !item.passive {
                let dot_color = if item.attention {
                    COLOR_DANGER
                } else {
                    COLOR_ACCENT
                };
                out.push(CompositorRenderElement::Solid(make_solid_element(
                    Rectangle::new(Point::from((tx + tw - 5, ty + 2)), Size::from((3, 3))),
                    dot_color,
                )));
            }
            let icon_loc = Point::from((tx + (tw - 16) / 2, ty + (th - 16) / 2));
            let tray_icon_el = item
                .icon_pixmap
                .and_then(|pm| {
                    shape_cache
                        .icons
                        .icon_element_from_pixmap(renderer, item.key, pm, 16, icon_loc)
                })
                .or_else(|| {
                    (!item.icon_name.is_empty()).then(|| {
                        shape_cache.icons.icon_element_by_name(
                            renderer,
                            item.icon_name,
                            16,
                            icon_loc,
                        )
                    })?
                });
            if let Some(el) = tray_icon_el {
                out.push(CompositorRenderElement::Memory(el));
            }
            if item.attention {
                push_rounded_glass_box(
                    &mut out,
                    shape_cache,
                    renderer,
                    Rectangle::new(Point::from((tx, ty)), Size::from((tw, th))),
                    10,
                    [229, 107, 117, 40],
                    Some(RGBA_DANGER),
                );
            }
        }
    }

    if top_telemetry.notification_count > 0 {
        push_rounded_glass_box(
            &mut out,
            shape_cache,
            renderer,
            Rectangle::new(
                Point::from((right_layout.notif_x, TOP_BAR_ITEM_Y)),
                Size::from((right_layout.notif_w, TOP_BAR_ITEM_H)),
            ),
            10,
            RGBA_ACCENT_MUTED,
            Some(RGBA_ACCENT),
        );
    }
    if top_telemetry.quick_settings_open {
        push_rounded_glass_box(
            &mut out,
            shape_cache,
            renderer,
            Rectangle::new(
                Point::from((right_layout.control_center_x - 5, TOP_BAR_ITEM_Y)),
                Size::from((right_layout.control_center_w + 10, TOP_BAR_ITEM_H)),
            ),
            6,
            [255, 255, 255, 58],
            Some([255, 255, 255, 42]),
        );
    }

    if top_telemetry.show_workspaces {
        out.push(CompositorRenderElement::Solid(make_solid_element(
            Rectangle::new(
                Point::from((left_layout.separator_x, 6)),
                Size::from((1, 12)),
            ),
            COLOR_BORDER,
        )));

        let ws_y = TOP_BAR_ITEM_Y;
        let ws_h = TOP_BAR_ITEM_H;
        let mut start_x = left_layout.ws_start_x;
        for ws in 1..=9 {
            if ws == active_workspace {
                push_rounded_glass_box(
                    &mut out,
                    shape_cache,
                    renderer,
                    Rectangle::new(
                        Point::from((start_x, ws_y)),
                        Size::from((WS_BTN_WIDTH, ws_h)),
                    ),
                    9,
                    RGBA_ACCENT,
                    None,
                );
            }
            start_x += WS_BTN_STRIDE;
        }

        push_rounded_glass_box(
            &mut out,
            shape_cache,
            renderer,
            Rectangle::new(
                Point::from((left_layout.layout_x, ws_y)),
                Size::from((left_layout.layout_w, ws_h)),
            ),
            9,
            [255, 255, 255, 14],
            (layout_mode == "master-stack").then_some(RGBA_ACCENT),
        );
    }

    // Fully transparent TopBar by default (`reduce_transparency == false`);
    // render solid high-contrast bar only when `reduce_transparency` is enabled (`RUSTYOS-049`).
    if top_telemetry.reduce_transparency {
        out.push(CompositorRenderElement::Solid(make_solid_element(
            Rectangle::new(Point::from((0, TOP_BAR_HEIGHT - 1)), Size::from((width, 1))),
            COLOR_BORDER,
        )));
        out.push(CompositorRenderElement::Solid(make_solid_element(
            Rectangle::new(Point::from((0, 0)), Size::from((width, TOP_BAR_HEIGHT))),
            rgba_to_premul_color32f([20, 15, 32, 245]),
        )));
    }

    // 4. Dock macOS glass container, 4x4 SSAA icons, soft drop shadows, hover magnification, tooltip & divider (`RUSTYOS-050`)
    if !dock_tooltip_hidden_by_overlay(overlay)
        && let Some((tooltip_rect, _)) = dock_tooltip_rect_and_label(width, height, dock_view)
    {
        push_rounded_glass_box(
            &mut out,
            shape_cache,
            renderer,
            tooltip_rect,
            9,
            scale_glass_rgba(
                [42, 34, 62, 224],
                top_telemetry.glass_opacity,
                top_telemetry.reduce_transparency,
            ),
            Some(RGBA_BORDER_HIGHLIGHT),
        );
        push_soft_shadow(&mut out, shape_cache, renderer, tooltip_rect, 9, 8, 34);
    }

    let (dock_x, dock_y, dock_width, dock_items_count) =
        dock_layout_bounds(width, height, dock_view.windows.len());
    let left_count = DOCK_PINNED_COUNT + dock_view.windows.len();
    let _ = dock_view.pinned_icons;

    let mut dock_icons = Vec::with_capacity(dock_items_count);
    let mut dock_dots_and_divider = Vec::new();

    if let Some(last_app_slot) =
        dock_slot_rect(width, height, dock_view.windows.len(), left_count - 1)
    {
        let sep_x = last_app_slot.loc.x
            + last_app_slot.size.w
            + i32::midpoint(DOCK_ITEM_GAP, DOCK_DIVIDER_GAP);
        dock_dots_and_divider.push(CompositorRenderElement::Solid(make_solid_element(
            Rectangle::new(Point::from((sep_x, dock_y + 15)), Size::from((1, 38))),
            rgba_to_premul_color32f([255, 255, 255, 64]),
        )));
    }

    for i in 0..dock_items_count {
        let Some(slot_rect) = dock_slot_rect(width, height, dock_view.windows.len(), i) else {
            continue;
        };
        let item_x = slot_rect.loc.x;
        let item_size = slot_rect.size.w;
        let hovered = dock_view.hover_slot == Some(i);
        let hover_factor = if hovered {
            if dock_view.reduced_motion {
                0.25
            } else {
                dock_view.hover_progress.clamp(0.35, 1.0)
            }
        } else {
            0.0
        };
        #[allow(clippy::cast_possible_truncation)]
        let lift = if hovered {
            if dock_view.reduced_motion {
                2
            } else {
                (6.0 * hover_factor).round() as i32
            }
        } else {
            0
        };
        #[allow(clippy::cast_possible_truncation)]
        let extra_half = if hovered {
            ((4.0 * hover_factor).round() as i32).clamp(0, 4)
        } else {
            0
        };
        let draw_size = DOCK_ICON_SIZE + extra_half * 2;
        let icon_x = item_x + (item_size - draw_size) / 2;
        let icon_y = (dock_y + 7) - extra_half - lift;
        let icon_loc = Point::from((icon_x, icon_y));

        let win_entry = (i >= DOCK_PINNED_COUNT && i < left_count)
            .then(|| dock_view.windows.get(i - DOCK_PINNED_COUNT))
            .flatten();
        let is_minimized = win_entry.is_some_and(|w| w.minimized);
        let is_active = match i {
            0 => true,
            1 => dock_view.overview_active,
            _ => win_entry.is_some_and(|w| w.focused && !w.minimized),
        };
        let has_running_dot =
            win_entry.is_some() || i == 0 || (i == 1 && dock_view.overview_active);
        if has_running_dot {
            let pip_color = if is_minimized {
                [232, 196, 119, 235]
            } else if is_active {
                [248, 246, 255, 240]
            } else {
                [230, 224, 245, 165]
            };
            if let Some(dot) = shape_cache.circle_element(
                renderer,
                5,
                pip_color,
                None,
                Point::from((item_x + (item_size - 5) / 2, dock_y + DOCK_HEIGHT - 8)),
            ) {
                dock_dots_and_divider.push(CompositorRenderElement::Memory(dot));
            }
        }

        let builtin_for_slot = match i {
            0 => Some(crate::icons::BuiltinAppIcon::Files),
            1 => Some(crate::icons::BuiltinAppIcon::Overview),
            2 => Some(crate::icons::BuiltinAppIcon::Terminal),
            3 => Some(crate::icons::BuiltinAppIcon::Browser),
            4 => Some(crate::icons::BuiltinAppIcon::Notes),
            5 => Some(crate::icons::BuiltinAppIcon::Settings),
            n if n == left_count => Some(crate::icons::BuiltinAppIcon::Downloads),
            n if n == left_count + 1 => Some(crate::icons::BuiltinAppIcon::Trash),
            _ => None,
        };
        let slot_icon_el = if let Some(kind) = builtin_for_slot {
            shape_cache
                .icons
                .builtin_squircle_icon_element(renderer, kind, draw_size, icon_loc)
                .map(|el| crate::icons::ResolvedIconElement::Memory(Box::new(el)))
        } else {
            win_entry.and_then(|w| {
                let key = format!("win-{}", w.id.0);
                shape_cache
                    .icons
                    .window_icon_element(
                        renderer,
                        &key,
                        w.toplevel_icon.as_ref(),
                        w.icon_name.as_deref(),
                        draw_size,
                        icon_loc,
                    )
                    .or_else(|| {
                        let hue_seed = title_hue_seed(&w.title, w.id.0);
                        shape_cache
                            .icons
                            .builtin_squircle_icon_element(
                                renderer,
                                crate::icons::BuiltinAppIcon::WindowFallback {
                                    hue_seed,
                                    active: is_active,
                                },
                                draw_size,
                                icon_loc,
                            )
                            .map(|el| crate::icons::ResolvedIconElement::Memory(Box::new(el)))
                    })
            })
        };
        if let Some(el) = slot_icon_el {
            dock_icons.push(resolved_icon_to_element(el));
        }
    }

    out.extend(dock_icons);
    out.extend(dock_dots_and_divider);

    if let Some(dock_glass) = shape_cache.dock_glass_panel_element(
        renderer,
        dock_width,
        DOCK_HEIGHT,
        DOCK_RADIUS,
        top_telemetry.glass_opacity,
        top_telemetry.reduce_transparency,
        Point::from((dock_x, dock_y)),
    ) {
        out.push(CompositorRenderElement::Memory(dock_glass));
    }

    out
}

#[allow(clippy::too_many_lines)]
fn build_overview_shapes(
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    top_telemetry: &TopBarTelemetry<'_>,
    ov: &OverviewRenderState<'_>,
) -> Vec<CompositorRenderElement> {
    let mut out = Vec::new();
    let width = screen_size.w;
    let height = screen_size.h;

    // 5. Workspace Overview Spaces strip, floating cards & light scrim (`RUSTYOS-023`/`025`/`051`)
    let heading_dy = anim_offset(ov.progress, 12.0);
    let card_dy = anim_offset(ov.progress, 22.0);

    if !ov.spaces.is_empty() {
        let space_rects = overview_space_rects(screen_size, ov.spaces.len());
        for (space, base_thumb) in ov.spaces.iter().zip(space_rects) {
            let thumb = Rectangle::new(
                Point::from((base_thumb.loc.x, base_thumb.loc.y + heading_dy)),
                base_thumb.size,
            );
            for &(nx, ny, nw, nh) in &space.preview_rects {
                if nw <= 0.0 || nh <= 0.0 {
                    continue;
                }
                #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
                let (rx, ry, rw, rh) = (
                    thumb.loc.x + 4 + ((thumb.size.w - 8) as f32 * nx).round() as i32,
                    thumb.loc.y + 4 + ((thumb.size.h - 8) as f32 * ny).round() as i32,
                    (((thumb.size.w - 8) as f32 * nw).round() as i32).max(6),
                    (((thumb.size.h - 8) as f32 * nh).round() as i32).max(5),
                );
                push_rounded_glass_box(
                    &mut out,
                    shape_cache,
                    renderer,
                    Rectangle::new(Point::from((rx, ry)), Size::from((rw, rh))),
                    3,
                    [224, 212, 248, 185],
                    Some([255, 255, 255, 90]),
                );
            }
            let thumb_border = if space.active {
                [185, 138, 255, 255]
            } else {
                RGBA_BORDER
            };
            let thumb_bg = if space.active {
                [72, 42, 124, 228]
            } else if space.window_count > 0 {
                [50, 32, 86, 214]
            } else {
                [36, 24, 62, 196]
            };
            push_rounded_glass_box(
                &mut out,
                shape_cache,
                renderer,
                thumb,
                8,
                thumb_bg,
                Some(thumb_border),
            );
            push_soft_shadow(&mut out, shape_cache, renderer, thumb, 8, 8, 32);
        }
        out.push(CompositorRenderElement::Solid(make_solid_element(
            Rectangle::new(Point::from((0, TOP_BAR_HEIGHT)), Size::from((width, 98))),
            premul(0.08, 0.05, 0.14, 0.28),
        )));
    }

    let cards = overview_card_rects(screen_size, ov.windows.len());
    for (win, base_card) in ov.windows.iter().zip(cards) {
        let card = Rectangle::new(
            Point::from((base_card.loc.x, base_card.loc.y + card_dy)),
            base_card.size,
        );
        push_liquid_glass_panel(
            &mut out,
            shape_cache,
            renderer,
            card,
            14,
            crate::surfaces::GlassPanelStyle::Popover,
            top_telemetry.glass_opacity,
            top_telemetry.reduce_transparency,
            None,
        );
        if win.focused {
            push_rounded_glass_box(
                &mut out,
                shape_cache,
                renderer,
                Rectangle::new(
                    Point::from((card.loc.x - 2, card.loc.y - 2)),
                    Size::from((card.size.w + 4, card.size.h + 4)),
                ),
                15,
                [165, 110, 255, 68],
                Some([198, 156, 255, 245]),
            );
        }
        push_soft_shadow(&mut out, shape_cache, renderer, card, 14, 16, 50);
    }
    #[allow(clippy::cast_possible_truncation)]
    let scrim_a = 0.36 * (ov.progress.clamp(0.0, 1.0) as f32);
    out.push(CompositorRenderElement::Solid(make_solid_element(
        Rectangle::new(
            Point::from((0, TOP_BAR_HEIGHT)),
            Size::from((width, (height - TOP_BAR_HEIGHT).max(0))),
        ),
        premul(0.059, 0.039, 0.098, scrim_a),
    )));

    out
}

#[cfg(test)]
mod tests;
