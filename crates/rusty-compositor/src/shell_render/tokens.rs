//! Liquid Glass design tokens, layout constants, geometry helpers, and SDF panel primitives.

use std::cell::RefCell;
use std::collections::HashMap;

use smithay::backend::renderer::Color32F;
use smithay::backend::renderer::element::solid::SolidColorRenderElement;
use smithay::backend::renderer::element::{Id, Kind};
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::renderer::utils::CommitCounter;
use smithay::utils::{Point, Rectangle, Size};

use super::CompositorRenderElement;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct SolidSlotKey {
    output_id: u32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    occurrence: u16,
}

#[derive(Debug)]
struct CachedSolidEntry {
    id: Id,
    commit: CommitCounter,
    color_bits: [u32; 4],
    last_epoch: u64,
}

#[derive(Debug, Default)]
struct SolidElementCache {
    active_output_id: u32,
    epoch: u64,
    frame_counts: HashMap<(i32, i32, i32, i32), u16>,
    entries: HashMap<SolidSlotKey, CachedSolidEntry>,
}

thread_local! {
    static SOLID_ELEMENT_CACHE: RefCell<SolidElementCache> = RefCell::new(SolidElementCache::default());
}

/// Resets per-frame solid element occurrence counters for `output_id` so unchanged shell chrome
/// rectangles keep stable [`Id`] and [`CommitCounter`] values across frames.
pub(crate) fn begin_solid_element_frame(output_id: u32) {
    SOLID_ELEMENT_CACHE.with(|cache| {
        let mut guard = cache.borrow_mut();
        guard.active_output_id = output_id;
        guard.epoch = guard.epoch.wrapping_add(1);
        guard.frame_counts.clear();
        let current_epoch = guard.epoch;
        if current_epoch.is_multiple_of(32) {
            guard
                .entries
                .retain(|_, entry| current_epoch.wrapping_sub(entry.last_epoch) <= 16);
        }
    });
}

pub(super) fn resolved_icon_to_element(
    icon: crate::icons::ResolvedIconElement,
) -> CompositorRenderElement {
    match icon {
        crate::icons::ResolvedIconElement::Memory(el) => CompositorRenderElement::Memory(*el),
        crate::icons::ResolvedIconElement::Texture(el) => CompositorRenderElement::Texture(el),
    }
}

/// Resize grab border thickness in logical pixels (`RUSTYOS-022`).
pub const RESIZE_GRAB_BORDER: f64 = 6.0;

/// `TopBar` height in logical pixels (`macOS` standard 24 px Menu Bar, `RUSTYOS-049`).
pub const TOP_BAR_HEIGHT: i32 = 24;

/// Vertical offset of `TopBar` pills/labels inside the bar (`RUSTYOS-036`/`049`).
pub const TOP_BAR_ITEM_Y: i32 = 2;

/// Height of `TopBar` pills/labels (`RUSTYOS-036`).
pub const TOP_BAR_ITEM_H: i32 = 20;

/// Unified font size in logical pixels for Menu Bar active app, menu items, and clock (`RUSTYOS-049`).
pub const TOP_BAR_FONT_SIZE: f32 = 13.0;

/// Server-Side Decoration (SSD) titlebar height in logical pixels (`RUSTYOS-019`).
pub const TITLEBAR_HEIGHT: i32 = 28;

/// Dock container height in logical pixels (`RUSTYOS-050`: 50 px icon + concentric padding & 4 px dot).
pub const DOCK_HEIGHT: i32 = 68;

/// Dock bottom margin from screen edge.
pub const DOCK_MARGIN_BOTTOM: i32 = 8;

/// Dock hit-slot width/height in logical pixels (`RUSTYOS-050`).
pub const DOCK_ITEM_SIZE: i32 = 52;

/// Horizontal gap between Dock icons in logical pixels (`RUSTYOS-050`).
pub const DOCK_ITEM_GAP: i32 = 6;

/// Dock inner horizontal/vertical padding in logical pixels (`RUSTYOS-050`).
pub const DOCK_PADDING: i32 = 8;

/// Dock corner radius in logical pixels (`RUSTYOS-050`: concentric outer curve matching macOS Dock).
pub const DOCK_RADIUS: i32 = 22;

/// Extra horizontal spacing allocated for the hairline divider before Downloads/Trash (`RUSTYOS-050`).
pub const DOCK_DIVIDER_GAP: i32 = 14;

/// Resting app icon edge length inside a Dock slot (`RUSTYOS-050`).
pub const DOCK_ICON_SIZE: i32 = 50;

