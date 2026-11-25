//! Text rendering passes for foreground overlays and persistent desktop chrome (`RUSTYOS-101`).

use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::{Point, Rectangle, Size};

use super::context_menu::render_context_menu_text;
use super::dock::dock_tooltip_rect_and_label;
use super::dock_stack::render_dock_stack_text;
use super::modals::{
    render_pick_color_text, render_polkit_text, render_portal_text, render_screenshot_hud_text,
};
use super::notification_center::render_notification_center_text;
use super::quick_settings::render_quick_settings_text;
use super::spotlight_overview::{
    render_overview_text, render_switcher_text, spotlight_results_rect, spotlight_search_pill_rect,
};
use super::tokens::{ThemePalette, anim_offset};
use super::top_bar::{
    MENU_ITEM_HEIGHT, MENU_POPOVER_PAD_Y, MENU_SEPARATOR_HEIGHT, push_notification_card_text_lines,
};
use super::top_bar_menus::ResolvedTopBarMenuItem;
use super::updates_popover::render_updates_text;
use super::wifi_popover::render_wifi_popover_text;
use super::{
    DockViewState, NotificationToastView, ShellOverlayView, TEXT_MUTED, TOP_BAR_FONT_SIZE,
    TOP_BAR_ITEM_H, TOP_BAR_ITEM_Y, TopBarMenuHit, TopBarTelemetry, WS_BTN_STRIDE, WS_BTN_WIDTH,
    compute_dock_layout, compute_top_bar_left_layout_for_profile,
    compute_top_bar_right_layout_with_options, dock_tooltip_hidden_by_overlay,
    notification_toast_rects, notifications_hidden_by_overlay, tray_slot_rects_with_options,
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
        ShellOverlayView::ScreenshotHud(hud) => {
            render_screenshot_hud_text(text_cache, renderer, screen_size, hud, elements);
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
        ShellOverlayView::TopBarQuickPopover(pop) => {
            super::topbar_popovers::render_topbar_quick_popover_text(
                text_cache,
                renderer,
                screen_size,
                pop,
                elements,
            );
        }
        ShellOverlayView::NotificationCenter(nc) => {
            render_notification_center_text(text_cache, renderer, screen_size, nc, elements);
        }
    }

    if !notifications_hidden_by_overlay(overlay) && !notifications.is_empty() {
        let rects = notification_toast_rects(screen_size, notifications.len());
        for (toast, card) in notifications.iter().zip(rects) {
            push_notification_card_text_lines(
                text_cache,
                renderer,
                card,
                toast.title,
                toast.body,
                &pal,
                elements,
            );
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
        render_overview_text(
            text_cache,
            renderer,
            screen_size,
            ov,
            |id| shape_cache.has_lifecycle_snapshot(id),
            &mut elements,
        );
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
    let pal = ThemePalette::active();
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
    let right_layout =
        compute_top_bar_right_layout_with_options(width, top_telemetry.clock_hhmm, opts);
    let max_left_menu_x = right_layout.tray_anchor_x - 10;

    let app_bounds = Rectangle::new(
        Point::from((left_layout.app_x, ws_y)),
        Size::from((left_layout.app_w, ws_btn_height)),
    );
    let light_opaque_bar = top_telemetry.reduce_transparency && !pal.is_dark;
    let use_light_bar_ink = top_telemetry
        .backdrop
        .map_or(!light_opaque_bar, |b| b.prefer_light_text);
    let bar_fg = if use_light_bar_ink {
        [255, 255, 255, 255]
    } else {
        [28, 29, 34, 255]
    };
    if let Some(el) = text_cache.render_label_centered_weighted(
        renderer,
        active_app_name,
        crate::text::FontWeight::Bold,
        TOP_BAR_FONT_SIZE,
        bar_fg,
        app_bounds,
    ) {
        elements.push(el);
    }

    let text_menu = if use_light_bar_ink {
        [246, 247, 250, 245]
    } else {
        [28, 29, 34, 240]
    };
    let (visible_menus, _) = super::visible_top_bar_menu_slots(
        app_profile,
        active_app_name,
        top_telemetry.app_menu_tree,
        max_left_menu_x,
    );
    for (menu_hit, title, x, w) in visible_menus {
        let color = if top_telemetry.active_menu == Some(menu_hit) {
            bar_fg
        } else {
            text_menu
        };
        let bounds = Rectangle::new(Point::from((x, ws_y)), Size::from((w, ws_btn_height)));
        if let Some(el) = text_cache.render_label_centered_weighted(
            renderer,
            title.as_ref(),
            crate::text::FontWeight::Regular,
            TOP_BAR_FONT_SIZE,
            color,
            bounds,
        ) {
            elements.push(el);
        }
    }

    // Active TopBar left dropdown menu labels + right-aligned real shortcuts (`RUSTYOS-048`/`049`/`051`/`068`/`145`)
    if let Some(open_menu) = top_telemetry
        .active_menu
        .filter(|m| *m != TopBarMenuHit::Go || left_layout.has_go)
    {
        let (mx, my, mw, _) = super::top_bar_menu_popover_rect_for_width(
            app_profile,
            active_app_name,
            open_menu,
            top_telemetry.app_menu_tree,
            max_left_menu_x,
        );
        let mut row_y = my + MENU_POPOVER_PAD_Y;
        for (entry_idx, entry) in super::resolve_top_bar_menu_entries_for_width(
            app_profile,
            active_app_name,
            open_menu,
            top_telemetry.app_menu_tree,
            max_left_menu_x,
        )
        .into_iter()
        .enumerate()
        {
            match entry {
                ResolvedTopBarMenuItem::Item {
                    label,
                    icon,
                    shortcut,
                    enabled,
                    ..
                } => {
                    let hovered = top_telemetry.menu_hover_row == Some(entry_idx) && enabled;
                    let fg = if hovered {
                        [255, 255, 255, 255]
                    } else if !enabled {
                        pal.popover_text_disabled
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
                        let (hint_w, _) = text_cache.measure_text(hint.as_ref(), 12.0);
                        let hint_fg = if hovered {
                            [255, 255, 255, 255]
                        } else {
                            pal.popover_text_muted
                        };
                        if let Some(el) = text_cache.render_label(
                            renderer,
                            hint.as_ref(),
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
            } else if use_light_bar_ink {
                [214, 214, 222, 220]
            } else {
                [74, 76, 84, 225]
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
        if let Some(el) =
            text_cache.render_label_centered(renderer, layout_label, 11.0, bar_fg, layout_bounds)
        {
            elements.push(el);
        }
    }

    // 4. Right status cluster live clock and optional kbd / notification badge / tray (`RUSTYOS-049`)
    let clock_bounds = Rectangle::new(
        Point::from((right_layout.clock_x, ws_y)),
        Size::from((right_layout.clock_w, ws_btn_height)),
    );
    if let Some(el) = text_cache.render_label_centered_weighted(
        renderer,
        top_telemetry.clock_hhmm,
        crate::text::FontWeight::Regular,
        TOP_BAR_FONT_SIZE,
        bar_fg,
        clock_bounds,
    ) {
        elements.push(el);
    }

    if top_telemetry.show_battery_percentage
        && let Some(bx) = right_layout.battery_x
    {
        let pct_text = format!("{}%", top_telemetry.battery_percent);
        let pct_bounds = Rectangle::new(
            Point::from((bx + 27, ws_y)),
            Size::from((30, ws_btn_height)),
        );
        if let Some(el) =
            text_cache.render_label_centered(renderer, &pct_text, 10.5, bar_fg, pct_bounds)
        {
            elements.push(el);
        }
    }

    if let Some(rx) = right_layout.recording_x {
        let rec_bounds = Rectangle::new(
            Point::from((rx + 12, ws_y)),
            Size::from((right_layout.recording_w - 14, ws_btn_height)),
        );
        if let Some(el) = text_cache.render_label_centered_weighted(
            renderer,
            "REC",
            crate::text::FontWeight::Bold,
            9.5,
            [255, 255, 255, 255],
            rec_bounds,
        ) {
            elements.push(el);
        }
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
            bar_fg,
            kbd_bounds,
        ) {
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
                if light_opaque_bar {
                    [200, 40, 52, 255]
                } else {
                    [255, 210, 214, 255]
                }
            } else if item.passive {
                if light_opaque_bar {
                    [90, 92, 100, 230]
                } else {
                    text_muted
                }
            } else {
                bar_fg
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
    append_dock_text(
        text_cache,
        renderer,
        screen_size,
        dock_view,
        overlay,
        elements,
    );
    let _ = shape_cache;
}

/// Renders Dock hover tooltip, drag `"Remove"` pill, and unread count badge labels (`RUSTYOS-050` / `RUSTYOS-161` / `RUSTYOS-162`).
pub(super) fn append_dock_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    dock_view: &DockViewState<'_>,
    overlay: ShellOverlayView<'_>,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    if !text_cache.has_font() {
        return;
    }
    let pal = ThemePalette::active();
    let width = screen_size.w;
    let height = screen_size.h;
    let owned_layout;
    let layout = if let Some(presented) = dock_view.presented_layout {
        presented
    } else {
        owned_layout = compute_dock_layout(screen_size, dock_view);
        &owned_layout
    };

    if !dock_tooltip_hidden_by_overlay(overlay) {
        if dock_view.drag_preview.is_some_and(|d| d.pulled_outside) {
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

    if dock_view.auto_hide_reveal > 0.01 {
        for slot in &layout.slots {
            if let Some((badge_rect, label)) =
                super::dock::dock_count_badge_rect(slot.icon_rect, slot.tile.badge_count)
            {
                let font_size = if badge_rect.size.h < 18 { 9.5 } else { 10.5 };
                if let Some(el) = text_cache.render_label_centered_weighted(
                    renderer,
                    &label,
                    crate::text::FontWeight::Bold,
                    font_size,
                    [255, 255, 255, 255],
                    Rectangle::new(
                        Point::from((badge_rect.loc.x, badge_rect.loc.y)),
                        Size::from((badge_rect.size.w, badge_rect.size.h)),
                    ),
                ) {
                    elements.push(el);
                }
            }
        }
    }
}
