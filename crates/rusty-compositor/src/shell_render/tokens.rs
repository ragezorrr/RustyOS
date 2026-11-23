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
    static ACTIVE_APPEARANCE: std::cell::Cell<crate::config::AppearanceMode> =
        const { std::cell::Cell::new(crate::config::AppearanceMode::Light) };
    static ACTIVE_ACCENT_INDEX: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

/// Sets the active [`crate::config::AppearanceMode`] for the current render frame (`RUSTYOS-067`).
pub(crate) fn set_active_appearance(mode: crate::config::AppearanceMode) {
    ACTIVE_APPEARANCE.with(|cell| cell.set(mode));
}

/// Returns the active [`crate::config::AppearanceMode`] for the current render frame (`RUSTYOS-067`).
#[must_use]
pub(crate) fn active_appearance() -> crate::config::AppearanceMode {
    ACTIVE_APPEARANCE.with(std::cell::Cell::get)
}

/// Sets the active `[ui].accent_index` (`0..=6`) for the current render frame (`RUSTYOS-114`).
pub(crate) fn set_active_accent_index(index: u8) {
    let clamped = rusty_services::file_manager::clamp_accent_index(usize::from(index));
    ACTIVE_ACCENT_INDEX.with(|cell| cell.set(u8::try_from(clamped).unwrap_or(0)));
}

/// Returns the active `[ui].accent_index` (`0..=6`) for the current render frame (`RUSTYOS-114`).
#[must_use]
pub(crate) fn active_accent_index() -> u8 {
    ACTIVE_ACCENT_INDEX.with(std::cell::Cell::get)
}

/// Dynamic RustyOS Light/Dark color palette (`RUSTYOS-067`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct ThemePalette {
    pub is_dark: bool,
    pub popover_text_primary: [u8; 4],
    pub popover_text_secondary: [u8; 4],
    pub popover_text_muted: [u8; 4],
    pub popover_text_disabled: [u8; 4],
    pub popover_icon: [u8; 4],
    pub popover_danger: [u8; 4],
    pub popover_separator: Color32F,
    pub accent_rgba: [u8; 4],
    pub accent_hover_fill: [u8; 4],
    pub accent_hover_border: [u8; 4],
    pub control_pill_off_rgba: [u8; 4],
    pub border_rgba: [u8; 4],
    pub border_highlight_rgba: [u8; 4],
}

impl ThemePalette {
    /// Returns the RustyOS [`ThemePalette`] for the currently active [`crate::config::AppearanceMode`].
    #[must_use]
    pub(crate) const fn current_for(mode: crate::config::AppearanceMode) -> Self {
        match mode {
            crate::config::AppearanceMode::Light => Self {
                is_dark: false,
                popover_text_primary: [29, 31, 36, 255],
                popover_text_secondary: [72, 76, 86, 255],
                popover_text_muted: [130, 136, 148, 255],
                popover_text_disabled: [72, 78, 90, 155],
                popover_icon: [44, 46, 53, 245],
                popover_danger: [215, 48, 62, 255],
                popover_separator: premul(0.0, 0.0, 0.0, 0.055),
                accent_rgba: [0, 122, 255, 255],
                accent_hover_fill: [0, 122, 255, 238],
                accent_hover_border: [56, 152, 255, 160],
                control_pill_off_rgba: [220, 224, 232, 224],
                border_rgba: [0, 0, 0, 20],
                border_highlight_rgba: [255, 255, 255, 96],
            },
            crate::config::AppearanceMode::Dark => Self {
                is_dark: true,
                popover_text_primary: [242, 242, 247, 255],
                popover_text_secondary: [198, 198, 206, 255],
                popover_text_muted: [148, 148, 156, 255],
                popover_text_disabled: [154, 158, 168, 145],
                popover_icon: [229, 229, 234, 245],
                popover_danger: [255, 105, 97, 255],
                popover_separator: premul(1.0, 1.0, 1.0, 0.07),
                accent_rgba: [10, 132, 255, 255],
                accent_hover_fill: [10, 132, 255, 234],
                accent_hover_border: [74, 164, 255, 145],
                control_pill_off_rgba: [56, 56, 62, 220],
                border_rgba: [255, 255, 255, 18],
                border_highlight_rgba: [255, 255, 255, 26],
            },
        }
    }

