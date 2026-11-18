//! Text rendering passes for foreground overlays and persistent desktop chrome (`RUSTYOS-101`).

use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::{Point, Rectangle, Size};

use super::context_menu::render_context_menu_text;
use super::dock::dock_tooltip_rect_and_label;
use super::dock_stack::render_dock_stack_text;
use super::modals::{
    render_pick_color_text, render_polkit_text, render_portal_text, render_settings_text,
};
use super::quick_settings::render_quick_settings_text;
use super::spotlight_overview::{
    render_overview_text, render_switcher_text, spotlight_results_rect, spotlight_search_pill_rect,
};
use super::tokens::{ThemePalette, anim_offset, truncate_ascii};
use super::top_bar::{MENU_ITEM_HEIGHT, MENU_POPOVER_PAD_Y, MENU_SEPARATOR_HEIGHT};
use super::top_bar_menus::{ResolvedTopBarMenuItem, resolve_top_bar_menu_entries};
use super::updates_popover::render_updates_text;
use super::wifi_popover::render_wifi_popover_text;
use super::{
    DockViewState, NotificationToastView, ShellOverlayView, TEXT_BRIGHT, TEXT_MUTED,
    TOP_BAR_FONT_SIZE, TOP_BAR_ITEM_H, TOP_BAR_ITEM_Y, TopBarMenuHit, TopBarTelemetry,
    WS_BTN_STRIDE, WS_BTN_WIDTH, compute_dock_layout, compute_top_bar_left_layout_for_profile,
    compute_top_bar_right_layout_with_options, dock_tooltip_hidden_by_overlay,
    notification_toast_rects, notifications_hidden_by_overlay,
    top_bar_menu_popover_rect_for_profile, tray_slot_rects_with_options,
};

