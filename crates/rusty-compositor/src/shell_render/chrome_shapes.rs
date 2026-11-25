//! Persistent desktop chrome (`TopBar`, `Dock`) and `Workspace Overview` shape rendering (`RUSTYOS-101`).

use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::{Point, Rectangle, Size};

use super::dock::{
    ComputedDockLayout, ComputedDockSlot, dock_count_badge_rect, dock_tooltip_rect_and_label,
};
use super::spotlight_overview::render_overview_text;
use super::text_pass::{append_chrome_text, append_dock_text};
use super::tokens::{
    RGBA_ACCENT, RGBA_BORDER, RGBA_DANGER, ThemePalette, anim_offset, premul,
    push_elevation_shadow, push_liquid_glass_panel, push_liquid_glass_panel_elevation,
    push_rounded_glass_box, resolved_icon_to_element, rgba_to_premul_color32f, title_hue_seed,
};
use super::top_bar::{
    MENU_ITEM_HEIGHT, MENU_POPOVER_PAD_Y, MENU_SEPARATOR_HEIGHT, push_top_bar_right_glyphs,
};
use super::top_bar_menus::ResolvedTopBarMenuItem;
use super::{
    COLOR_ACCENT, COLOR_BORDER, COLOR_DANGER, CompositorRenderElement, DockAppTile, DockSection,
    DockViewState, MENU_POPOVER_RADIUS, OverviewRenderState, ShellOverlayView, TOP_BAR_HEIGHT,
    TOP_BAR_ITEM_H, TOP_BAR_ITEM_Y, TOP_BAR_LOGO_W, TOP_BAR_LOGO_X, TopBarMenuHit, TopBarTelemetry,
    WS_BTN_STRIDE, WS_BTN_WIDTH, compute_dock_layout, compute_dock_radius,
    compute_top_bar_left_layout_for_profile, compute_top_bar_right_layout_with_options,
    dock_tooltip_hidden_by_overlay, make_solid_element, overview_card_close_rect,
    overview_card_rects, overview_space_rects, tray_slot_rects_with_options,
};

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
    generate_shell_chrome_composite_with_dock(
        text_cache,
        shape_cache,
        renderer,
        screen_size,
        active_workspace,
        layout_mode,
        Some(dock_view),
        top_telemetry,
        overlay,
    )
}

/// Generates desktop chrome (`TopBar`, optional menu popover, and optional `Dock`) for secondary outputs.
#[allow(clippy::too_many_arguments)]
pub fn generate_secondary_shell_chrome_elements(
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    active_workspace: u8,
    layout_mode: &str,
    dock_view: Option<&DockViewState<'_>>,
    top_telemetry: &TopBarTelemetry<'_>,
    overlay: ShellOverlayView<'_>,
) -> Vec<CompositorRenderElement> {
    generate_shell_chrome_composite_with_dock(
        text_cache,
        shape_cache,
        renderer,
        screen_size,
        active_workspace,
        layout_mode,
        dock_view,
        top_telemetry,
        overlay,
    )
}

