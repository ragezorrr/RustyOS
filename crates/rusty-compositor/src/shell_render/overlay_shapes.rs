//! Foreground overlay shape rendering pass (`Spotlight`, `QuickSettings`, `Switcher`, modals, toasts) (`RUSTYOS-101`).

use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::{Point, Rectangle, Size};

use super::context_menu::render_context_menu_composite;
use super::dock_stack::render_dock_stack_composite;
use super::modals::{
    render_pick_color_composite, render_polkit_composite, render_portal_composite,
    render_settings_composite,
};
use super::quick_settings::render_quick_settings_composite;
use super::spotlight_overview::{spotlight_results_rect, spotlight_search_pill_rect};
use super::text_pass::append_foreground_overlay_text;
use super::tokens::{
    ThemePalette, anim_offset, push_liquid_glass_panel, push_rounded_glass_box,
    resolved_icon_to_element,
};
use super::top_bar::notification_app_icon;
use super::updates_popover::render_updates_composite;
use super::{
    COLOR_ACCENT, COLOR_MUTED, COLOR_WARNING, CompositorRenderElement, NotificationToastView,
    ShellOverlayView, TopBarTelemetry, make_solid_element, notification_toast_rects,
    notifications_hidden_by_overlay, switcher_layout,
};

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
    let pal = ThemePalette::active();
    match overlay {
        ShellOverlayView::None | ShellOverlayView::Overview(_) => {}
        ShellOverlayView::Spotlight(spot) => {
            let dy = anim_offset(spot.progress, -22.0);
            let (sx, search_top, sw, sh) = spotlight_search_pill_rect(width, height);
            let (rx, results_top, rw, rh) = spotlight_results_rect(width, height);
            let sy = search_top + dy;
            let ry = results_top + dy;
            let glass_op = top_telemetry.glass_opacity;
            let reduce_tr = top_telemetry.reduce_transparency;

            // Search pill foreground elements (front-to-back above glass capsule)
            if let Some(el) = shape_cache.ui_glyph_element(
                renderer,
                crate::surfaces::UiGlyphKind::Search,
                18,
                pal.popover_icon,
                Point::from((sx + 22, sy + 18)),
            ) {
                out.push(CompositorRenderElement::Memory(el));
            }
            let cursor_x = (sx + 50 + spot.cursor_offset_x).min(sx + sw - 28);
            out.push(CompositorRenderElement::Solid(make_solid_element(
                Rectangle::new(Point::from((cursor_x, sy + 16)), Size::from((2, 22))),
                COLOR_ACCENT,
            )));
            if !has_text {
                out.push(CompositorRenderElement::Solid(make_solid_element(
                    Rectangle::new(Point::from((sx + 54, sy + 23)), Size::from((160, 8))),
                    COLOR_MUTED,
                )));
            }
            push_liquid_glass_panel(
                &mut out,
                shape_cache,
                renderer,
                Rectangle::new(Point::from((sx, sy)), Size::from((sw, sh))),
                27,
                crate::surfaces::GlassPanelStyle::Popover,
                glass_op,
                reduce_tr,
                Some((24, 30)),
            );

            // Detached results capsule with 10 px breathing space below search pill
            let row_h = 46;
            let start_row_y = ry + 14;
            for (idx, item) in spot.items.iter().take(5).enumerate() {
                let i = i32::try_from(idx).unwrap_or(0);
                let row_y = start_row_y + i * (row_h + 4);
                let is_sel = idx == spot.selected_in_window;

                let icon_x = rx + 22;
                let icon_y = row_y + 9;
                if let Some(icon_el) = item.icon.as_deref().and_then(|name| {
                    shape_cache.icons.icon_element_by_name(
                        renderer,
                        name,
                        28,
                        Point::from((icon_x, icon_y)),
                    )
                }) {
                    out.push(CompositorRenderElement::Memory(icon_el));
                } else {
                    push_rounded_glass_box(
                        &mut out,
                        shape_cache,
                        renderer,
                        Rectangle::new(Point::from((icon_x, icon_y)), Size::from((28, 28))),
                        8,
                        if is_sel {
                            [255, 255, 255, 42]
                        } else {
                            pal.control_pill_off_rgba
                        },
                        None,
                    );
                }

                if is_sel {
                    push_rounded_glass_box(
                        &mut out,
                        shape_cache,
                        renderer,
                        Rectangle::new(Point::from((rx + 12, row_y)), Size::from((rw - 24, row_h))),
                        12,
                        pal.accent_hover_fill,
                        None,
                    );
                }
            }

            push_liquid_glass_panel(
                &mut out,
                shape_cache,
                renderer,
                Rectangle::new(Point::from((rx, ry)), Size::from((rw, rh))),
                22,
                crate::surfaces::GlassPanelStyle::Popover,
                glass_op,
                reduce_tr,
                Some((26, 32)),
            );
        }
        ShellOverlayView::QuickSettings(qs) => {
            render_quick_settings_composite(&mut out, shape_cache, renderer, width, height, qs);
        }
        ShellOverlayView::Switcher(sw) => {
            let (panel_rect, cards) = switcher_layout(screen_size, sw.cards.len());
            for (idx, (card_view, card)) in sw.cards.iter().zip(cards).enumerate() {
                let is_sel = idx == sw.selected;
                let icon_x = card.loc.x + (card.size.w - 44) / 2;
                let icon_y = card.loc.y + 14;
                let sw_key = format!("win-{}", card_view.window_id.0);
                if let Some(icon_el) = shape_cache.icons.window_icon_element(
                    renderer,
                    &sw_key,
                    card_view.toplevel_icon,
                    card_view.icon_name,
                    44,
                    Point::from((icon_x, icon_y)),
                ) {
                    out.push(resolved_icon_to_element(icon_el));
                } else {
                    let well_rect =
                        Rectangle::new(Point::from((icon_x, icon_y)), Size::from((44, 44)));
                    push_rounded_glass_box(
                        &mut out,
                        shape_cache,
                        renderer,
                        well_rect,
                        12,
                        if is_sel {
                            pal.accent_rgba
                        } else {
                            pal.control_pill_off_rgba
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

                if is_sel {
                    let sel_fill = if pal.is_dark {
                        [255, 255, 255, 36]
                    } else {
                        [0, 0, 0, 22]
                    };
                    push_rounded_glass_box(
                        &mut out,
                        shape_cache,
                        renderer,
                        card,
                        16,
                        sel_fill,
                        None,
                    );
                }
            }

            if panel_rect.size.w > 0 {
                push_liquid_glass_panel(
                    &mut out,
                    shape_cache,
                    renderer,
                    panel_rect,
                    24,
                    crate::surfaces::GlassPanelStyle::Popover,
                    top_telemetry.glass_opacity,
                    top_telemetry.reduce_transparency,
                    Some((20, 58)),
                );
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
        ShellOverlayView::Settings(st) => {
            render_settings_composite(
                &mut out,
                shape_cache,
                renderer,
                screen_size,
                top_telemetry,
                st,
            );
        }
        ShellOverlayView::ContextMenu(ctx) => {
            render_context_menu_composite(
                &mut out,
                shape_cache,
                renderer,
                screen_size,
                top_telemetry.glass_opacity,
                top_telemetry.reduce_transparency,
                ctx,
                true,
            );
        }
        ShellOverlayView::DockStack(ds) => {
            render_dock_stack_composite(
                &mut out,
                shape_cache,
                renderer,
                screen_size,
                top_telemetry.glass_opacity,
                top_telemetry.reduce_transparency,
                ds,
            );
        }
        ShellOverlayView::Updates(up) => {
            render_updates_composite(&mut out, shape_cache, renderer, screen_size, up);
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
                    [255, 255, 255, 255],
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
                16,
                crate::surfaces::GlassPanelStyle::Popover,
                top_telemetry.glass_opacity,
                top_telemetry.reduce_transparency,
                Some((12, 46)),
            );
        }
    }

    out
}