/// Number of left-side pinned app slots before running windows (`RUSTYOS-050`).
pub const DOCK_PINNED_COUNT: usize = 6;

/// Number of right-side utility slots after the divider (`Downloads`, `Trash`) (`RUSTYOS-050`).
pub const DOCK_UTILITY_COUNT: usize = 2;

/// SSD window frame & glass popover corner radius in logical pixels (`DESIGN.md` §12.3).
pub const WINDOW_RADIUS: i32 = 20;

/// Liquid Glass context/dropdown menu corner radius in logical pixels (`DESIGN.md` §12.3).
pub(super) const MENU_POPOVER_RADIUS: i32 = 16;

/// `TopBar` Left Menu metrics (`macOS` Menu Bar layout with `Inter` 13 px) (`RUSTYOS-038`/`049`).
pub const TOP_BAR_LOGO_X: i32 = 12;
pub const TOP_BAR_LOGO_W: i32 = 20;
pub const TOP_BAR_APP_X: i32 = 36;

/// Workspace button width, stride, and default start offset when optional strip is enabled (`RUSTYOS-038`/`049`).
pub const WS_BTN_WIDTH: i32 = 18;
pub const WS_BTN_GAP: i32 = 3;
pub const WS_BTN_STRIDE: i32 = WS_BTN_WIDTH + WS_BTN_GAP;

pub(super) const fn premul(r: f32, g: f32, b: f32, a: f32) -> Color32F {
    Color32F::new(r * a, g * a, b * a, a)
}

// Design tokens from `DESIGN.md` (Version 2.0 "RustyOS Purple Glass")
// Smithay Color32F requires premultiplied RGBA components.
pub const COLOR_BACKGROUND: Color32F = Color32F::new(0.059, 0.039, 0.098, 1.0); // #0F0A19
pub const COLOR_SURFACE: Color32F = Color32F::new(0.086, 0.063, 0.137, 1.0); // #161023
pub const COLOR_SURFACE_ELEVATED: Color32F = premul(0.125, 0.094, 0.180, 0.96); // #20182E
pub const COLOR_SURFACE_OVERLAY: Color32F = premul(0.161, 0.125, 0.216, 0.98); // #292037

pub const COLOR_ACCENT: Color32F = Color32F::new(0.647, 0.431, 1.0, 1.0); // #A56EFF
pub const COLOR_ACCENT_HOVER: Color32F = Color32F::new(0.725, 0.541, 1.0, 1.0); // #B98AFF
pub const COLOR_ACCENT_MUTED: Color32F = Color32F::new(0.463, 0.314, 0.710, 1.0); // #7650B5

pub const COLOR_TEXT: Color32F = Color32F::new(0.957, 0.941, 0.980, 1.0); // #F4F0FA
pub const COLOR_MUTED: Color32F = Color32F::new(0.651, 0.608, 0.710, 1.0); // #A69BB5
pub const COLOR_SUBTLE: Color32F = Color32F::new(0.463, 0.420, 0.522, 1.0); // #766B85

pub const COLOR_DANGER: Color32F = Color32F::new(0.898, 0.420, 0.459, 1.0); // #E56B75
pub const COLOR_SUCCESS: Color32F = Color32F::new(0.514, 0.839, 0.627, 1.0); // #83D6A0
pub const COLOR_WARNING: Color32F = Color32F::new(0.910, 0.769, 0.467, 1.0); // #E8C477

pub const COLOR_BORDER: Color32F = premul(1.0, 1.0, 1.0, 0.12); // #FFFFFF12 border
pub const COLOR_BORDER_HIGHLIGHT: Color32F = premul(1.0, 1.0, 1.0, 0.22); // outer glass rim

/// Computes the usable work area rectangle for client windows, reserving space for `TopBar` and `Dock`.
#[must_use]
pub fn usable_work_area(screen_size: Size<i32, smithay::utils::Physical>) -> rusty_wm::Rect {
    let top_margin = TOP_BAR_HEIGHT;
    let bottom_margin = DOCK_HEIGHT + DOCK_MARGIN_BOTTOM * 2;
    let h = (screen_size.h - top_margin - bottom_margin).max(100);
    rusty_wm::Rect::new(0, top_margin, screen_size.w, h)
}

