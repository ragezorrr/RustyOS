//! Visual shell elements rendering (`TopBar`, `Dock`, Desktop background) (`RUSTYOS-012`, ADR-0007).
//!
//! Renders macOS-inspired top status bar and bottom floating dock using Smithay's
//! solid color render elements, adhering to design tokens in `DESIGN.md` ("RustyOS Light & Dark").

use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
use smithay::backend::renderer::element::solid::SolidColorRenderElement;
use smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement;
use smithay::backend::renderer::element::texture::TextureRenderElement;
use smithay::backend::renderer::element::utils::{RelocateRenderElement, RescaleRenderElement};
use smithay::backend::renderer::gles::{GlesRenderer, GlesTexture};
use smithay::utils::Size;

smithay::backend::renderer::element::render_elements! {
    /// Unified render element type emitted across shell, SSD frames, text, and Wayland surfaces.
    pub CompositorRenderElement<=GlesRenderer>;
    /// Client Wayland surface element (raw/unclipped for fullscreen Direct Scanout).
    Surface = WaylandSurfaceRenderElement<GlesRenderer>,
    /// Client Wayland surface element clipped to unified cubic squircle corners (`RUSTYOS-128`).
    ClippedSurface = crate::surfaces::ClippedSurfaceElement,
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
    /// Hardware Dual-Kawase Liquid Glass panel element (`RUSTYOS-069`).
    ShaderTexture = crate::surfaces::LiquidGlassElement,
    /// Relocated Liquid Glass panel element aligned to HiDPI physical output coordinates (`RUSTYOS-069`).
    ScaledShaderTexture = RelocateRenderElement<crate::surfaces::LiquidGlassElement>,
}

mod chrome_shapes;
mod context_menu;
mod desktop_canvas;
mod dock;
mod dock_presentation;
mod dock_stack;
mod lock_screen;
mod modals;
mod notification_center;
mod overlay_shapes;
mod quick_settings;
mod spotlight_overview;
mod ssd;
mod text_pass;
mod tokens;
mod top_bar;
mod top_bar_menus;
mod topbar_popovers;
mod updates_popover;
mod wifi_popover;

pub use chrome_shapes::{
    generate_dock_only_chrome_elements, generate_shell_chrome_composite_elements,
    generate_shell_overview_elements,
};
pub use overlay_shapes::generate_shell_foreground_overlay_elements;
pub use text_pass::generate_shell_text_elements;

