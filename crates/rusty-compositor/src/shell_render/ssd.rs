//! Server-Side Decoration (SSD) titlebars, traffic lights, borders, and drop shadows.

use smithay::backend::renderer::Color32F;
use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
use smithay::backend::renderer::element::solid::SolidColorRenderElement;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::{Point, Rectangle, Size};

use super::CompositorRenderElement;
use super::tokens::{
    COLOR_BORDER, COLOR_DANGER, COLOR_MUTED, COLOR_SUBTLE, COLOR_SUCCESS, COLOR_WARNING,
    RESIZE_GRAB_BORDER, RGBA_BORDER, RGBA_BORDER_HIGHLIGHT, RGBA_SURFACE_ELEVATED,
    RGBA_SURFACE_OVERLAY, TITLEBAR_HEIGHT, WINDOW_RADIUS, blend_border_over_fill_color,
    make_solid_element, push_cpu_nine_slice_shadow, rgba_to_premul_color32f,
};

/// Active edges targeted by an interactive window resize (`RUSTYOS-022`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(clippy::struct_excessive_bools)]
pub struct ResizeEdges {
    /// Resize top edge.
    pub top: bool,
    /// Resize bottom edge.
    pub bottom: bool,
    /// Resize left edge.
    pub left: bool,
    /// Resize right edge.
    pub right: bool,
}

impl ResizeEdges {
    /// Returns `true` if at least one edge is active.
    #[must_use]
    pub const fn any(self) -> bool {
        self.top || self.bottom || self.left || self.right
    }
}

/// Interactive hit zone on a window's Server-Side Decoration frame (`RUSTYOS-019`/`022`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SsdHit {
    /// Red close button ("traffic light").
    Close(rusty_wm::WindowId),
    /// Amber minimize / snap-center button ("traffic light").
    Minimize(rusty_wm::WindowId),
    /// Green maximize / zoom button ("traffic light").
    Maximize(rusty_wm::WindowId),
    /// Remaining titlebar surface initiating window drag.
    TitlebarDrag(rusty_wm::WindowId),
    /// Outer border/corner band initiating window resize (`RUSTYOS-022`).
    Resize(rusty_wm::WindowId, ResizeEdges),
}

/// Window frame metadata used for SSD/CSD-aware pointer hit-testing (`RUSTYOS-030`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SsdHitWindow {
    /// Window manager identifier.
    pub id: rusty_wm::WindowId,
    /// Visual outer frame bounds.
    pub rect: rusty_wm::Rect,
    /// `true` when the compositor renders an SSD titlebar (`TITLEBAR_HEIGHT`);
    /// `false` when the client owns its own header/buttons (CSD).
    pub has_ssd: bool,
}

/// Hit-tests `(x, y)` against visible window frames in reverse (top-to-bottom) stacking order,
/// respecting CSD windows by forwarding top header / corner button clicks directly to the client.
#[must_use]
pub fn check_window_frame_hit(
    windows_bottom_to_top: &[SsdHitWindow],
    x: f64,
    y: f64,
) -> Option<SsdHit> {
    let pad = RESIZE_GRAB_BORDER;
    for &win in windows_bottom_to_top.iter().rev() {
        let rx = f64::from(win.rect.x);
        let ry = f64::from(win.rect.y);
        let rw = f64::from(win.rect.width);
        let rh = f64::from(win.rect.height);
        let th = f64::from(TITLEBAR_HEIGHT);

        if x < rx - pad || x >= rx + rw + pad || y < ry - pad || y >= ry + rh + pad {
            continue;
        }

        let rel_x = x - rx;
        let rel_y = y - ry;

        // 1. Traffic-light buttons take priority inside SSD titlebars only.
        if win.has_ssd {
            let ssd_tl = rusty_ui::TrafficLightButtons::default_ssd();
            if let Some(kind) = ssd_tl.hit_test(rel_x, rel_y) {
                return match kind {
                    rusty_ui::TrafficLightKind::Close => Some(SsdHit::Close(win.id)),
                    rusty_ui::TrafficLightKind::Minimize => Some(SsdHit::Minimize(win.id)),
                    rusty_ui::TrafficLightKind::Maximize => Some(SsdHit::Maximize(win.id)),
                };
            }
        }

        // 2. Outer frame edges & corners trigger interactive resize.
        // For CSD windows keep resize grabs outside the client surface (plus a 1px hairlip)
        // so interior header controls (such as corner `_ [] X` buttons) stay clickable.
        let inner_inset = if win.has_ssd { pad } else { 1.5 };
        let edges = ResizeEdges {
            left: rel_x < inner_inset,
            right: rel_x >= rw - inner_inset,
            top: rel_y < if win.has_ssd { pad - 1.0 } else { 1.5 },
            bottom: rel_y >= rh - inner_inset,
        };
        if edges.any() {
            return Some(SsdHit::Resize(win.id, edges));
        }

        // 3. Remaining SSD titlebar interior triggers move drag.
        if win.has_ssd && rel_x >= 0.0 && rel_x < rw && rel_y >= 0.0 && rel_y < th {
            return Some(SsdHit::TitlebarDrag(win.id));
        }

        // Click landed inside this foreground window's client surface; stop checking windows underneath.
        return None;
    }
    None
}