pub(crate) fn make_solid_element(
    geo: Rectangle<i32, smithay::utils::Physical>,
    color: Color32F,
) -> SolidColorRenderElement {
    let comps = color.components();
    let color_bits = [
        comps[0].to_bits(),
        comps[1].to_bits(),
        comps[2].to_bits(),
        comps[3].to_bits(),
    ];
    let (id, commit) = SOLID_ELEMENT_CACHE.with(|cache| {
        let mut guard = cache.borrow_mut();
        let rect_tuple = (geo.loc.x, geo.loc.y, geo.size.w, geo.size.h);
        let occ = {
            let count = guard.frame_counts.entry(rect_tuple).or_insert(0);
            let current = *count;
            *count = count.saturating_add(1);
            current
        };
        let key = SolidSlotKey {
            output_id: guard.active_output_id,
            x: geo.loc.x,
            y: geo.loc.y,
            w: geo.size.w,
            h: geo.size.h,
            occurrence: occ,
        };
        let epoch = guard.epoch;
        let entry = guard
            .entries
            .entry(key)
            .or_insert_with(|| CachedSolidEntry {
                id: Id::new(),
                commit: CommitCounter::default(),
                color_bits,
                last_epoch: epoch,
            });
        if entry.color_bits != color_bits {
            entry.commit.increment();
            entry.color_bits = color_bits;
        }
        entry.last_epoch = epoch;
        (entry.id.clone(), entry.commit)
    });
    SolidColorRenderElement::new(id, geo, commit, color, Kind::Unspecified)
}

/// Scales panel glass RGBA alpha by `glass_opacity` and `reduce_transparency` (`RUSTYOS-048`).
#[must_use]
pub(crate) fn scale_glass_rgba(
    base: [u8; 4],
    glass_opacity: f32,
    reduce_transparency: bool,
) -> [u8; 4] {
    if reduce_transparency {
        return [
            base[0].saturating_sub(4),
            base[1].saturating_sub(4),
            base[2].saturating_sub(2),
            base[3].max(246),
        ];
    }
    let clamped = crate::config::clamp_glass_opacity(glass_opacity);
    let scale = clamped / crate::config::DEFAULT_GLASS_OPACITY;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let alpha = (f32::from(base[3]) * scale).clamp(48.0, 252.0).round() as u8;
    [base[0], base[1], base[2], alpha]
}

#[allow(clippy::cast_possible_truncation)]
pub(super) fn anim_offset(progress: f64, max_offset: f64) -> i32 {
    ((1.0 - progress.clamp(0.0, 1.0)) * max_offset).round() as i32
}

pub(super) fn rect_contains(
    rect: Rectangle<i32, smithay::utils::Physical>,
    x: f64,
    y: f64,
) -> bool {
    let rx = f64::from(rect.loc.x);
    let ry = f64::from(rect.loc.y);
    let rw = f64::from(rect.size.w);
    let rh = f64::from(rect.size.h);
    x >= rx && x < rx + rw && y >= ry && y < ry + rh
}

pub(super) fn rounded_rect_contains(
    rect: Rectangle<i32, smithay::utils::Physical>,
    radius: i32,
    x: f64,
    y: f64,
) -> bool {
    if !rect_contains(rect, x, y) {
        return false;
    }
    let rx = f64::from(rect.loc.x);
    let ry = f64::from(rect.loc.y);
    let rw = f64::from(rect.size.w.max(1));
    let rh = f64::from(rect.size.h.max(1));
    let r = f64::from(radius).clamp(0.0, rw * 0.5).min(rh * 0.5);
    let cx = rx + rw * 0.5;
    let cy = ry + rh * 0.5;
    let qx = (x - cx).abs() - (rw * 0.5 - r);
    let qy = (y - cy).abs() - (rh * 0.5 - r);
    let dx = qx.max(0.0);
    let dy = qy.max(0.0);
    dx.hypot(dy) <= r + 0.5
}