pub use context_menu::{
    ContextMenuAction, ContextMenuEntry, ContextMenuHit, ContextMenuKeyOutcome, ContextMenuLayout,
    ContextMenuState, ContextMenuSubmenuLayout, ContextMenuTarget, compute_context_menu_layout,
    generate_context_menu_elements, handle_context_menu_key, hit_test_context_menu,
    update_context_menu_hover,
};
pub use desktop_canvas::{
    DesktopCanvasItem, DesktopDragState, DesktopInlineRename, DesktopRubberBand, cell_at_point,
    compute_desktop_cell_rect, compute_desktop_item_rect, hit_test_desktop_item,
    load_desktop_grid_positions, render_desktop_canvas_elements, save_desktop_grid_positions,
};
pub use dock::{
    ComputedDockLayout, ComputedDockSlot, DOCK_REVEAL_STRIP_PX, DockAppTile, DockDragPreview,
    DockHit, DockSection, DockViewState, DockWindowEntry, append_utility_dock_tiles,
    canonical_dock_app_key, check_dock_click, check_dock_layout_slot, check_dock_slot,
    compute_dock_layout, dock_builtin_for_key, dock_count_badge_rect, dock_hit_for_pinned_key,
    dock_label_for_key, dock_obstruction_region, dock_slot_rect, extract_app_name,
    is_dock_pin_removable, is_pointer_in_dock_auto_hide_zone, make_window_badge,
};
pub use dock_presentation::{
    DOCK_LEAVE_DURATION, DOCK_PRESENTATION_DURATION, DockPresentation, dock_tile_presentation_key,
};
pub use dock_stack::{
    DockStackEntry, DockStackHit, DockStackItemSlot, DockStackLayout, DockStackRenderState,
    compute_dock_stack_layout, hit_test_dock_stack,
};
#[cfg(test)]
pub(crate) use lock_screen::lock_screen_primary_row_center;
pub use lock_screen::{
    LockScreenHit, LockScreenRenderState, check_lock_screen_unlock_click,
    generate_lock_screen_elements, hit_test_lock_screen,
};
pub use modals::{
    PolkitHit, PolkitPromptRenderState, PortalHit, PortalPromptRenderState, PortalSourceRow,
    ScreenshotHudHit, ScreenshotHudState, ScreenshotMode, hit_test_polkit_dialog,
    hit_test_portal_dialog, hit_test_screenshot_hud, normalize_selection_rect,
    screenshot_hud_pill_rect,
};
pub use notification_center::{
    DEFAULT_NOTIFICATION_TASKS, NotificationCardSlot, NotificationCenterHit,
    NotificationCenterItemView, NotificationCenterLayout, NotificationCenterRenderState,
    compute_notification_center_layout, hit_test_notification_center,
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
    generate_window_shadow_elements_with_opacity, generate_window_ssd_composite_elements,
    generate_window_ssd_composite_elements_with_hover, generate_window_ssd_elements,
};
pub(crate) use tokens::push_liquid_glass_panel_elevation;
#[cfg(test)]
pub(crate) use tokens::scale_glass_rgba;
pub use tokens::{
    COLOR_ACCENT, COLOR_ACCENT_HOVER, COLOR_ACCENT_MUTED, COLOR_BACKGROUND, COLOR_BORDER,
    COLOR_BORDER_HIGHLIGHT, COLOR_DANGER, COLOR_MUTED, COLOR_SUBTLE, COLOR_SUCCESS, COLOR_SURFACE,
    COLOR_SURFACE_ELEVATED, COLOR_SURFACE_OVERLAY, COLOR_TEXT, COLOR_WARNING, DOCK_DIVIDER_GAP,
    DOCK_HEIGHT, DOCK_ICON_SIZE, DOCK_ITEM_GAP, DOCK_ITEM_SIZE, DOCK_MARGIN_BOTTOM, DOCK_PADDING,
    DOCK_PINNED_COUNT, DOCK_RADIUS, DOCK_UTILITY_COUNT, RESIZE_GRAB_BORDER, TITLEBAR_HEIGHT,
    TOP_BAR_APP_X, TOP_BAR_FONT_SIZE, TOP_BAR_HEIGHT, TOP_BAR_ITEM_H, TOP_BAR_ITEM_Y,
    TOP_BAR_LOGO_W, TOP_BAR_LOGO_X, WINDOW_RADIUS, WS_BTN_GAP, WS_BTN_STRIDE, WS_BTN_WIDTH,
    compute_dock_radius, usable_work_area, usable_work_area_for_output, usable_work_area_with_dock,
};
pub use top_bar::{
    BarBackdropStyle, analyze_top_bar_backdrop_bgra, check_top_bar_menu_click_with_app_menu,
    check_top_bar_menu_popover_click_with_app_menu, dynamic_app_menu_slots,
    prefers_light_foreground, relative_luminance, resolve_top_bar_menu_entries_with_app_menu,
    top_bar_menu_popover_rect_with_app_menu,
};
pub use top_bar::{
    MAX_TOP_BAR_TRAY_ITEMS, MAX_VISIBLE_NOTIFICATIONS, MENU_POPOVER_RADIUS, NotificationToastView,
    OsdRenderState, TopBarLayoutOptions, TopBarLeftLayout, TopBarMenuAction, TopBarMenuEntry,
    TopBarMenuHit, TopBarMenuPopoverClick, TopBarRightLayout, TopBarTelemetry, TopBarTrayItem,
    check_notification_click, check_top_bar_battery_click_for_options,
    check_top_bar_bluetooth_click_for_options, check_top_bar_clock_click_for_options,
    check_top_bar_kbd_click, check_top_bar_kbd_click_for_options,
    check_top_bar_kbd_click_for_telemetry, check_top_bar_layout_click,
    check_top_bar_layout_click_for_app, check_top_bar_layout_click_for_profile,
    check_top_bar_layout_click_with_options, check_top_bar_menu_click,
    check_top_bar_menu_click_for_app, check_top_bar_menu_click_for_profile,
    check_top_bar_menu_popover_click, check_top_bar_menu_popover_click_for_profile,
    check_top_bar_menu_popover_hover_row, check_top_bar_menu_popover_hover_row_for_profile,
    check_top_bar_notification_click, check_top_bar_notification_click_for_options,
    check_top_bar_notification_click_for_telemetry, check_top_bar_recording_click_for_options,
    check_top_bar_search_click_for_options, check_top_bar_search_click_for_telemetry,
    check_top_bar_status_click, check_top_bar_status_click_for_options,
    check_top_bar_status_click_for_telemetry, check_top_bar_tray_click,
    check_top_bar_tray_click_for_options, check_top_bar_tray_click_for_telemetry,
    check_top_bar_updates_click_for_options, check_top_bar_volume_click_for_options,
    check_top_bar_wifi_click_for_options, check_top_bar_workspace_click,
    check_top_bar_workspace_click_for_app, check_top_bar_workspace_click_for_profile,
    check_top_bar_workspace_click_with_options, compute_top_bar_left_layout,
    compute_top_bar_left_layout_for_profile, compute_top_bar_left_layout_with_options,
    compute_top_bar_right_layout, compute_top_bar_right_layout_with_options, generate_osd_elements,
    notification_toast_rects, osd_pill_rect, top_bar_menu_entries, top_bar_menu_items,
    top_bar_menu_popover_rect, top_bar_menu_popover_rect_for_profile, tray_slot_rects,
    tray_slot_rects_with_layout, tray_slot_rects_with_options,
};
pub use top_bar_menus::{
    AppShortcutAction, ResolvedTopBarMenuItem, TopBarAppProfile, classify_top_bar_app,
    resolve_top_bar_menu_entries,
};
pub use topbar_popovers::{
    TopBarQuickPopoverHit, TopBarQuickPopoverKind, TopBarQuickPopoverLayout,
    TopBarQuickPopoverState, compute_topbar_quick_popover_layout, hit_test_topbar_quick_popover,
};
pub use updates_popover::{
    MAX_POPOVER_UPDATE_ITEMS, UpdatesPopoverHit, UpdatesPopoverLayout, UpdatesRenderState,
    compute_updates_popover_layout, hit_test_updates_popover,
};
pub use wifi_popover::{
    WifiPopoverHit, WifiPopoverLayout, WifiPopoverRenderState, WifiPopoverRowSlot,
    compute_wifi_popover_layout, hit_test_wifi_popover,
};

