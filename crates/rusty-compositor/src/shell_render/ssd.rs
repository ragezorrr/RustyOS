//! Server-Side Decoration (SSD) titlebars, traffic lights, borders, and drop shadows.

use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
use smithay::backend::renderer::element::solid::SolidColorRenderElement;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::{Point, Rectangle, Size};

use super::CompositorRenderElement;
use super::tokens::{
    COLOR_BORDER, COLOR_BORDER_HIGHLIGHT, COLOR_DANGER, COLOR_MUTED, COLOR_SUBTLE, COLOR_SUCCESS,
    COLOR_SURFACE_ELEVATED, COLOR_SURFACE_OVERLAY, COLOR_WARNING, RESIZE_GRAB_BORDER, RGBA_BORDER,
    RGBA_BORDER_HIGHLIGHT, RGBA_SURFACE_ELEVATED, RGBA_SURFACE_OVERLAY, TITLEBAR_HEIGHT,
    WINDOW_RADIUS, blend_border_over_fill_color, make_solid_element, push_soft_shadow,
    rgba_to_premul_color32f,
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
        if win.has_ssd && (6.0..=22.0).contains(&rel_y) {
            if (10.0..=26.0).contains(&rel_x) {
                return Some(SsdHit::Close(win.id));
            }
            if (28.0..=44.0).contains(&rel_x) {
                return Some(SsdHit::Minimize(win.id));
            }
            if (46.0..=62.0).contains(&rel_x) {
                return Some(SsdHit::Maximize(win.id));
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

    let border_color = if is_focused {
        COLOR_BORDER_HIGHLIGHT
    } else {
        COLOR_BORDER
    };
    let titlebar_bg = if is_focused {
        COLOR_SURFACE_OVERLAY
    } else {
        COLOR_SURFACE_ELEVATED
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
        COLOR_BORDER,
    ));

    // 3. macOS-style traffic light buttons (Close, Minimize, Maximize)
    let btn_y = y + 8;
    let btn_size = 12;
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
            Point::from((x + 12, btn_y)),
            Size::from((btn_size, btn_size)),
        ),
        close_color,
    ));
    elements.push(make_solid_element(
        Rectangle::new(
            Point::from((x + 30, btn_y)),
            Size::from((btn_size, btn_size)),
        ),
        min_color,
    ));
    elements.push(make_solid_element(
        Rectangle::new(
            Point::from((x + 48, btn_y)),
            Size::from((btn_size, btn_size)),
        ),
        max_color,
    ));

    // 4. Centered title pill bar (when no font renderer is active)
    let title_w = (w / 4).clamp(48, 160);
    if !has_text && w > 140 {
        let title_x = x + (w - title_w) / 2;
        let title_color = if is_focused {
            COLOR_MUTED
        } else {
            COLOR_SUBTLE
        };
        elements.push(make_solid_element(
            Rectangle::new(Point::from((title_x, y + 11)), Size::from((title_w, 6))),
            title_color,
        ));
    }

    // Smithay render_output expects front-to-back order.
    elements.reverse();
    elements
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
    let color = if is_focused {
        [244, 240, 250, 255] // #F4F0FA
    } else {
        [166, 155, 181, 255] // #A69BB5
    };
    let bounds = Rectangle::new(
        Point::from((rect.x + 70, rect.y)),
        Size::from((rect.width - 140, TITLEBAR_HEIGHT)),
    );
    text_cache.render_label_centered(renderer, title, 12.0, color, bounds)
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
    let btn_y = y + 8;
    let btn_d = 12;
    for (bx, kind) in [
        (x + 12, TrafficLightKind::Close),
        (x + 30, TrafficLightKind::Minimize),
        (x + 48, TrafficLightKind::Maximize),
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

    // Rounded top & bottom corners (`WINDOW_RADIUS = 20`) + crisp 1px glass rim
    let r = WINDOW_RADIUS.min(w / 2).min(h / 2).max(4);
    let titlebar_rgba = if is_focused {
        RGBA_SURFACE_OVERLAY
    } else {
        RGBA_SURFACE_ELEVATED
    };
    let border_rgba = if is_focused {
        RGBA_BORDER_HIGHLIGHT
    } else {
        RGBA_BORDER
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
            rgba_to_premul_color32f([255, 252, 255, if is_focused { 30 } else { 14 }]),
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
        COLOR_BORDER,
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

/// Generates layered macOS ambient + contact drop shadows strictly outside a window frame (`RUSTYOS-022`/`051`).
pub fn generate_window_shadow_elements(
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    rect: rusty_wm::Rect,
    is_focused: bool,
) -> Vec<CompositorRenderElement> {
    let mut out = Vec::new();
    let w = rect.width.max(64);
    let h = rect.height.max(TITLEBAR_HEIGHT + 32);
    if is_focused {
        push_soft_shadow(
            &mut out,
            shape_cache,
            renderer,
            Rectangle::new(Point::from((rect.x, rect.y + 2)), Size::from((w, h))),
            WINDOW_RADIUS,
            8,
            40,
        );
        push_soft_shadow(
            &mut out,
            shape_cache,
            renderer,
            Rectangle::new(Point::from((rect.x, rect.y + 6)), Size::from((w, h))),
            WINDOW_RADIUS,
            26,
            36,
        );
    } else {
        push_soft_shadow(
            &mut out,
            shape_cache,
            renderer,
            Rectangle::new(Point::from((rect.x, rect.y + 2)), Size::from((w, h))),
            WINDOW_RADIUS,
            6,
            20,
        );
        push_soft_shadow(
            &mut out,
            shape_cache,
            renderer,
            Rectangle::new(Point::from((rect.x, rect.y + 4)), Size::from((w, h))),
            WINDOW_RADIUS,
            14,
            22,
        );
    }
    out
}
