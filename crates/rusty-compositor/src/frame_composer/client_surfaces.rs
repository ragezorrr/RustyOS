//! Client Wayland surface, Liquid Glass, popup, and XWayland override-redirect composition.

use smithay::{
    backend::renderer::{
        element::AsRenderElements,
        element::Kind,
        element::surface::{WaylandSurfaceRenderElement, render_elements_from_surface_tree},
        gles::GlesRenderer,
    },
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point, Rectangle, Size},
};

use super::portal_capture::should_skip_uncommitted_bound_window;
use crate::{shell_render::CompositorRenderElement, state::RustyCompositorState};

const fn map_client_glass_style(
    preset: crate::state::liquid_glass::GlassStylePreset,
    tint: Option<[u8; 4]>,
) -> crate::surfaces::GlassPanelStyle {
    match preset {
        crate::state::liquid_glass::GlassStylePreset::Sidebar => {
            crate::surfaces::GlassPanelStyle::Sidebar
        }
        crate::state::liquid_glass::GlassStylePreset::ControlPill => {
            crate::surfaces::GlassPanelStyle::ControlModule
        }
        crate::state::liquid_glass::GlassStylePreset::BasePanel => {
            crate::surfaces::GlassPanelStyle::Dock
        }
        crate::state::liquid_glass::GlassStylePreset::Popover => {
            crate::surfaces::GlassPanelStyle::Popover
        }
        crate::state::liquid_glass::GlassStylePreset::Custom => {
            crate::surfaces::GlassPanelStyle::Custom(tint)
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::cast_possible_wrap)]
pub(super) fn push_window_liquid_glass_elements(
    backdrop_list: &mut Vec<CompositorRenderElement>,
    renderer: &mut GlesRenderer,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    state: &RustyCompositorState,
    sw: &smithay::desktop::Window,
    origin: (i32, i32),
    titlebar_h: i32,
    backdrop_offset: Point<i32, Logical>,
) {
    use smithay::wayland::seat::WaylandFocus;

    let Some(root) = sw.wl_surface() else {
        return;
    };

    let win_sz = sw.geometry().size;
    // Main window surface (e.g. sidebar or subregions); popup surfaces are composited separately
    // above backdrop elements so their Liquid Glass samples the real blurred scene (`RUSTYOS-107`).
    let Some(glass) = crate::state::liquid_glass::surface_liquid_glass_state(&root) else {
        return;
    };
    let base_loc = Point::<i32, Logical>::from((origin.0, origin.1 + titlebar_h));
    let style = map_client_glass_style(glass.style, glass.tint);
    let radius = if glass.corner_radius > 0 {
        glass.corner_radius as i32
    } else {
        crate::shell_render::WINDOW_RADIUS
    };

    if let Some(ref regions) = glass.region {
        for rect in regions {
            let draw_loc = base_loc + rect.loc;
            if let Some(elem) = shape_cache.glass_panel_render_element_at(
                renderer,
                style,
                rect.size.w,
                rect.size.h,
                radius,
                state.glass_opacity,
                state.reduce_transparency,
                draw_loc,
                draw_loc + backdrop_offset,
            ) {
                backdrop_list.push(elem);
            }
        }
    } else if let Some(elem) = shape_cache.glass_panel_render_element_at(
        renderer,
        style,
        win_sz.w,
        win_sz.h,
        radius,
        state.glass_opacity,
        state.reduce_transparency,
        base_loc,
        base_loc + backdrop_offset,
    ) {
        backdrop_list.push(elem);
    }
}

#[allow(clippy::cast_possible_wrap)]
pub(super) fn push_popup_glass_panels(
    render_list: &mut Vec<CompositorRenderElement>,
    renderer: &mut GlesRenderer,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    state: &RustyCompositorState,
    popup_surface: &WlSurface,
    popup_size: Size<i32, Logical>,
    popup_logical_loc: Point<i32, Logical>,
) {
    let Some(glass) = crate::state::liquid_glass::surface_liquid_glass_state(popup_surface) else {
        return;
    };
    let style = map_client_glass_style(glass.style, glass.tint);
    let radius = if glass.corner_radius > 0 {
        glass.corner_radius as i32
    } else {
        rusty_ui::menu::MENU_RADIUS
    };
    if let Some(ref regions) = glass.region {
        for region_rect in regions {
            let panel_rect = Rectangle::new(
                Point::from((
                    popup_logical_loc.x + region_rect.loc.x,
                    popup_logical_loc.y + region_rect.loc.y,
                )),
                Size::from((region_rect.size.w.max(1), region_rect.size.h.max(1))),
            );
            crate::shell_render::push_liquid_glass_panel_elevation(
                render_list,
                shape_cache,
                renderer,
                panel_rect,
                radius,
                style,
                state.glass_opacity,
                state.reduce_transparency,
                rusty_ui::shadow::ShadowElevation::MenuOrTooltip,
            );
        }
    } else {
        let panel_rect = Rectangle::new(
            Point::from((popup_logical_loc.x, popup_logical_loc.y)),
            Size::from((popup_size.w.max(1), popup_size.h.max(1))),
        );
        crate::shell_render::push_liquid_glass_panel_elevation(
            render_list,
            shape_cache,
            renderer,
            panel_rect,
            radius,
            style,
            state.glass_opacity,
            state.reduce_transparency,
            rusty_ui::shadow::ShadowElevation::MenuOrTooltip,
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn push_client_popup_elements(
    render_list: &mut Vec<CompositorRenderElement>,
    renderer: &mut GlesRenderer,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    state: &RustyCompositorState,
    output_loc: Point<i32, Logical>,
    output_scale: f64,
    output_windows: &[(rusty_wm::WindowId, rusty_wm::Rect)],
    fullscreen_win: Option<rusty_wm::WindowId>,
) {
    use smithay::wayland::seat::WaylandFocus;

    for &(win_id, rect) in output_windows.iter().rev() {
        if state.is_window_minimized(win_id)
            || state
                .minimize_anims
                .iter()
                .any(|anim| anim.win_id == win_id)
            || fullscreen_win.is_some_and(|fs_id| win_id != fs_id)
        {
            continue;
        }
        let Some(sw) = state.smithay_window_for_id(win_id) else {
            continue;
        };
        let Some(root) = sw.wl_surface() else {
            continue;
        };
        let global_origin = state
            .animations
            .animated_window_origin(win_id.0)
            .unwrap_or((rect.x, rect.y));
        let origin = (
            global_origin.0 - output_loc.x,
            global_origin.1 - output_loc.y,
        );
        let is_fullscreen = fullscreen_win == Some(win_id) || state.is_window_fullscreen(win_id);
        let titlebar_h = if !is_fullscreen && !state.window_uses_csd(win_id) {
            crate::shell_render::TITLEBAR_HEIGHT
        } else {
            0
        };
        let geo_loc = sw.geometry().loc;
        let client_loc =
            Point::<i32, Logical>::from((origin.0 - geo_loc.x, origin.1 + titlebar_h - geo_loc.y))
                .to_physical_precise_round(output_scale);

        // Render nested child submenus before their parent popups so child submenus appear on top
        // in Smithay's front-to-back render list (`RUSTYOS-165`).
        let popups: Vec<_> = smithay::desktop::PopupManager::popups_for_surface(&root).collect();
        for (popup, popup_offset) in popups.into_iter().rev() {
            let popup_surface = popup.wl_surface();
            let offset = geo_loc + popup_offset - popup.geometry().loc;
            let phys_loc = client_loc + offset.to_physical_precise_round(output_scale);
            render_list.extend(
                render_elements_from_surface_tree::<
                    GlesRenderer,
                    WaylandSurfaceRenderElement<GlesRenderer>,
                >(
                    renderer,
                    popup_surface,
                    phys_loc,
                    smithay::utils::Scale::from(output_scale),
                    1.0,
                    Kind::Unspecified,
                )
                .into_iter()
                .map(CompositorRenderElement::Surface),
            );
            let popup_logical_loc = Point::<i32, Logical>::from((
                origin.0 - geo_loc.x + offset.x,
                origin.1 + titlebar_h - geo_loc.y + offset.y,
            ));
            push_popup_glass_panels(
                render_list,
                renderer,
                shape_cache,
                state,
                popup_surface,
                popup.geometry().size,
                popup_logical_loc,
            );
        }
    }
}

/// Appends visible XWayland override-redirect surfaces (menus, dropdowns, tooltips) intersecting
/// `output_geo` in front-to-back order (`RUSTYOS-165`).
pub(super) fn push_xwayland_override_redirect_elements(
    render_list: &mut Vec<CompositorRenderElement>,
    renderer: &mut GlesRenderer,
    state: &RustyCompositorState,
    output_geo: smithay::utils::Rectangle<i32, Logical>,
    output_scale: f64,
) {
    for el in state.space.elements().rev() {
        let Some(x11) = el.x11_surface() else {
            continue;
        };
        if !x11.is_override_redirect() {
            continue;
        }
        let Some(loc) = state.space.element_location(el) else {
            continue;
        };
        let bbox = smithay::utils::Rectangle::new(loc, el.geometry().size);
        if !bbox.overlaps(output_geo) {
            continue;
        }
        let Some(root) = smithay::wayland::seat::WaylandFocus::wl_surface(el) else {
            continue;
        };
        let render_loc = loc - el.geometry().loc - output_geo.loc;
        let phys_loc = render_loc.to_physical_precise_round(output_scale);
        render_list.extend(
            render_elements_from_surface_tree::<
                GlesRenderer,
                WaylandSurfaceRenderElement<GlesRenderer>,
            >(
                renderer,
                &root,
                phys_loc,
                smithay::utils::Scale::from(output_scale),
                1.0,
                Kind::Unspecified,
            )
            .into_iter()
            .map(CompositorRenderElement::Surface),
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn push_window_backdrop_elements(
    backdrop_list: &mut Vec<CompositorRenderElement>,
    renderer: &mut GlesRenderer,
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    state: &RustyCompositorState,
    win_id: rusty_wm::WindowId,
    rect: rusty_wm::Rect,
    output_loc: Point<i32, Logical>,
    output_scale: f64,
    is_focused: bool,
    is_fullscreen: bool,
) {
    push_window_elements_with_options(
        backdrop_list,
        renderer,
        text_cache,
        shape_cache,
        state,
        win_id,
        rect,
        output_loc,
        output_scale,
        is_focused,
        is_fullscreen,
        true,
        Point::from((0, 0)),
    );
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(super) fn push_window_elements_with_options(
    backdrop_list: &mut Vec<CompositorRenderElement>,
    renderer: &mut GlesRenderer,
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    state: &RustyCompositorState,
    win_id: rusty_wm::WindowId,
    rect: rusty_wm::Rect,
    output_loc: Point<i32, Logical>,
    output_scale: f64,
    is_focused: bool,
    is_fullscreen: bool,
    include_shadow: bool,
    glass_backdrop_offset: Point<i32, Logical>,
) {
    let global_origin = state
        .animations
        .animated_window_origin(win_id.0)
        .unwrap_or((rect.x, rect.y));
    let mut origin = (
        global_origin.0 - output_loc.x,
        global_origin.1 - output_loc.y,
    );
    let has_ssd = !is_fullscreen && !state.window_uses_csd(win_id);
    let titlebar_h = if has_ssd {
        crate::shell_render::TITLEBAR_HEIGHT
    } else {
        0
    };
    let active_resize = state
        .active_window_resize
        .filter(|resize| resize.window_id == win_id);
    let actively_resizing = active_resize.is_some();

    let smithay_win = state.smithay_window_for_id(win_id);
    let committed_size = smithay_win.as_ref().map(|w| w.geometry().size);
    // Anchor-lock top/left interactive resize: keep the opposite right/bottom edge pinned to
    // `resize.start_rect` while waiting for the client to commit the resized buffer (`RUSTYOS-166`).
    if let (Some(resize), Some(sz)) = (active_resize, committed_size)
        && sz.w >= 64
        && sz.h >= 64
    {
        let frame_w = sz.w;
        let frame_h = sz.h + titlebar_h;
        if resize.edges.left {
            let right_edge = resize.start_rect.x + resize.start_rect.width - output_loc.x;
            origin.0 = right_edge - frame_w;
        }
        if resize.edges.top {
            let bottom_edge = resize.start_rect.y + resize.start_rect.height - output_loc.y;
            origin.1 = bottom_edge - frame_h;
        }
    }
    let frame_rect = match (state.wm.active_layout_mode(), committed_size) {
        (rusty_wm::LayoutMode::Floating, Some(sz)) if sz.w >= 64 && sz.h >= 64 => {
            rusty_wm::Rect::new(origin.0, origin.1, sz.w, sz.h + titlebar_h)
        }
        _ => rusty_wm::Rect::new(origin.0, origin.1, rect.width, rect.height),
    };
    let _ = actively_resizing;
    let square_framed = is_fullscreen || state.window_is_square_framed(win_id);

    let has_open_popup = smithay_win
        .as_ref()
        .and_then(smithay::wayland::seat::WaylandFocus::wl_surface)
        .is_some_and(|root| {
            smithay::desktop::PopupManager::popups_for_surface(&root)
                .next()
                .is_some()
        });

    let mut surface_elems: Vec<CompositorRenderElement> = Vec::new();
    if let Some(ref sw) = smithay_win {
        let geo_loc = sw.geometry().loc;
        let client_loc =
            Point::<i32, Logical>::from((origin.0 - geo_loc.x, origin.1 + titlebar_h - geo_loc.y))
                .to_physical_precise_round(output_scale);
        let raw_elems = if has_open_popup
            && let Some(root) = smithay::wayland::seat::WaylandFocus::wl_surface(sw)
        {
            render_elements_from_surface_tree::<
                GlesRenderer,
                WaylandSurfaceRenderElement<GlesRenderer>,
            >(
                renderer,
                &root,
                client_loc,
                smithay::utils::Scale::from(output_scale),
                1.0,
                Kind::Unspecified,
            )
        } else {
            <smithay::desktop::Window as AsRenderElements<GlesRenderer>>::render_elements::<
                WaylandSurfaceRenderElement<GlesRenderer>,
            >(
                sw,
                renderer,
                client_loc,
                smithay::utils::Scale::from(output_scale),
                1.0,
            )
        };
        if square_framed {
            surface_elems.extend(raw_elems.into_iter().map(CompositorRenderElement::Surface));
        } else {
            let clip_p0: Point<i32, smithay::utils::Physical> =
                Point::<i32, Logical>::from((frame_rect.x, frame_rect.y))
                    .to_physical_precise_round(output_scale);
            let clip_p1: Point<i32, smithay::utils::Physical> = Point::<i32, Logical>::from((
                frame_rect.x + frame_rect.width,
                frame_rect.y + frame_rect.height,
            ))
            .to_physical_precise_round(output_scale);
            let clip_rect_phys = Rectangle::new(
                clip_p0,
                Size::from((
                    (clip_p1.x - clip_p0.x).max(1),
                    (clip_p1.y - clip_p0.y).max(1),
                )),
            );
            #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
            let clip_radius_phys = crate::shell_render::WINDOW_RADIUS as f32
                * (crate::config::clamp_display_scale(output_scale) as f32);
            surface_elems.extend(raw_elems.into_iter().map(|el| {
                shape_cache.clip_surface_element(
                    renderer,
                    el,
                    clip_rect_phys,
                    clip_radius_phys,
                    1.0,
                )
            }));
        }
        if should_skip_uncommitted_bound_window(!surface_elems.is_empty(), committed_size) {
            return;
        }
    }

    // Always render SSD titlebar/chrome in front of the client surface; xdg_popups render in their
    // own dedicated pass above all windows (`push_client_popup_elements`) (`RUSTYOS-165`).
    let ssd_hovered = state.hovered_traffic_light_window == Some(win_id);
    if has_ssd {
        let title_text = state.window_title(win_id);
        backdrop_list.extend(
            crate::shell_render::generate_window_ssd_composite_elements_with_options(
                text_cache,
                shape_cache,
                renderer,
                frame_rect,
                &title_text,
                is_focused,
                ssd_hovered,
                square_framed,
            ),
        );
    }

    backdrop_list.extend(surface_elems);
    if let Some(ref sw) = smithay_win {
        push_window_liquid_glass_elements(
            backdrop_list,
            renderer,
            shape_cache,
            state,
            sw,
            origin,
            titlebar_h,
            glass_backdrop_offset,
        );
    }

    if include_shadow
        && !square_framed
        && state.user_config.window_shadow != crate::config::WindowShadowMode::None
    {
        backdrop_list.extend(crate::shell_render::generate_window_shadow_elements(
            shape_cache,
            renderer,
            frame_rect,
            is_focused,
            state.user_config.window_shadow,
        ));
    }
}

pub(super) const fn window_glass_backdrop_slot(output_id: u32, win_id: u64) -> u64 {
    0x8000_0000_0000_0000 | ((output_id as u64) << 40) | (win_id & 0x00FF_FFFF_FFFF)
}