/// Hit-tests `(x, y)` against visible SSD window frames in reverse (top-to-bottom) stacking order.
#[must_use]
pub fn check_window_ssd_hit(
    windows_bottom_to_top: &[(rusty_wm::WindowId, rusty_wm::Rect)],
    x: f64,
    y: f64,
) -> Option<SsdHit> {
    let targets: Vec<SsdHitWindow> = windows_bottom_to_top
        .iter()
        .map(|&(id, rect)| SsdHitWindow {
            id,
            rect,
            has_ssd: true,
        })
        .collect();
    check_window_frame_hit(&targets, x, y)
}

/// Returns the topmost SSD window whose traffic-light cluster is hovered at `(x, y)` (`RUSTYOS-051`).
#[must_use]
pub fn check_window_traffic_lights_hover(
    windows_bottom_to_top: &[SsdHitWindow],
    x: f64,
    y: f64,
) -> Option<rusty_wm::WindowId> {
    match check_window_frame_hit(windows_bottom_to_top, x, y) {
        Some(SsdHit::Close(id) | SsdHit::Minimize(id) | SsdHit::Maximize(id)) => Some(id),
        _ => None,
    }
}

/// Generates Server-Side Decoration elements (titlebar, macOS traffic lights, 1px glass border)
/// for a managed window rectangle in Smithay front-to-back order (`RUSTYOS-019`).
#[must_use]
pub fn generate_window_ssd_elements(
    rect: rusty_wm::Rect,
    is_focused: bool,
    has_text: bool,
) -> Vec<SolidColorRenderElement> {
    let mut elements = Vec::new();
    let x = rect.x;
    let y = rect.y;
    let w = rect.width.max(64);
    let h = rect.height.max(TITLEBAR_HEIGHT + 32);

    let pal = super::tokens::ThemePalette::active();
    let (titlebar_rgba, border_rgba) = ssd_frame_colors(pal.is_dark, is_focused);
    let border_color = rgba_to_premul_color32f(border_rgba);
    let titlebar_bg = rgba_to_premul_color32f(titlebar_rgba);
    let divider_color = if pal.is_dark {
        COLOR_BORDER
    } else {
        pal.popover_separator
    };

    // 1. Outer 1px glass rim around the whole window frame (4 edge strips)
    elements.push(make_solid_element(
        Rectangle::new(Point::from((x - 1, y - 1)), Size::from((w + 2, 1))),
        border_color,
    ));
    elements.push(make_solid_element(
        Rectangle::new(Point::from((x - 1, y + h)), Size::from((w + 2, 1))),
        border_color,
    ));
    elements.push(make_solid_element(
        Rectangle::new(Point::from((x - 1, y)), Size::from((1, h))),
        border_color,
    ));
    elements.push(make_solid_element(
        Rectangle::new(Point::from((x + w, y)), Size::from((1, h))),
        border_color,
    ));

    // 2. Titlebar body & bottom divider line
    elements.push(make_solid_element(
        Rectangle::new(Point::from((x, y)), Size::from((w, TITLEBAR_HEIGHT))),
        titlebar_bg,
    ));
    elements.push(make_solid_element(
        Rectangle::new(
            Point::from((x, y + TITLEBAR_HEIGHT - 1)),
            Size::from((w, 1)),
        ),
        divider_color,
    ));

    // 3. Traffic light buttons (Close, Minimize, Maximize)
    let ssd_tl = rusty_ui::TrafficLightButtons::default_ssd();
    let btn_size = i32::try_from(ssd_tl.close_rect.width).unwrap_or(14);
    let close_color = if is_focused {
        COLOR_DANGER
    } else {
        COLOR_SUBTLE
    };
    let min_color = if is_focused {
        COLOR_WARNING
    } else {
        COLOR_SUBTLE
    };
    let max_color = if is_focused {
        COLOR_SUCCESS
    } else {
        COLOR_SUBTLE
    };

    elements.push(make_solid_element(
        Rectangle::new(
            Point::from((x + ssd_tl.close_rect.x, y + ssd_tl.close_rect.y)),
            Size::from((btn_size, btn_size)),
        ),
        close_color,
    ));
    elements.push(make_solid_element(
        Rectangle::new(
            Point::from((x + ssd_tl.minimize_rect.x, y + ssd_tl.minimize_rect.y)),
            Size::from((btn_size, btn_size)),
        ),
        min_color,
    ));
    elements.push(make_solid_element(
        Rectangle::new(
            Point::from((x + ssd_tl.maximize_rect.x, y + ssd_tl.maximize_rect.y)),
            Size::from((btn_size, btn_size)),
        ),
        max_color,
    ));

    // 4. Centered title pill bar (when no font renderer is active)
    let title_w = (w / 4).clamp(48, 160);
    if !has_text && w > 140 {
        let title_x = x + (w - title_w) / 2;
        elements.push(make_solid_element(
            Rectangle::new(Point::from((title_x, y + 11)), Size::from((title_w, 6))),
            ssd_title_pill_color(&pal, is_focused),
        ));
    }

    // Smithay render_output expects front-to-back order.
    elements.reverse();
    elements
}