use dock::render_dock;
use quick_settings::render_quick_settings_popover;
use spotlight_overview::render_spotlight_modal;
pub(crate) use tokens::{
    active_accent_index, active_appearance, begin_solid_element_frame, make_solid_element,
    set_active_accent_index, set_active_appearance,
};
use top_bar::render_top_bar;

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
    /// Interactive Screenshot HUD (`Region / Window / Full Screen`, `RUSTYOS-133`).
    ScreenshotHud(&'a ScreenshotHudState),
    /// Context menu rendered with Liquid Glass (`RUSTYOS-064`).
    ContextMenu(&'a ContextMenuState),
    /// Graphical Downloads Stack popover (`RUSTYOS-073`).
    DockStack(&'a DockStackRenderState<'a>),
    /// Software Updates popover anchored to `TopBar` (`RUSTYOS-076`).
    Updates(&'a UpdatesRenderState<'a>),
    /// Wi-Fi & Ethernet `Liquid Glass` popover anchored to `TopBar` (`RUSTYOS-108`).
    Wifi(&'a WifiPopoverRenderState<'a>),
    /// Compact `TopBar` popover for Volume, Battery, or Bluetooth (`RUSTYOS-141`).
    TopBarQuickPopover(&'a TopBarQuickPopoverState<'a>),
    /// Right-edge `Notification Center` stack and widgets (`RUSTYOS-109`).
    NotificationCenter(&'a NotificationCenterRenderState<'a>),
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
        | ShellOverlayView::PickColor(_)
        | ShellOverlayView::ScreenshotHud(_)
        | ShellOverlayView::ContextMenu(_)
        | ShellOverlayView::DockStack(_)
        | ShellOverlayView::Updates(_)
        | ShellOverlayView::Wifi(_)
        | ShellOverlayView::TopBarQuickPopover(_)
        | ShellOverlayView::NotificationCenter(_) => {}
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

pub(super) const TEXT_MUTED: [u8; 4] = [148, 148, 156, 255]; // #94949C

pub(super) const fn notifications_hidden_by_overlay(overlay: ShellOverlayView<'_>) -> bool {
    matches!(
        overlay,
        ShellOverlayView::Spotlight(_)
            | ShellOverlayView::QuickSettings(_)
            | ShellOverlayView::Polkit(_)
            | ShellOverlayView::Portal(_)
            | ShellOverlayView::ContextMenu(_)
            | ShellOverlayView::DockStack(_)
            | ShellOverlayView::Updates(_)
            | ShellOverlayView::Wifi(_)
            | ShellOverlayView::TopBarQuickPopover(_)
            | ShellOverlayView::NotificationCenter(_)
    )
}

pub(super) const fn dock_tooltip_hidden_by_overlay(overlay: ShellOverlayView<'_>) -> bool {
    matches!(
        overlay,
        ShellOverlayView::Spotlight(_)
            | ShellOverlayView::QuickSettings(_)
            | ShellOverlayView::Switcher(_)
            | ShellOverlayView::Polkit(_)
            | ShellOverlayView::Portal(_)
            | ShellOverlayView::ContextMenu(_)
            | ShellOverlayView::DockStack(_)
            | ShellOverlayView::Updates(_)
            | ShellOverlayView::Wifi(_)
            | ShellOverlayView::TopBarQuickPopover(_)
            | ShellOverlayView::NotificationCenter(_)
    )
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

#[cfg(test)]
mod tests;