    /// Returns the RustyOS [`ThemePalette`] for `mode` and explicit `[ui].accent_index` (`RUSTYOS-114`).
    #[must_use]
    pub(crate) const fn current_for_styled(
        mode: crate::config::AppearanceMode,
        accent_index: u8,
    ) -> Self {
        let mut base = Self::current_for(mode);
        let accent = rusty_services::file_manager::accent_rgba_for_index(
            accent_index as usize,
            base.is_dark,
        );
        let border_r = accent[0] / 2 + 127;
        let border_g = accent[1] / 2 + 127;
        let border_b = accent[2] / 2 + 127;
        base.accent_rgba = accent;
        base.accent_hover_fill = [
            accent[0],
            accent[1],
            accent[2],
            if base.is_dark { 234 } else { 238 },
        ];
        if accent_index != 0 {
            base.accent_hover_border = [
                border_r,
                border_g,
                border_b,
                if base.is_dark { 145 } else { 160 },
            ];
        }
        base
    }

    /// Returns the active [`ThemePalette`] for the current thread's [`active_appearance`] and [`active_accent_index`].
    #[must_use]
    pub(crate) fn active() -> Self {
        Self::current_for_styled(active_appearance(), active_accent_index())
    }
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

/// `TopBar` height in logical pixels (`macOS` Menu Bar, `RUSTYOS-049`).
pub const TOP_BAR_HEIGHT: i32 = 30;

/// Vertical offset of `TopBar` pills/labels inside the bar (`RUSTYOS-036`/`049`).
pub const TOP_BAR_ITEM_Y: i32 = 4;

/// Height of `TopBar` pills/labels (`RUSTYOS-036`).
pub const TOP_BAR_ITEM_H: i32 = 22;

/// Unified font size in logical pixels for Menu Bar active app, menu items, and clock (`RUSTYOS-049`).
pub const TOP_BAR_FONT_SIZE: f32 = 14.0;

/// Server-Side Decoration (SSD) titlebar height in logical pixels (`RUSTYOS-019`).
pub const TITLEBAR_HEIGHT: i32 = 28;

/// Dock container height in logical pixels (`RUSTYOS-050`: 50 px icon + concentric padding & 4 px dot).
pub const DOCK_HEIGHT: i32 = 68;

/// Dock bottom margin from screen edge.
pub const DOCK_MARGIN_BOTTOM: i32 = 4;

/// Dock hit-slot width/height in logical pixels (`RUSTYOS-050`).
pub const DOCK_ITEM_SIZE: i32 = 52;

/// Horizontal gap between Dock icons in logical pixels (`RUSTYOS-050`).
pub const DOCK_ITEM_GAP: i32 = 6;

/// Dock inner horizontal/vertical padding in logical pixels (`RUSTYOS-050`).
pub const DOCK_PADDING: i32 = 8;

/// Dock corner radius in logical pixels (`RUSTYOS-094`: flat frosted glass container matching reference, radius 18px).
pub const DOCK_RADIUS: i32 = 18;

/// Computes dynamic Dock corner radius calibrated to `RustyOS` standard (`thickness * 28 / 86`) (`RUSTYOS-098`).
#[must_use]
pub fn compute_dock_radius(thickness: i32) -> i32 {
    let raw = (thickness * 28 + 43) / 86;
    raw.clamp(12, (thickness / 2).max(12))
}

/// Extra horizontal spacing allocated for the hairline divider before Downloads/Trash (`RUSTYOS-050`).
pub const DOCK_DIVIDER_GAP: i32 = 14;

/// Resting app icon edge length inside a Dock slot (`RUSTYOS-050`).
pub const DOCK_ICON_SIZE: i32 = 50;

/// Number of left-side pinned app slots before running windows (`RUSTYOS-050`).
pub const DOCK_PINNED_COUNT: usize = 6;

/// Number of right-side utility slots after the divider (`Downloads`, `Trash`) (`RUSTYOS-050`).
pub const DOCK_UTILITY_COUNT: usize = 2;

/// SSD window frame & glass popover corner radius in logical pixels (`DESIGN.md` §12.3).
#[allow(clippy::cast_possible_wrap)]
pub const WINDOW_RADIUS: i32 = rusty_ui::WINDOW_RADIUS as i32;

/// Liquid Glass context/dropdown menu corner radius in logical pixels (`DESIGN.md` §12.3, ADR-0058, ADR-0069).
pub(super) const MENU_POPOVER_RADIUS: i32 = rusty_ui::menu::MENU_RADIUS;

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

// Design tokens from `DESIGN.md` (Version 3.0 "RustyOS Light & Dark")
// Smithay Color32F requires premultiplied RGBA components.
pub const COLOR_BACKGROUND: Color32F = Color32F::new(0.078, 0.082, 0.094, 1.0); // #141518
pub const COLOR_SURFACE: Color32F = Color32F::new(0.118, 0.118, 0.129, 1.0); // #1E1E21
pub const COLOR_SURFACE_ELEVATED: Color32F = premul(0.149, 0.149, 0.165, 0.96); // #26262A
pub const COLOR_SURFACE_OVERLAY: Color32F = premul(0.188, 0.188, 0.212, 0.98); // #303036

pub const COLOR_ACCENT: Color32F = Color32F::new(0.039, 0.518, 1.0, 1.0); // #0A84FF
pub const COLOR_ACCENT_HOVER: Color32F = Color32F::new(0.251, 0.612, 1.0, 1.0); // #409CFF
pub const COLOR_ACCENT_MUTED: Color32F = Color32F::new(0.0, 0.384, 0.800, 1.0); // #0062CC

pub const COLOR_TEXT: Color32F = Color32F::new(0.949, 0.949, 0.969, 1.0); // #F2F2F7
pub const COLOR_MUTED: Color32F = Color32F::new(0.580, 0.580, 0.612, 1.0); // #94949C
pub const COLOR_SUBTLE: Color32F = Color32F::new(0.431, 0.431, 0.463, 1.0); // #6E6E76

pub const COLOR_DANGER: Color32F = Color32F::new(1.0, 0.271, 0.227, 1.0); // #FF453A
pub const COLOR_SUCCESS: Color32F = Color32F::new(0.188, 0.820, 0.345, 1.0); // #30D158
pub const COLOR_WARNING: Color32F = Color32F::new(1.0, 0.839, 0.039, 1.0); // #FFD60A

pub const COLOR_BORDER: Color32F = premul(1.0, 1.0, 1.0, 0.12); // #FFFFFF12 border
pub const COLOR_BORDER_HIGHLIGHT: Color32F = premul(1.0, 1.0, 1.0, 0.22); // outer glass rim

/// Computes the usable work area rectangle reserving `TopBar` and the active floating `Dock`
/// (`Bottom`, `Left`, or `Right`, or zero Dock reservation when `auto_hide` is enabled) (`RUSTYOS-073`).
#[must_use]
pub fn usable_work_area_with_dock(
    screen_size: Size<i32, smithay::utils::Physical>,
    position: crate::config::DockPosition,
    icon_size: i32,
    auto_hide: bool,
) -> rusty_wm::Rect {
    let top_margin = TOP_BAR_HEIGHT;
    if auto_hide {
        let h = (screen_size.h - top_margin).max(100);
        return rusty_wm::Rect::new(0, top_margin, screen_size.w.max(100), h);
    }
    let clamped_icon = crate::config::clamp_dock_icon_size(icon_size);
    let dock_thickness = clamped_icon + DOCK_PADDING * 2 + 2;
    let dock_reserve = dock_thickness + DOCK_MARGIN_BOTTOM * 2;
    match position {
        crate::config::DockPosition::Bottom => {
            let h = (screen_size.h - top_margin - dock_reserve).max(100);
            rusty_wm::Rect::new(0, top_margin, screen_size.w.max(100), h)
        }
        crate::config::DockPosition::Left => {
            let w = (screen_size.w - dock_reserve).max(100);
            let h = (screen_size.h - top_margin).max(100);
            rusty_wm::Rect::new(dock_reserve, top_margin, w, h)
        }
        crate::config::DockPosition::Right => {
            let w = (screen_size.w - dock_reserve).max(100);
            let h = (screen_size.h - top_margin).max(100);
            rusty_wm::Rect::new(0, top_margin, w, h)
        }
    }
}

/// Computes the usable work area rectangle for client windows using default bottom Dock metrics.
#[must_use]
pub fn usable_work_area(screen_size: Size<i32, smithay::utils::Physical>) -> rusty_wm::Rect {
    usable_work_area_with_dock(
        screen_size,
        crate::config::DockPosition::Bottom,
        crate::config::DEFAULT_DOCK_ICON_SIZE,
        false,
    )
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

pub(super) const RGBA_SURFACE_ELEVATED: [u8; 4] = [28, 28, 30, 246]; // #1C1C1E @ 0.96
pub(super) const RGBA_SURFACE_OVERLAY: [u8; 4] = [34, 34, 36, 250]; // #222224 @ 0.98
pub(super) const RGBA_ACCENT: [u8; 4] = [10, 132, 255, 255]; // #0A84FF
pub(super) const RGBA_BORDER: [u8; 4] = [255, 255, 255, 18];
pub(super) const RGBA_BORDER_HIGHLIGHT: [u8; 4] = [255, 255, 255, 28];
pub(super) const RGBA_DANGER: [u8; 4] = [255, 69, 58, 255];

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

/// CPU 9-slice soft drop shadow fallback when GPU shadow shader resources are unavailable.
#[allow(
    clippy::too_many_arguments,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::many_single_char_names
)]
pub(super) fn push_cpu_nine_slice_shadow(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    rect: Rectangle<i32, smithay::utils::Physical>,
    inner_radius: i32,
    elevation: rusty_ui::shadow::ShadowElevation,
) {
    use crate::surfaces::{CornerQuadrant, ShadowEdgeSide};

    let x = rect.loc.x;
    let y = rect.loc.y;
    let w = rect.size.w;
    let h = rect.size.h;
    let r = inner_radius.min(w / 2).min(h / 2).max(0);
    let style = rusty_ui::shadow::ShadowStyle::for_elevation(elevation);
    let spread = style.lobes[0].sigma.ceil() as i32;
    let max_alpha = (style.lobes[0].opacity * 255.0).clamp(0.0, 255.0) as u8;
    if spread <= 0 || w <= 0 || h <= 0 || max_alpha == 0 {
        return;
    }

    let shadow_rgb = [6, 8, 14];
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

    let edge_strips = [
        (
            ShadowEdgeSide::Top,
            Rectangle::new(
                Point::from((x + r, y - spread)),
                Size::from((w - 2 * r, spread)),
            ),
        ),
        (
            ShadowEdgeSide::Bottom,
            Rectangle::new(Point::from((x + r, y + h)), Size::from((w - 2 * r, spread))),
        ),
        (
            ShadowEdgeSide::Left,
            Rectangle::new(
                Point::from((x - spread, y + r)),
                Size::from((spread, h - 2 * r)),
            ),
        ),
        (
            ShadowEdgeSide::Right,
            Rectangle::new(Point::from((x + w, y + r)), Size::from((spread, h - 2 * r))),
        ),
    ];
    for (side, dst) in edge_strips {
        if let Some(el) =
            shape_cache.shadow_edge_element(renderer, spread, side, shadow_rgb, max_alpha, dst)
        {
            out.push(CompositorRenderElement::Memory(el));
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn push_liquid_glass_panel(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    rect: Rectangle<i32, smithay::utils::Physical>,
    radius: i32,
    style: crate::surfaces::GlassPanelStyle,
    glass_opacity: f32,
    reduce_transparency: bool,
) {
    if let Some(el) = shape_cache.glass_panel_render_element(
        renderer,
        style,
        rect.size.w,
        rect.size.h,
        radius,
        glass_opacity,
        reduce_transparency,
        Point::from((rect.loc.x, rect.loc.y)),
    ) {
        out.push(el);
    }
}

/// Pushes a standardized 3-lobe elevation drop shadow (`RUSTYOS-126`, `RUSTYOS-128`).
#[allow(clippy::many_single_char_names)]
pub(crate) fn push_elevation_shadow(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    rect: Rectangle<i32, smithay::utils::Physical>,
    inner_radius: i32,
    elevation: rusty_ui::shadow::ShadowElevation,
) {
    if elevation == rusty_ui::shadow::ShadowElevation::None {
        return;
    }
    let x = rect.loc.x;
    let y = rect.loc.y;
    let w = rect.size.w;
    let h = rect.size.h;
    let r = inner_radius.min(w / 2).min(h / 2).max(0);
    if w <= 0 || h <= 0 {
        return;
    }

    let shadow_rgb = [6, 8, 14];
    let logical_rect = Rectangle::new(Point::from((x, y)), Size::from((w, h)));
    if let Some(el) = shape_cache.elevation_shadow_render_element(
        renderer,
        logical_rect,
        r,
        elevation,
        shadow_rgb,
    ) {
        out.push(el);
        return;
    }

    push_cpu_nine_slice_shadow(out, shape_cache, renderer, rect, r, elevation);
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn push_liquid_glass_panel_elevation(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    rect: Rectangle<i32, smithay::utils::Physical>,
    radius: i32,
    style: crate::surfaces::GlassPanelStyle,
    glass_opacity: f32,
    reduce_transparency: bool,
    elevation: rusty_ui::shadow::ShadowElevation,
) {
    if let Some(el) = shape_cache.glass_panel_render_element(
        renderer,
        style,
        rect.size.w,
        rect.size.h,
        radius,
        glass_opacity,
        reduce_transparency,
        Point::from((rect.loc.x, rect.loc.y)),
    ) {
        out.push(el);
    }
    if elevation != rusty_ui::shadow::ShadowElevation::None {
        push_elevation_shadow(out, shape_cache, renderer, rect, radius, elevation);
    }
}

pub(super) fn title_hue_seed(title: &str, id: u64) -> u8 {
    let base = u8::try_from(id & 0xff).unwrap_or(0);
    title.bytes().fold(base, u8::wrapping_add) % 4
}