fn ssd_title_pill_color(pal: &super::tokens::ThemePalette, is_focused: bool) -> Color32F {
    if pal.is_dark {
        if is_focused {
            COLOR_MUTED
        } else {
            COLOR_SUBTLE
        }
    } else {
        rgba_to_premul_color32f(if is_focused {
            pal.popover_text_secondary
        } else {
            pal.popover_text_muted
        })
    }
}

const fn ssd_frame_colors(is_dark: bool, is_focused: bool) -> ([u8; 4], [u8; 4]) {
    if is_dark {
        // Neutral macOS Dark header tones matching RustyOS Dark (#1C1C1D) & Chromium (#202124).
        let titlebar = if is_focused {
            RGBA_SURFACE_OVERLAY
        } else {
            RGBA_SURFACE_ELEVATED
        };
        let border = if is_focused {
            RGBA_BORDER_HIGHLIGHT
        } else {
            RGBA_BORDER
        };
        (titlebar, border)
    } else {
        let titlebar = if is_focused {
            [246, 246, 248, 250]
        } else {
            [236, 237, 240, 246]
        };
        let border = if is_focused {
            [0, 0, 0, 24]
        } else {
            [0, 0, 0, 15]
        };
        (titlebar, border)
    }
}

/// Generates centered title text element for a window's SSD titlebar (`RUSTYOS-020`).
pub fn generate_ssd_title_element(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    rect: rusty_wm::Rect,
    title: &str,
    is_focused: bool,
) -> Option<MemoryRenderBufferRenderElement<GlesRenderer>> {
    if !text_cache.has_font() || rect.width < 120 {
        return None;
    }
    let pal = super::tokens::ThemePalette::active();
    let color = if is_focused {
        pal.popover_text_primary
    } else {
        pal.popover_text_muted
    };
    let bounds = Rectangle::new(
        Point::from((rect.x + 70, rect.y)),
        Size::from((rect.width - 140, TITLEBAR_HEIGHT)),
    );
    text_cache.render_label_centered_weighted(
        renderer,
        title,
        crate::text::FontWeight::Bold,
        12.5,
        color,
        bounds,
    )
}