#[allow(clippy::too_many_lines)]
pub(super) fn append_foreground_overlay_text(
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
    let pal = ThemePalette::active();

    match overlay {
        ShellOverlayView::None | ShellOverlayView::Overview(_) => {}
        ShellOverlayView::Spotlight(spot) => {
            let dy = anim_offset(spot.progress, -22.0);
            let (sx, search_top, _sw, _sh) = spotlight_search_pill_rect(width, height);
            let (rx, results_top, rw, _rh) = spotlight_results_rect(width, height);
            let sy = search_top + dy;
            let ry = results_top + dy;

            if spot.query.is_empty() {
                if let Some(el) = text_cache.render_label(
                    renderer,
                    "Spotlight Search",
                    14.5,
                    pal.popover_text_muted,
                    Point::from((sx + 50, sy + 18)),
                ) {
                    elements.push(el);
                }
            } else if let Some(el) = text_cache.render_label(
                renderer,
                spot.query,
                14.5,
                pal.popover_text_primary,
                Point::from((sx + 50, sy + 18)),
            ) {
                elements.push(el);
            }

            let start_row_y = ry + 14;
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
                    let badge_bounds =
                        Rectangle::new(Point::from((rx + 22, row_y + 9)), Size::from((28, 28)));
                    if let Some(el) = text_cache.render_label_centered(
                        renderer,
                        &item.badge,
                        11.0,
                        if is_sel {
                            [255, 255, 255, 255]
                        } else {
                            pal.popover_text_primary
                        },
                        badge_bounds,
                    ) {
                        elements.push(el);
                    }
                }

                let row_title_fg = if is_sel {
                    [255, 255, 255, 255]
                } else {
                    pal.popover_text_primary
                };
                let row_sub_fg = if is_sel {
                    [236, 242, 255, 240]
                } else {
                    pal.popover_text_muted
                };
                if let Some(el) = text_cache.render_label(
                    renderer,
                    &item.name,
                    13.5,
                    row_title_fg,
                    Point::from((rx + 60, row_y + 7)),
                ) {
                    elements.push(el);
                }
                if let Some(el) = text_cache.render_label(
                    renderer,
                    &item.subtitle,
                    10.5,
                    row_sub_fg,
                    Point::from((rx + 60, row_y + 26)),
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
                    Point::from((rx + rw - 98, row_y + 14)),
                    Size::from((80, 18)),
                );
                let cat_color = if is_sel {
                    [255, 255, 255, 255]
                } else {
                    pal.popover_text_muted
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
        ShellOverlayView::Settings(st) => {
            render_settings_text(text_cache, renderer, screen_size, st, elements);
        }
        ShellOverlayView::ContextMenu(ctx) => {
            render_context_menu_text(text_cache, renderer, screen_size, ctx, true, elements);
        }
        ShellOverlayView::DockStack(ds) => {
            render_dock_stack_text(text_cache, renderer, screen_size, ds, elements);
        }
        ShellOverlayView::Updates(up) => {
            render_updates_text(text_cache, renderer, screen_size, up, elements);
        }
        ShellOverlayView::Wifi(wifi) => {
            render_wifi_popover_text(text_cache, renderer, screen_size, wifi, elements);
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
                pal.popover_text_primary,
                Point::from((tx + 62, ty + 13)),
            ) {
                elements.push(el);
            }

            let body_text = truncate_ascii(toast.body, 36);
            if let Some(el) = text_cache.render_label(
                renderer,
                &body_text,
                10.5,
                pal.popover_text_muted,
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
pub(super) fn append_chrome_text(
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
    let pal = ThemePalette::active();
    let text_bright = TEXT_BRIGHT;
    let text_muted = TEXT_MUTED;

    // 1b. TopBar Left Active App & Global Menus (`RUSTYOS-038`, `RUSTYOS-048`, `RUSTYOS-049`)
    let ws_btn_height = TOP_BAR_ITEM_H;
    let ws_y = TOP_BAR_ITEM_Y;

    let active_app_name = top_telemetry.active_app_name;
    let app_profile = top_telemetry.app_profile;
    let opts = top_telemetry.layout_options();
    let left_layout = compute_top_bar_left_layout_for_profile(
        app_profile,
        active_app_name,
        top_telemetry.show_workspaces,
    );

    let app_bounds = Rectangle::new(
        Point::from((left_layout.app_x, ws_y)),
        Size::from((left_layout.app_w, ws_btn_height)),
    );
    let pure_white = [255, 255, 255, 255];
    if let Some(el) = text_cache.render_label_centered_weighted(
        renderer,
        active_app_name,
        crate::text::FontWeight::Bold,
        TOP_BAR_FONT_SIZE,
        pure_white,
        app_bounds,
    ) {
        elements.push(el);
    }

    let text_menu = [246, 247, 250, 245];
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
        if menu_kind == TopBarMenuHit::Go && !left_layout.has_go {
            continue;
        }
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

    // Active TopBar left dropdown menu labels + right-aligned real shortcuts (`RUSTYOS-048`/`049`/`051`/`068`)
    if let Some(open_menu) = top_telemetry
        .active_menu
        .filter(|m| *m != TopBarMenuHit::Go || left_layout.has_go)
    {
        let (mx, my, mw, _) =
            top_bar_menu_popover_rect_for_profile(app_profile, active_app_name, open_menu);
        let mut row_y = my + MENU_POPOVER_PAD_Y;
        for (entry_idx, entry) in
            resolve_top_bar_menu_entries(app_profile, active_app_name, open_menu)
                .into_iter()
                .enumerate()
        {
            match entry {
                ResolvedTopBarMenuItem::Item {
                    label,
                    icon,
                    shortcut,
                    ..
                } => {
                    let hovered = top_telemetry.menu_hover_row == Some(entry_idx);
                    let fg = if hovered {
                        [255, 255, 255, 255]
                    } else {
                        pal.popover_text_primary
                    };
                    let text_x = if icon.is_some() { mx + 35 } else { mx + 14 };
                    let (_, label_h) = text_cache.measure_text(&label, 13.0);
                    let label_y = row_y + (MENU_ITEM_HEIGHT - label_h) / 2;
                    if let Some(el) = text_cache.render_label(
                        renderer,
                        &label,
                        13.0,
                        fg,
                        Point::from((text_x, label_y)),
                    ) {
                        elements.push(el);
                    }
                    if let Some(hint) = shortcut {
                        let (hint_w, _) = text_cache.measure_text(hint, 12.0);
                        let hint_fg = if hovered {
                            [255, 255, 255, 255]
                        } else {
                            pal.popover_text_muted
                        };
                        if let Some(el) = text_cache.render_label(
                            renderer,
                            hint,
                            12.0,
                            hint_fg,
                            Point::from((mx + mw - 14 - hint_w, label_y + 1)),
                        ) {
                            elements.push(el);
                        }
                    }
                    row_y += MENU_ITEM_HEIGHT;
                }
                ResolvedTopBarMenuItem::Separator => {
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
                [214, 214, 222, 220]
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

    // 5. Dock hover tooltip & drag Remove pill label (`RUSTYOS-050` / `RUSTYOS-073`)
    if !dock_tooltip_hidden_by_overlay(overlay) {
        if dock_view.drag_preview.is_some_and(|d| d.pulled_outside) {
            let layout = compute_dock_layout(screen_size, dock_view);
            if let Some(badge_rect) = layout.drag_remove_badge
                && let Some(el) = text_cache.render_label_centered_weighted(
                    renderer,
                    "Remove",
                    crate::text::FontWeight::Bold,
                    11.5,
                    [255, 255, 255, 255],
                    Rectangle::new(
                        Point::from((badge_rect.loc.x, badge_rect.loc.y)),
                        Size::from((badge_rect.size.w, badge_rect.size.h)),
                    ),
                )
            {
                elements.push(el);
            }
        } else if let Some((tooltip_rect, tooltip_label)) =
            dock_tooltip_rect_and_label(width, height, dock_view)
            && let Some(el) = text_cache.render_label_centered_weighted(
                renderer,
                &tooltip_label,
                crate::text::FontWeight::Bold,
                12.0,
                pal.popover_text_primary,
                Rectangle::new(
                    Point::from((tooltip_rect.loc.x, tooltip_rect.loc.y)),
                    Size::from((tooltip_rect.size.w, tooltip_rect.size.h)),
                ),
            )
        {
            elements.push(el);
        }
    }
    let _ = shape_cache;
}