#[allow(clippy::too_many_arguments)]
fn generate_shell_chrome_composite_with_dock(
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    active_workspace: u8,
    layout_mode: &str,
    dock_view: Option<&DockViewState<'_>>,
    top_telemetry: &TopBarTelemetry<'_>,
    overlay: ShellOverlayView<'_>,
) -> Vec<CompositorRenderElement> {
    let mut out = Vec::new();
    let mut chrome_text = Vec::new();
    let dummy_dock = DockViewState::default();
    let dock_ref = dock_view.unwrap_or(&dummy_dock);
    append_chrome_text(
        text_cache,
        shape_cache,
        renderer,
        screen_size,
        active_workspace,
        layout_mode,
        dock_ref,
        top_telemetry,
        overlay,
        &mut chrome_text,
    );
    out.extend(chrome_text.into_iter().map(CompositorRenderElement::Memory));
    out.extend(build_chrome_shapes_with_dock(
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

/// Generates floating `Dock`-only chrome elements (text + shapes) for secondary monitors (`RUSTYOS-161`).
#[allow(clippy::too_many_arguments)]
pub fn generate_dock_only_chrome_elements(
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    dock_view: &DockViewState<'_>,
    glass_opacity: f32,
    reduce_transparency: bool,
    overlay: ShellOverlayView<'_>,
) -> Vec<CompositorRenderElement> {
    let mut out = Vec::new();
    let mut dock_text = Vec::new();
    append_dock_text(
        text_cache,
        renderer,
        screen_size,
        dock_view,
        overlay,
        &mut dock_text,
    );
    out.extend(dock_text.into_iter().map(CompositorRenderElement::Memory));
    let pal = ThemePalette::active();
    push_dock_chrome_shapes_with_glass(
        &mut out,
        shape_cache,
        renderer,
        screen_size,
        dock_view,
        glass_opacity,
        reduce_transparency,
        overlay,
        &pal,
    );
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
        render_overview_text(
            text_cache,
            renderer,
            screen_size,
            ov,
            |id| shape_cache.has_lifecycle_snapshot(id),
            &mut ov_text,
        );
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

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn build_chrome_shapes_with_dock(
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    active_workspace: u8,
    layout_mode: &str,
    dock_view: Option<&DockViewState<'_>>,
    top_telemetry: &TopBarTelemetry<'_>,
    overlay: ShellOverlayView<'_>,
) -> Vec<CompositorRenderElement> {
    let mut out = Vec::new();
    let width = screen_size.w;
    let pal = ThemePalette::active();

    // 3. TopBar macOS Menu Bar SDF glyphs, optional tray/workspaces, menu popover & transparent default (`RUSTYOS-048`, `RUSTYOS-049`, `RUSTYOS-068`)
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

    let light_opaque_bar = top_telemetry.reduce_transparency && !pal.is_dark;
    let use_light_bar_ink = top_telemetry
        .backdrop
        .map_or(!light_opaque_bar, |b| b.prefer_light_text);
    let logo_ink = if use_light_bar_ink {
        [255, 255, 255, 255]
    } else {
        [28, 29, 34, 255]
    };
    let logo_loc = Point::from((TOP_BAR_LOGO_X + 1, TOP_BAR_ITEM_Y + 1));
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        crate::surfaces::UiGlyphKind::RustyLogo,
        18,
        logo_ink,
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

    let max_left_menu_x = right_layout.tray_anchor_x - 10;
    if let Some(open_menu) = top_telemetry
        .active_menu
        .filter(|m| *m != TopBarMenuHit::Go || left_layout.has_go)
    {
        let (visible_menus, _) = super::visible_top_bar_menu_slots(
            app_profile,
            active_app_name,
            top_telemetry.app_menu_tree,
            max_left_menu_x,
        );
        let (anchor_x, anchor_w) = match open_menu {
            TopBarMenuHit::SystemLogo => (TOP_BAR_LOGO_X - 2, TOP_BAR_LOGO_W + 4),
            TopBarMenuHit::ActiveApp => (left_layout.app_x - 2, left_layout.app_w + 4),
            other => visible_menus
                .iter()
                .find(|(hit, _, _, _)| *hit == other)
                .map_or(
                    (left_layout.file_x - 2, left_layout.file_w + 4),
                    |(_, _, x, w)| (x - 2, w + 4),
                ),
        };
        let plate_bg = if pal.is_dark {
            [255, 255, 255, 51]
        } else {
            [0, 0, 0, 26]
        };
        push_rounded_glass_box(
            &mut out,
            shape_cache,
            renderer,
            Rectangle::new(
                Point::from((anchor_x, TOP_BAR_ITEM_Y)),
                Size::from((anchor_w, TOP_BAR_ITEM_H)),
            ),
            8,
            plate_bg,
            None,
        );
        let (mx, my, mw, mh) = super::top_bar_menu_popover_rect_for_width(
            app_profile,
            active_app_name,
            open_menu,
            top_telemetry.app_menu_tree,
            max_left_menu_x,
        );
        let menu_rect = Rectangle::new(Point::from((mx, my)), Size::from((mw, mh)));
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
                ResolvedTopBarMenuItem::Item { icon, enabled, .. } => {
                    let hovered = top_telemetry.menu_hover_row == Some(entry_idx) && enabled;
                    if let Some(glyph) = icon {
                        let icon_tint = if hovered {
                            [255, 255, 255, 255]
                        } else if !enabled {
                            pal.popover_text_disabled
                        } else {
                            pal.popover_icon
                        };
                        if let Some(el) = shape_cache.ui_glyph_element(
                            renderer,
                            glyph,
                            14,
                            icon_tint,
                            Point::from((mx + 14, row_y + 5)),
                        ) {
                            out.push(CompositorRenderElement::Memory(el));
                        }
                    }
                    if hovered {
                        let hover_bg = if pal.is_dark {
                            [
                                pal.accent_rgba[0],
                                pal.accent_rgba[1],
                                pal.accent_rgba[2],
                                210,
                            ]
                        } else {
                            [
                                pal.accent_rgba[0],
                                pal.accent_rgba[1],
                                pal.accent_rgba[2],
                                230,
                            ]
                        };
                        push_rounded_glass_box(
                            &mut out,
                            shape_cache,
                            renderer,
                            Rectangle::new(
                                Point::from((mx + 5, row_y + 1)),
                                Size::from((mw - 10, MENU_ITEM_HEIGHT - 2)),
                            ),
                            rusty_ui::menu::HOVER_PILL_RADIUS,
                            hover_bg,
                            None,
                        );
                    }
                    row_y += MENU_ITEM_HEIGHT;
                }
                ResolvedTopBarMenuItem::Separator => {
                    out.push(CompositorRenderElement::Solid(make_solid_element(
                        Rectangle::new(Point::from((mx + 12, row_y + 5)), Size::from((mw - 24, 1))),
                        pal.popover_separator,
                    )));
                    row_y += MENU_SEPARATOR_HEIGHT;
                }
            }
        }
        push_liquid_glass_panel_elevation(
            &mut out,
            shape_cache,
            renderer,
            menu_rect,
            MENU_POPOVER_RADIUS,
            crate::surfaces::GlassPanelStyle::Popover,
            top_telemetry.glass_opacity,
            top_telemetry.reduce_transparency,
            rusty_ui::shadow::ShadowElevation::MenuOrTooltip,
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

    let topbar_active_pill = if light_opaque_bar {
        [0, 0, 0, 24]
    } else {
        [255, 255, 255, 42]
    };
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
            topbar_active_pill,
            None,
        );
    }
    if top_telemetry.notification_center_open {
        push_rounded_glass_box(
            &mut out,
            shape_cache,
            renderer,
            Rectangle::new(
                Point::from((right_layout.clock_x - 5, TOP_BAR_ITEM_Y)),
                Size::from((right_layout.clock_w + 10, TOP_BAR_ITEM_H)),
            ),
            6,
            topbar_active_pill,
            None,
        );
    }
    if top_telemetry.wifi_open {
        push_rounded_glass_box(
            &mut out,
            shape_cache,
            renderer,
            Rectangle::new(
                Point::from((right_layout.wifi_x - 4, TOP_BAR_ITEM_Y)),
                Size::from((28, TOP_BAR_ITEM_H)),
            ),
            6,
            topbar_active_pill,
            None,
        );
    }
    if top_telemetry.updates_open
        && let Some(ux) = right_layout.updates_x
    {
        push_rounded_glass_box(
            &mut out,
            shape_cache,
            renderer,
            Rectangle::new(
                Point::from((ux - 3, TOP_BAR_ITEM_Y)),
                Size::from((right_layout.updates_w + 6, TOP_BAR_ITEM_H)),
            ),
            6,
            topbar_active_pill,
            None,
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

        if layout_mode == "master-stack" {
            push_rounded_glass_box(
                &mut out,
                shape_cache,
                renderer,
                Rectangle::new(
                    Point::from((left_layout.layout_x, ws_y)),
                    Size::from((left_layout.layout_w, ws_h)),
                ),
                9,
                pal.accent_hover_fill,
                None,
            );
        }
    }

    // Fully transparent TopBar by default (`reduce_transparency == false`);
    // render solid Light or Dark high-contrast bar when `reduce_transparency` is enabled (`RUSTYOS-049`, `RUSTYOS-128`).
    if top_telemetry.reduce_transparency {
        let (bar_fill, bar_border) = if pal.is_dark {
            (rgba_to_premul_color32f([26, 26, 30, 245]), COLOR_BORDER)
        } else {
            (
                rgba_to_premul_color32f([242, 243, 247, 248]),
                rgba_to_premul_color32f([0, 0, 0, 26]),
            )
        };
        out.push(CompositorRenderElement::Solid(make_solid_element(
            Rectangle::new(Point::from((0, TOP_BAR_HEIGHT - 1)), Size::from((width, 1))),
            bar_border,
        )));
        out.push(CompositorRenderElement::Solid(make_solid_element(
            Rectangle::new(Point::from((0, 0)), Size::from((width, TOP_BAR_HEIGHT))),
            bar_fill,
        )));
    } else if let Some(backdrop) = top_telemetry.backdrop
        && backdrop.scrim_alpha > 0
    {
        let scrim = if backdrop.dark_scrim {
            rgba_to_premul_color32f([0, 0, 0, backdrop.scrim_alpha])
        } else {
            rgba_to_premul_color32f([255, 255, 255, backdrop.scrim_alpha])
        };
        out.push(CompositorRenderElement::Solid(make_solid_element(
            Rectangle::new(Point::from((0, 0)), Size::from((width, TOP_BAR_HEIGHT))),
            scrim,
        )));
    }

    // 4. Dock macOS glass container, 4x4 SSAA/XDG icons, local hover, drag overlay & indicators (`RUSTYOS-050`/`073`)
    if let Some(dock) = dock_view {
        push_dock_chrome_shapes(
            &mut out,
            shape_cache,
            renderer,
            screen_size,
            dock,
            top_telemetry,
            overlay,
            &pal,
        );
    }

    out
}

fn render_dock_tile_icon(
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    tile: &DockAppTile,
    draw_size: i32,
    icon_loc: Point<i32, smithay::utils::Logical>,
) -> Option<CompositorRenderElement> {
    // Frame third-party browser (Firefox) inside a cohesive RustyOS squircle tile
    // when its XDG PNG is available, falling back to our procedural Browser compass squircle.
    if (tile.app_key == "browser" || tile.app_key == "firefox")
        && let Some(icon_name) = tile.icon_name.as_deref()
        && let Some(framed_el) = shape_cache
            .icons
            .framed_xdg_squircle_icon_element(renderer, icon_name, draw_size, icon_loc)
    {
        return Some(CompositorRenderElement::Memory(framed_el));
    }
    if let Some(kind) = tile.builtin_icon {
        return shape_cache
            .icons
            .builtin_squircle_icon_element(renderer, kind, draw_size, icon_loc)
            .map(CompositorRenderElement::Memory);
    }
    // Unify all third-party XDG app icons (Chrome, Steam, Discord, etc.) inside the same squircle frame.
    for candidate in tile
        .icon_name
        .as_deref()
        .into_iter()
        .chain(std::iter::once(tile.app_key.as_str()))
    {
        if let Some(framed_el) = shape_cache
            .icons
            .framed_xdg_squircle_icon_element(renderer, candidate, draw_size, icon_loc)
        {
            return Some(CompositorRenderElement::Memory(framed_el));
        }
    }
    shape_cache
        .icons
        .window_icon_element(
            renderer,
            &tile.app_key,
            tile.toplevel_icon.as_ref(),
            tile.icon_name.as_deref(),
            draw_size,
            icon_loc,
        )
        .or_else(|| {
            let win_id = tile.representative_win.map_or(0, |w| w.0);
            let hue_seed = title_hue_seed(&tile.fallback_title, win_id);
            shape_cache
                .icons
                .builtin_squircle_icon_element(
                    renderer,
                    crate::icons::BuiltinAppIcon::WindowFallback {
                        hue_seed,
                        active: tile.is_active,
                    },
                    draw_size,
                    icon_loc,
                )
                .map(|el| crate::icons::ResolvedIconElement::Memory(Box::new(el)))
        })
        .map(resolved_icon_to_element)
}

#[allow(clippy::too_many_arguments)]
fn push_dock_tooltip_and_drag(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    dock_view: &DockViewState<'_>,
    glass_opacity: f32,
    reduce_transparency: bool,
    overlay: ShellOverlayView<'_>,
    layout: &ComputedDockLayout,
) {
    if !dock_tooltip_hidden_by_overlay(overlay)
        && let Some((tooltip_rect, _)) =
            dock_tooltip_rect_and_label(screen_size.w, screen_size.h, dock_view)
    {
        push_liquid_glass_panel_elevation(
            out,
            shape_cache,
            renderer,
            tooltip_rect,
            9,
            crate::surfaces::GlassPanelStyle::Popover,
            glass_opacity,
            reduce_transparency,
            rusty_ui::shadow::ShadowElevation::MenuOrTooltip,
        );
    }
    if let Some(badge_rect) = layout.drag_remove_badge {
        push_rounded_glass_box(
            out,
            shape_cache,
            renderer,
            badge_rect,
            10,
            [218, 56, 66, 232],
            Some([255, 146, 150, 185]),
        );
        push_elevation_shadow(
            out,
            shape_cache,
            renderer,
            badge_rect,
            10,
            rusty_ui::shadow::ShadowElevation::Popup,
        );
    }
    if let Some((ref dragged_tile, drag_rect)) = layout.dragged_icon {
        if let Some(el) = render_dock_tile_icon(
            shape_cache,
            renderer,
            dragged_tile,
            drag_rect.size.w,
            Point::from((drag_rect.loc.x, drag_rect.loc.y)),
        ) {
            out.push(el);
        }
        push_elevation_shadow(
            out,
            shape_cache,
            renderer,
            drag_rect,
            12,
            rusty_ui::shadow::ShadowElevation::Popup,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn push_dock_chrome_shapes(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    dock_view: &DockViewState<'_>,
    top_telemetry: &TopBarTelemetry<'_>,
    overlay: ShellOverlayView<'_>,
    pal: &ThemePalette,
) {
    push_dock_chrome_shapes_with_glass(
        out,
        shape_cache,
        renderer,
        screen_size,
        dock_view,
        top_telemetry.glass_opacity,
        top_telemetry.reduce_transparency,
        overlay,
        pal,
    );
}

#[allow(clippy::too_many_arguments)]
fn push_dock_chrome_shapes_with_glass(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    dock_view: &DockViewState<'_>,
    glass_opacity: f32,
    reduce_transparency: bool,
    overlay: ShellOverlayView<'_>,
    pal: &ThemePalette,
) {
    let owned_layout;
    let layout = if let Some(presented) = dock_view.presented_layout {
        presented
    } else {
        owned_layout = compute_dock_layout(screen_size, dock_view);
        &owned_layout
    };
    push_dock_tooltip_and_drag(
        out,
        shape_cache,
        renderer,
        screen_size,
        dock_view,
        glass_opacity,
        reduce_transparency,
        overlay,
        layout,
    );

    if dock_view.auto_hide_reveal <= 0.01 {
        return;
    }

    let mut dock_badges = Vec::new();
    let mut dock_icons = Vec::with_capacity(layout.slots.len());
    let mut dock_dots_and_divider = Vec::new();
    let sep_col = if pal.is_dark {
        rgba_to_premul_color32f([255, 255, 255, 64])
    } else {
        rgba_to_premul_color32f([0, 0, 0, 52])
    };
    for &div_rect in &layout.dividers {
        dock_dots_and_divider.push(CompositorRenderElement::Solid(make_solid_element(
            div_rect, sep_col,
        )));
    }

    for slot in &layout.slots {
        push_dock_slot_shape(
            &mut dock_badges,
            &mut dock_icons,
            &mut dock_dots_and_divider,
            shape_cache,
            renderer,
            dock_view,
            pal,
            slot,
        );
    }

    if let Some(insert_rect) = layout.drop_insert_rect {
        let (fill, border) = if pal.is_dark {
            ([10, 132, 255, 58], [10, 132, 255, 225])
        } else {
            ([0, 122, 255, 42], [0, 122, 255, 215])
        };
        push_rounded_glass_box(
            &mut dock_icons,
            shape_cache,
            renderer,
            insert_rect,
            13,
            fill,
            Some(border),
        );
    }

    out.extend(dock_badges);
    out.extend(dock_icons);
    out.extend(dock_dots_and_divider);

    let capsule = layout.capsule_rect;
    let radius = compute_dock_radius(capsule.size.h.min(capsule.size.w));
    if let Some(dock_glass) = shape_cache.dock_glass_render_element(
        renderer,
        capsule.size.w,
        capsule.size.h,
        radius,
        glass_opacity,
        reduce_transparency,
        Point::from((capsule.loc.x, capsule.loc.y)),
    ) {
        out.push(dock_glass);
    }
    push_elevation_shadow(
        out,
        shape_cache,
        renderer,
        capsule,
        radius,
        rusty_ui::shadow::ShadowElevation::Dock,
    );
}

#[allow(clippy::too_many_arguments)]
fn push_dock_slot_shape(
    dock_badges: &mut Vec<CompositorRenderElement>,
    dock_icons: &mut Vec<CompositorRenderElement>,
    dock_dots: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    dock_view: &DockViewState<'_>,
    pal: &ThemePalette,
    slot: &ComputedDockSlot,
) {
    let tile = &slot.tile;
    if let Some((badge_rect, _)) = dock_count_badge_rect(slot.icon_rect, tile.badge_count) {
        let (fill, border) = if pal.is_dark {
            ([255, 69, 58, 255], [255, 255, 255, 115])
        } else {
            ([255, 59, 48, 255], [255, 255, 255, 215])
        };
        let radius = badge_rect.size.h / 2;
        push_rounded_glass_box(
            dock_badges,
            shape_cache,
            renderer,
            badge_rect,
            radius,
            fill,
            Some(border),
        );
        push_elevation_shadow(
            dock_badges,
            shape_cache,
            renderer,
            badge_rect,
            radius,
            rusty_ui::shadow::ShadowElevation::Popup,
        );
    }
    if dock_view.show_indicators && tile.has_indicator {
        let pip_color = if tile.has_minimized && tile.window_count == 1 {
            [232, 196, 119, 235]
        } else if pal.is_dark {
            if tile.is_active {
                [246, 247, 250, 242]
            } else {
                [220, 224, 232, 165]
            }
        } else if tile.is_active {
            [28, 30, 36, 228]
        } else {
            [44, 48, 56, 155]
        };
        if let Some(dot) = shape_cache.circle_element(
            renderer,
            slot.dot_rect.size.w,
            pip_color,
            None,
            Point::from((slot.dot_rect.loc.x, slot.dot_rect.loc.y)),
        ) {
            dock_dots.push(CompositorRenderElement::Memory(dot));
        }
    }

    if tile.section == DockSection::MinimizedWindow {
        push_minimized_dock_slot(dock_icons, shape_cache, renderer, pal, slot);
    } else if let Some(el) = render_dock_tile_icon(
        shape_cache,
        renderer,
        tile,
        slot.icon_rect.size.w,
        Point::from((slot.icon_rect.loc.x, slot.icon_rect.loc.y)),
    ) {
        dock_icons.push(el);
    }

    push_dock_slot_rings(dock_icons, shape_cache, renderer, dock_view, pal, slot);
}

fn push_dock_slot_rings(
    dock_icons: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    dock_view: &DockViewState<'_>,
    pal: &ThemePalette,
    slot: &ComputedDockSlot,
) {
    if dock_view.focused_slot == Some(slot.slot_idx) {
        let (fill, border) = if pal.is_dark {
            ([10, 132, 255, 36], [110, 185, 255, 225])
        } else {
            ([0, 122, 255, 28], [0, 122, 255, 220])
        };
        let ring = Rectangle::new(
            Point::from((slot.icon_rect.loc.x - 3, slot.icon_rect.loc.y - 3)),
            Size::from((slot.icon_rect.size.w + 6, slot.icon_rect.size.h + 6)),
        );
        push_rounded_glass_box(
            dock_icons,
            shape_cache,
            renderer,
            ring,
            15,
            fill,
            Some(border),
        );
    }

    if let Some(super::DockDropHighlight::Slot { slot_idx, kind, .. }) = dock_view.drop_highlight
        && slot_idx == slot.slot_idx
    {
        let danger = kind == super::DockDropKind::MoveToTrash;
        let (fill, border) = match (pal.is_dark, danger) {
            (true, true) => ([255, 69, 58, 62], [255, 69, 58, 235]),
            (false, true) => ([255, 59, 48, 48], [255, 59, 48, 225]),
            (true, false) => ([10, 132, 255, 58], [10, 132, 255, 230]),
            (false, false) => ([0, 122, 255, 42], [0, 122, 255, 220]),
        };
        let ring = Rectangle::new(
            Point::from((slot.icon_rect.loc.x - 4, slot.icon_rect.loc.y - 4)),
            Size::from((slot.icon_rect.size.w + 8, slot.icon_rect.size.h + 8)),
        );
        push_rounded_glass_box(
            dock_icons,
            shape_cache,
            renderer,
            ring,
            14,
            fill,
            Some(border),
        );
    }
}

fn push_minimized_dock_slot(
    dock_icons: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    pal: &ThemePalette,
    slot: &ComputedDockSlot,
) {
    let tile = &slot.tile;
    let pad = (slot.icon_rect.size.w / 16).clamp(2, 4);
    let thumb_slot = Rectangle::new(
        Point::from((slot.icon_rect.loc.x + pad, slot.icon_rect.loc.y + pad)),
        Size::from((
            (slot.icon_rect.size.w - pad * 2).max(12),
            (slot.icon_rect.size.h - pad * 2).max(12),
        )),
    );
    if let Some(win_id) = tile.representative_win
        && let Some((thumb_el, fit_rect)) = shape_cache
            .render_minimized_dock_thumbnail_element(renderer, win_id.0, thumb_slot, 1.0, 1.0)
    {
        // Front-to-back: small app-icon badge + shadow -> live GPU thumbnail -> subtle frame + shadow.
        let frame_rect = Rectangle::new(
            Point::from((fit_rect.loc.x - 1, fit_rect.loc.y - 1)),
            Size::from((fit_rect.size.w + 2, fit_rect.size.h + 2)),
        );
        let badge_sz = (slot.icon_rect.size.w * 42 / 100).clamp(16, 24);
        let max_badge_x = slot.icon_rect.loc.x + slot.icon_rect.size.w - badge_sz + 2;
        let max_badge_y = slot.icon_rect.loc.y + slot.icon_rect.size.h - badge_sz + 2;
        let badge_x = (frame_rect.loc.x + frame_rect.size.w - badge_sz + 3).min(max_badge_x);
        let badge_y = (frame_rect.loc.y + frame_rect.size.h - badge_sz + 3).min(max_badge_y);
        if let Some(badge_icon) = render_dock_tile_icon(
            shape_cache,
            renderer,
            tile,
            badge_sz,
            Point::from((badge_x, badge_y)),
        ) {
            dock_icons.push(badge_icon);
            push_elevation_shadow(
                dock_icons,
                shape_cache,
                renderer,
                Rectangle::new(
                    Point::from((badge_x, badge_y)),
                    Size::from((badge_sz, badge_sz)),
                ),
                (badge_sz / 4).max(4),
                rusty_ui::shadow::ShadowElevation::MenuOrTooltip,
            );
        }
        dock_icons.push(CompositorRenderElement::ShaderTexture(thumb_el));
        push_rounded_glass_box(
            dock_icons,
            shape_cache,
            renderer,
            frame_rect,
            6,
            if pal.is_dark {
                [24, 26, 32, 224]
            } else {
                [248, 250, 254, 232]
            },
            Some(pal.border_highlight_rgba),
        );
        push_elevation_shadow(
            dock_icons,
            shape_cache,
            renderer,
            frame_rect,
            6,
            rusty_ui::shadow::ShadowElevation::MenuOrTooltip,
        );
        return;
    }

    let inset = (slot.icon_rect.size.w / 8).clamp(3, 6);
    let inner_sz = (slot.icon_rect.size.w - inset * 2).max(16);
    if let Some(el) = render_dock_tile_icon(
        shape_cache,
        renderer,
        tile,
        inner_sz,
        Point::from((slot.icon_rect.loc.x + inset, slot.icon_rect.loc.y + inset)),
    ) {
        dock_icons.push(el);
    }
    push_rounded_glass_box(
        dock_icons,
        shape_cache,
        renderer,
        slot.icon_rect,
        10,
        if pal.is_dark {
            [255, 255, 255, 26]
        } else {
            [255, 255, 255, 168]
        },
        Some(pal.border_highlight_rgba),
    );
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
                    [224, 232, 248, 190],
                    Some([255, 255, 255, 95]),
                );
            }
            let is_drop_target = ov.drag_target_workspace == Some(space.id);
            let is_space_hov = ov.hovered_space == Some(space.id);
            let thumb_border = if is_drop_target {
                [88, 178, 255, 255]
            } else if space.active {
                RGBA_ACCENT
            } else if is_space_hov {
                [215, 224, 240, 225]
            } else {
                RGBA_BORDER
            };
            let thumb_bg = if is_drop_target {
                [18, 96, 198, 238]
            } else if space.active {
                [26, 72, 138, 228]
            } else if is_space_hov {
                [58, 64, 76, 228]
            } else if space.window_count > 0 {
                [44, 48, 56, 214]
            } else {
                [32, 34, 40, 196]
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
            push_elevation_shadow(
                &mut out,
                shape_cache,
                renderer,
                thumb,
                8,
                rusty_ui::shadow::ShadowElevation::Popup,
            );
        }
        out.push(CompositorRenderElement::Solid(make_solid_element(
            Rectangle::new(Point::from((0, TOP_BAR_HEIGHT)), Size::from((width, 98))),
            premul(0.08, 0.09, 0.11, 0.28),
        )));
    }

    let cards = overview_card_rects(screen_size, ov.windows.len());
    #[allow(clippy::cast_possible_truncation)]
    let card_alpha = ov.progress.clamp(0.0, 1.0) as f32;
    for (idx, (win, base_card)) in ov.windows.iter().zip(cards).enumerate() {
        let is_dragging_this = ov
            .dragging_card
            .is_some_and(|(drag_idx, _)| drag_idx == idx);
        let drag_delta = ov
            .dragging_card
            .and_then(|(drag_idx, delta)| (drag_idx == idx).then_some(delta))
            .unwrap_or_else(|| Point::from((0, 0)));
        let card = Rectangle::new(
            Point::from((
                base_card.loc.x + drag_delta.x,
                base_card.loc.y + drag_delta.y + card_dy,
            )),
            base_card.size,
        );
        let is_hov = ov.hovered_card == Some(idx);
        let is_sel = ov.selected_card == Some(idx);
        let is_close_hov = ov.hovered_close == Some(idx);

        if (is_hov || is_sel) && !is_dragging_this {
            let close_rect = overview_card_close_rect(card);
            let close_bg = if is_close_hov {
                RGBA_DANGER
            } else {
                [42, 44, 50, 228]
            };
            push_rounded_glass_box(
                &mut out,
                shape_cache,
                renderer,
                close_rect,
                11,
                close_bg,
                Some([255, 255, 255, 64]),
            );
        }

        let thumb_slot = Rectangle::new(
            Point::from((card.loc.x + 8, card.loc.y + 8)),
            Size::from(((card.size.w - 16).max(1), (card.size.h - 16).max(1))),
        );
        let effective_alpha = if is_dragging_this {
            card_alpha * 0.85
        } else {
            card_alpha
        };
        if let Some(thumb_el) = shape_cache.render_overview_thumbnail_element(
            renderer,
            win.id.0,
            thumb_slot,
            1.0,
            effective_alpha,
        ) {
            out.push(CompositorRenderElement::ShaderTexture(thumb_el));
        }
        push_liquid_glass_panel(
            &mut out,
            shape_cache,
            renderer,
            card,
            14,
            crate::surfaces::GlassPanelStyle::Popover,
            top_telemetry.glass_opacity,
            top_telemetry.reduce_transparency,
        );
        if is_sel || is_dragging_this || (ov.selected_card.is_none() && win.focused) {
            push_rounded_glass_box(
                &mut out,
                shape_cache,
                renderer,
                Rectangle::new(
                    Point::from((card.loc.x - 2, card.loc.y - 2)),
                    Size::from((card.size.w + 4, card.size.h + 4)),
                ),
                15,
                if is_sel || is_dragging_this {
                    [10, 132, 255, 92]
                } else {
                    [10, 132, 255, 68]
                },
                Some([64, 156, 255, 245]),
            );
        } else if is_hov {
            push_rounded_glass_box(
                &mut out,
                shape_cache,
                renderer,
                Rectangle::new(
                    Point::from((card.loc.x - 2, card.loc.y - 2)),
                    Size::from((card.size.w + 4, card.size.h + 4)),
                ),
                15,
                [255, 255, 255, 26],
                Some([225, 232, 245, 195]),
            );
        }
        push_elevation_shadow(
            &mut out,
            shape_cache,
            renderer,
            card,
            14,
            rusty_ui::shadow::ShadowElevation::Popup,
        );
    }
    #[allow(clippy::cast_possible_truncation)]
    let scrim_a = 0.36 * (ov.progress.clamp(0.0, 1.0) as f32);
    out.push(CompositorRenderElement::Solid(make_solid_element(
        Rectangle::new(
            Point::from((0, TOP_BAR_HEIGHT)),
            Size::from((width, (height - TOP_BAR_HEIGHT).max(0))),
        ),
        premul(0.06, 0.07, 0.09, scrim_a),
    )));

    out
}