/// Generates rounded SSD titlebar, gel-lit macOS traffic lights (with optional hover glyphs),
/// and 4-corner rounded 1px glass rim (`RUSTYOS-022`/`051`).
#[must_use]
pub fn generate_window_ssd_composite_elements(
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    rect: rusty_wm::Rect,
    title: &str,
    is_focused: bool,
) -> Vec<CompositorRenderElement> {
    generate_window_ssd_composite_elements_with_hover(
        text_cache,
        shape_cache,
        renderer,
        rect,
        title,
        is_focused,
        false,
    )
}

/// Generates rounded SSD titlebar, gel-lit macOS traffic lights, optional hover glyphs (`×`, `−`, `+`),
/// and 4-corner `WINDOW_RADIUS` rounded 1px frame rim (`RUSTYOS-051`).
#[allow(clippy::many_single_char_names, clippy::too_many_lines)]
pub fn generate_window_ssd_composite_elements_with_hover(
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    rect: rusty_wm::Rect,
    title: &str,
    is_focused: bool,
    traffic_lights_hovered: bool,
) -> Vec<CompositorRenderElement> {
    use crate::surfaces::{CornerQuadrant, TrafficLightKind};

    let mut out = Vec::new();
    let x = rect.x;
    let y = rect.y;
    let w = rect.width.max(64);
    let h = rect.height.max(TITLEBAR_HEIGHT + 32);

    if let Some(title_el) =
        generate_ssd_title_element(text_cache, renderer, rect, title, is_focused)
    {
        out.push(CompositorRenderElement::Memory(title_el));
    }

    // 4x4 SSAA gel-lit traffic lights with hover glyphs (`Close`, `Minimize`, `Maximize`)
    let ssd_tl = rusty_ui::TrafficLightButtons::default_ssd();
    let btn_y = y + ssd_tl.close_rect.y;
    let btn_d = i32::try_from(ssd_tl.close_rect.width).unwrap_or(14);
    for (bx, kind) in [
        (x + ssd_tl.close_rect.x, TrafficLightKind::Close),
        (x + ssd_tl.minimize_rect.x, TrafficLightKind::Minimize),
        (x + ssd_tl.maximize_rect.x, TrafficLightKind::Maximize),
    ] {
        if let Some(el) = shape_cache.traffic_light_element(
            renderer,
            kind,
            btn_d,
            is_focused,
            traffic_lights_hovered,
            Point::from((bx, btn_y)),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
    }

    // Rounded top & bottom corners (`WINDOW_RADIUS = 14`) + crisp 1px glass rim
    let r = WINDOW_RADIUS.min(w / 2).min(h / 2).max(4);
    let pal = super::tokens::ThemePalette::active();
    let (titlebar_rgba, border_rgba) = ssd_frame_colors(pal.is_dark, is_focused);
    let spec_alpha = if pal.is_dark {
        if is_focused { 30 } else { 14 }
    } else if is_focused {
        160
    } else {
        90
    };
    let divider_col = if pal.is_dark {
        COLOR_BORDER
    } else {
        pal.popover_separator
    };
    let titlebar_fill = rgba_to_premul_color32f(titlebar_rgba);
    let top_rim_col = blend_border_over_fill_color(border_rgba, titlebar_rgba);
    let edge_border_col = rgba_to_premul_color32f(border_rgba);

    for (quad, fill, loc) in [
        (CornerQuadrant::TopLeft, titlebar_rgba, Point::from((x, y))),
        (
            CornerQuadrant::TopRight,
            titlebar_rgba,
            Point::from((x + w - r, y)),
        ),
        (
            CornerQuadrant::BottomLeft,
            [0, 0, 0, 0],
            Point::from((x, y + h - r)),
        ),
        (
            CornerQuadrant::BottomRight,
            [0, 0, 0, 0],
            Point::from((x + w - r, y + h - r)),
        ),
    ] {
        if let Some(el) =
            shape_cache.corner_element(renderer, r, quad, fill, Some(border_rgba), loc)
        {
            out.push(CompositorRenderElement::Memory(el));
        }
    }

    // Top rim + subtle top specular highlight + straight side/bottom 1px frame segments
    if w > 2 * r {
        out.push(CompositorRenderElement::Solid(make_solid_element(
            Rectangle::new(Point::from((x + r, y)), Size::from((w - 2 * r, 1))),
            top_rim_col,
        )));
        out.push(CompositorRenderElement::Solid(make_solid_element(
            Rectangle::new(Point::from((x + r, y + 1)), Size::from((w - 2 * r, 1))),
            rgba_to_premul_color32f([255, 255, 255, spec_alpha]),
        )));
        if r > 2 {
            out.push(CompositorRenderElement::Solid(make_solid_element(
                Rectangle::new(Point::from((x + r, y + 2)), Size::from((w - 2 * r, r - 2))),
                titlebar_fill,
            )));
        }
        out.push(CompositorRenderElement::Solid(make_solid_element(
            Rectangle::new(Point::from((x + r, y + h - 1)), Size::from((w - 2 * r, 1))),
            edge_border_col,
        )));
    }
    if TITLEBAR_HEIGHT > r + 1 {
        out.push(CompositorRenderElement::Solid(make_solid_element(
            Rectangle::new(
                Point::from((x + 1, y + r)),
                Size::from((w - 2, TITLEBAR_HEIGHT - r - 1)),
            ),
            titlebar_fill,
        )));
    }
    out.push(CompositorRenderElement::Solid(make_solid_element(
        Rectangle::new(
            Point::from((x + 1, y + TITLEBAR_HEIGHT - 1)),
            Size::from((w - 2, 1)),
        ),
        divider_col,
    )));
    if h > 2 * r {
        out.push(CompositorRenderElement::Solid(make_solid_element(
            Rectangle::new(Point::from((x, y + r)), Size::from((1, h - 2 * r))),
            edge_border_col,
        )));
        out.push(CompositorRenderElement::Solid(make_solid_element(
            Rectangle::new(Point::from((x + w - 1, y + r)), Size::from((1, h - 2 * r))),
            edge_border_col,
        )));
    }

    out
}

/// Generates a seamless 3-lobe drop shadow behind a window frame (`RUSTYOS-075`, `RUSTYOS-128`).
pub fn generate_window_shadow_elements(
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    rect: rusty_wm::Rect,
    is_focused: bool,
    shadow_mode: crate::config::WindowShadowMode,
) -> Vec<CompositorRenderElement> {
    generate_window_shadow_elements_with_opacity(
        shape_cache,
        renderer,
        rect,
        is_focused,
        shadow_mode,
        1.0,
    )
}

/// Generates a seamless 3-lobe drop shadow behind a window frame scaled by `opacity_scale` (`RUSTYOS-128`).
pub fn generate_window_shadow_elements_with_opacity(
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    rect: rusty_wm::Rect,
    is_focused: bool,
    shadow_mode: crate::config::WindowShadowMode,
    opacity_scale: f32,
) -> Vec<CompositorRenderElement> {
    let mut out = Vec::new();
    let opacity = opacity_scale.clamp(0.0, 1.0);
    if shadow_mode == crate::config::WindowShadowMode::None || opacity <= 0.002 {
        return out;
    }
    let w = rect.width.max(64);
    let h = rect.height.max(TITLEBAR_HEIGHT + 32);
    let win_logical = Rectangle::new(Point::from((rect.x, rect.y)), Size::from((w, h)));
    if let Some(el) = shape_cache.window_shadow_render_element_with_opacity(
        renderer,
        win_logical,
        WINDOW_RADIUS,
        is_focused,
        shadow_mode,
        opacity,
    ) {
        out.push(el);
        return out;
    }

    let elevation = if is_focused && opacity >= 0.65 {
        rusty_ui::shadow::ShadowElevation::ActiveWindow
    } else {
        rusty_ui::shadow::ShadowElevation::InactiveWindow
    };
    push_cpu_nine_slice_shadow(
        &mut out,
        shape_cache,
        renderer,
        Rectangle::new(Point::from((rect.x, rect.y + 6)), Size::from((w, h))),
        WINDOW_RADIUS,
        elevation,
    );
    out
}