pub(super) fn truncate_ascii(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        text.to_owned()
    } else {
        let mut out: String = text.chars().take(max_chars.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

pub(super) const RGBA_SURFACE: [u8; 4] = [22, 16, 35, 255]; // #161023
pub(super) const RGBA_SURFACE_ELEVATED: [u8; 4] = [32, 24, 46, 245]; // #20182E @ 0.96
pub(super) const RGBA_SURFACE_OVERLAY: [u8; 4] = [41, 32, 55, 250]; // #292037 @ 0.98
pub(super) const RGBA_ACCENT: [u8; 4] = [165, 110, 255, 255]; // #A56EFF
pub(super) const RGBA_ACCENT_MUTED: [u8; 4] = [118, 80, 181, 255]; // #7650B5
pub(super) const RGBA_BORDER: [u8; 4] = [255, 255, 255, 31];
pub(super) const RGBA_BORDER_HIGHLIGHT: [u8; 4] = [255, 255, 255, 56];
pub(super) const RGBA_DANGER: [u8; 4] = [229, 107, 117, 255];

pub(super) fn rgba_to_premul_color32f(rgba: [u8; 4]) -> Color32F {
    let a = f32::from(rgba[3]) / 255.0;
    let r = f32::from(rgba[0]) / 255.0;
    let g = f32::from(rgba[1]) / 255.0;
    let b = f32::from(rgba[2]) / 255.0;
    Color32F::new(r * a, g * a, b * a, a)
}

pub(super) fn blend_border_over_fill_color(border: [u8; 4], fill: [u8; 4]) -> Color32F {
    let fa = f32::from(border[3]) / 255.0;
    let ba = f32::from(fill[3]) / 255.0;
    let out_a = fa + ba * (1.0 - fa);
    let inv = 1.0 - fa;
    let r = (f32::from(border[0]) * fa + f32::from(fill[0]) * ba * inv) / 255.0;
    let g = (f32::from(border[1]) * fa + f32::from(fill[1]) * ba * inv) / 255.0;
    let b = (f32::from(border[2]) * fa + f32::from(fill[2]) * ba * inv) / 255.0;
    Color32F::new(r, g, b, out_a)
}

/// Pushes a zero-overdraw rounded glass box (4 SDF corners + 4 1px borders + 3 fill bands).
#[allow(clippy::too_many_arguments, clippy::many_single_char_names)]
pub(super) fn push_rounded_glass_box(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    rect: Rectangle<i32, smithay::utils::Physical>,
    radius: i32,
    fill_rgba: [u8; 4],
    border_rgba: Option<[u8; 4]>,
) {
    use crate::surfaces::CornerQuadrant;

    let x = rect.loc.x;
    let y = rect.loc.y;
    let w = rect.size.w;
    let h = rect.size.h;
    let r = radius.min(w / 2).min(h / 2).max(0);
    let fill_col = rgba_to_premul_color32f(fill_rgba);

    if r <= 1 {
        out.push(CompositorRenderElement::Solid(make_solid_element(
            rect, fill_col,
        )));
        return;
    }

    // 4 cached SDF rounded corners
    let corners = [
        (CornerQuadrant::TopLeft, Point::from((x, y))),
        (CornerQuadrant::TopRight, Point::from((x + w - r, y))),
        (CornerQuadrant::BottomLeft, Point::from((x, y + h - r))),
        (
            CornerQuadrant::BottomRight,
            Point::from((x + w - r, y + h - r)),
        ),
    ];
    for (quad, loc) in corners {
        if let Some(el) = shape_cache.corner_element(renderer, r, quad, fill_rgba, border_rgba, loc)
        {
            out.push(CompositorRenderElement::Memory(el));
        }
    }

    // Straight 1px border strips matching composite_over(border, fill)
    if let Some(b_rgba) = border_rgba {
        let rim_col = blend_border_over_fill_color(b_rgba, fill_rgba);
        if w > 2 * r {
            out.push(CompositorRenderElement::Solid(make_solid_element(
                Rectangle::new(Point::from((x + r, y)), Size::from((w - 2 * r, 1))),
                rim_col,
            )));
            out.push(CompositorRenderElement::Solid(make_solid_element(
                Rectangle::new(Point::from((x + r, y + h - 1)), Size::from((w - 2 * r, 1))),
                rim_col,
            )));
        }
        if h > 2 * r {
            out.push(CompositorRenderElement::Solid(make_solid_element(
                Rectangle::new(Point::from((x, y + r)), Size::from((1, h - 2 * r))),
                rim_col,
            )));
            out.push(CompositorRenderElement::Solid(make_solid_element(
                Rectangle::new(Point::from((x + w - 1, y + r)), Size::from((1, h - 2 * r))),
                rim_col,
            )));
        }
    }

    let b_inset = i32::from(border_rgba.is_some());
    if w > 2 * r && r > b_inset {
        out.push(CompositorRenderElement::Solid(make_solid_element(
            Rectangle::new(
                Point::from((x + r, y + b_inset)),
                Size::from((w - 2 * r, r - b_inset)),
            ),
            fill_col,
        )));
        out.push(CompositorRenderElement::Solid(make_solid_element(
            Rectangle::new(
                Point::from((x + r, y + h - r)),
                Size::from((w - 2 * r, r - b_inset)),
            ),
            fill_col,
        )));
    }
    if h > 2 * r && w > 2 * b_inset {
        out.push(CompositorRenderElement::Solid(make_solid_element(
            Rectangle::new(
                Point::from((x + b_inset, y + r)),
                Size::from((w - 2 * b_inset, h - 2 * r)),
            ),
            fill_col,
        )));
    }
}

/// Pushes a zero-overdraw soft drop shadow halo around `rect` (`RUSTYOS-022`).
#[allow(
    clippy::too_many_arguments,
    clippy::cast_precision_loss,
    clippy::many_single_char_names
)]
pub(super) fn push_soft_shadow(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    rect: Rectangle<i32, smithay::utils::Physical>,
    inner_radius: i32,
    spread: i32,
    max_alpha: u8,
) {
    use crate::surfaces::CornerQuadrant;

    let x = rect.loc.x;
    let y = rect.loc.y;
    let w = rect.size.w;
    let h = rect.size.h;
    let r = inner_radius.min(w / 2).min(h / 2).max(0);
    if spread <= 0 {
        return;
    }

    let shadow_rgb = [5, 2, 12];
    let corner_locs = [
        (
            CornerQuadrant::TopLeft,
            Point::from((x - spread, y - spread)),
        ),
        (
            CornerQuadrant::TopRight,
            Point::from((x + w - r, y - spread)),
        ),
        (
            CornerQuadrant::BottomLeft,
            Point::from((x - spread, y + h - r)),
        ),
        (
            CornerQuadrant::BottomRight,
            Point::from((x + w - r, y + h - r)),
        ),
    ];
    for (quad, loc) in corner_locs {
        if let Some(el) =
            shape_cache.shadow_corner_element(renderer, r, spread, quad, shadow_rgb, max_alpha, loc)
        {
            out.push(CompositorRenderElement::Memory(el));
        }
    }

    // Stepped non-overlapping perimeter gradient bands matching quadratic SDF falloff
    let band_count = 5;
    let step = (spread / band_count).max(1);
    let max_a = f32::from(max_alpha) / 255.0;
    for band in 0..band_count {
        let d0 = band * step;
        let d1 = if band + 1 == band_count {
            spread
        } else {
            ((band + 1) * step).min(spread)
        };
        let thick = d1 - d0;
        if thick <= 0 {
            continue;
        }
        let d_mid = (d0 + d1) as f32 * 0.5;
        let t = (d_mid / spread as f32).clamp(0.0, 1.0);
        let alpha = max_a * (1.0 - t) * (1.0 - t);
        let col = premul(5.0 / 255.0, 2.0 / 255.0, 12.0 / 255.0, alpha);

        if w > 2 * r {
            out.push(CompositorRenderElement::Solid(make_solid_element(
                Rectangle::new(Point::from((x + r, y - d1)), Size::from((w - 2 * r, thick))),
                col,
            )));
            out.push(CompositorRenderElement::Solid(make_solid_element(
                Rectangle::new(
                    Point::from((x + r, y + h + d0)),
                    Size::from((w - 2 * r, thick)),
                ),
                col,
            )));
        }
        if h > 2 * r {
            out.push(CompositorRenderElement::Solid(make_solid_element(
                Rectangle::new(Point::from((x - d1, y + r)), Size::from((thick, h - 2 * r))),
                col,
            )));
            out.push(CompositorRenderElement::Solid(make_solid_element(
                Rectangle::new(
                    Point::from((x + w + d0, y + r)),
                    Size::from((thick, h - 2 * r)),
                ),
                col,
            )));
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn push_liquid_glass_panel(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    rect: Rectangle<i32, smithay::utils::Physical>,
    radius: i32,
    style: crate::surfaces::GlassPanelStyle,
    glass_opacity: f32,
    reduce_transparency: bool,
    shadow: Option<(i32, u8)>,
) {
    if let Some(el) = shape_cache.glass_panel_element(
        renderer,
        style,
        rect.size.w,
        rect.size.h,
        radius,
        glass_opacity,
        reduce_transparency,
        Point::from((rect.loc.x, rect.loc.y)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    if let Some((spread, max_alpha)) = shadow {
        push_soft_shadow(out, shape_cache, renderer, rect, radius, spread, max_alpha);
    }
}

pub(super) fn title_hue_seed(title: &str, id: u64) -> u8 {
    let base = u8::try_from(id & 0xff).unwrap_or(0);
    title.bytes().fold(base, u8::wrapping_add) % 4
}
