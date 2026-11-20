//! `TopBar` right-side status cluster, system tray, updates badge, and control center hit-testing (`RUSTYOS-090`).

use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::Point;

use crate::shell_render::CompositorRenderElement;
use crate::shell_render::tokens::{TOP_BAR_HEIGHT, TOP_BAR_ITEM_H, TOP_BAR_ITEM_Y};

use super::panel::{
    TopBarLayoutOptions, TopBarRightLayout, TopBarTelemetry,
    compute_top_bar_left_layout_with_options, compute_top_bar_right_layout,
    compute_top_bar_right_layout_with_options,
};

/// Compact system tray item metadata rendered inside `TopBar` (`RUSTYOS-032`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TopBarTrayItem<'a> {
    /// Unique identifier (`<bus><path>`).
    pub key: &'a str,
    /// Short 1-2 char monogram (`"NM"`, `"TG"`, etc.).
    pub label: &'a str,
    /// Optional XDG / filesystem icon name (`RUSTYOS-033`).
    pub icon_name: &'a str,
    /// Optional decoded SNI ARGB32 icon pixmap (`RUSTYOS-033`).
    pub icon_pixmap: Option<&'a crate::icons::TrayPixmap>,
    /// Attention alert requested by applet (`NeedsAttention`).
    pub attention: bool,
    /// Passive/idle state (`Passive`).
    pub passive: bool,
}

/// Maximum number of system tray applets displayed simultaneously in `TopBar` (`RUSTYOS-032`).
pub const MAX_TOP_BAR_TRAY_ITEMS: usize = 6;

pub(crate) fn status_pill_rect(screen_width: i32) -> (i32, i32, i32, i32) {
    let layout = compute_top_bar_right_layout(screen_width, false, 0, "12:00");
    (
        layout.status_pill_x,
        TOP_BAR_ITEM_Y,
        layout.status_pill_w,
        TOP_BAR_ITEM_H,
    )
}

#[cfg(test)]
pub(crate) fn kbd_badge_rect(screen_width: i32, notification_count: u32) -> (i32, i32, i32, i32) {
    let layout = compute_top_bar_right_layout_with_options(
        screen_width,
        "12:00",
        TopBarLayoutOptions {
            notification_count,
            show_input_source: true,
            ..TopBarLayoutOptions::default()
        },
    );
    (
        layout.kbd_x.unwrap_or(layout.tray_anchor_x),
        TOP_BAR_ITEM_Y,
        layout.kbd_w,
        TOP_BAR_ITEM_H,
    )
}

/// Computes `TopBar` tray pill rectangles when tray visibility is enabled (`RUSTYOS-032`/`049`).
#[must_use]
pub fn tray_slot_rects(
    screen_width: i32,
    notification_count: u32,
    tray_count: usize,
) -> Vec<(i32, i32, i32, i32)> {
    tray_slot_rects_with_options(
        screen_width,
        "12:00",
        TopBarLayoutOptions {
            notification_count,
            show_tray: true,
            show_input_source: true,
            ..TopBarLayoutOptions::default()
        },
        tray_count,
    )
}

/// Computes `TopBar` tray pill rectangles using exact telemetry and visibility flags (`RUSTYOS-048`/`049`).
#[must_use]
pub fn tray_slot_rects_with_options(
    screen_width: i32,
    clock_text: &str,
    opts: TopBarLayoutOptions,
    tray_count: usize,
) -> Vec<(i32, i32, i32, i32)> {
    if tray_count == 0 || !opts.show_tray {
        return Vec::new();
    }
    let left = compute_top_bar_left_layout_with_options("RustyOS", opts.show_workspaces);
    let min_left_x = if opts.show_workspaces {
        left.layout_x + left.layout_w + 10
    } else {
        left.help_x + left.help_w + 12
    };
    let right = compute_top_bar_right_layout_with_options(screen_width, clock_text, opts);
    let anchor_x = right.tray_anchor_x;
    let avail = (anchor_x - 8 - min_left_x).max(0);
    let slot_w = 24;
    let gap = 6;
    let stride = slot_w + gap;
    let max_fit = usize::try_from((avail + gap) / stride)
        .unwrap_or(0)
        .min(MAX_TOP_BAR_TRAY_ITEMS);
    let count = tray_count.min(max_fit);
    if count == 0 {
        return Vec::new();
    }
    let count_i32 = i32::try_from(count).unwrap_or(0);
    let first_x = anchor_x - 8 - (count_i32 * stride - gap);
    (0..count)
        .map(|idx| {
            let idx_i32 = i32::try_from(idx).unwrap_or(0);
            (
                first_x + idx_i32 * stride,
                TOP_BAR_ITEM_Y,
                slot_w,
                TOP_BAR_ITEM_H,
            )
        })
        .collect()
}

/// Backwards-compatible wrapper for telemetry-driven tray rectangles (`RUSTYOS-048`).
#[must_use]
pub fn tray_slot_rects_with_layout(
    screen_width: i32,
    has_battery: bool,
    notification_count: u32,
    clock_text: &str,
    tray_count: usize,
) -> Vec<(i32, i32, i32, i32)> {
    tray_slot_rects_with_options(
        screen_width,
        clock_text,
        TopBarLayoutOptions {
            has_battery,
            notification_count,
            show_tray: true,
            ..TopBarLayoutOptions::default()
        },
        tray_count,
    )
}

/// Checks if `(x, y)` clicked the `TopBar` keyboard layout pill (`"EN"` / `"RU"`) when visible (`RUSTYOS-029`).
#[must_use]
pub fn check_top_bar_kbd_click(screen_width: i32, notification_count: u32, x: f64, y: f64) -> bool {
    check_top_bar_kbd_click_for_options(
        screen_width,
        "12:00",
        TopBarLayoutOptions {
            notification_count,
            show_input_source: true,
            ..TopBarLayoutOptions::default()
        },
        x,
        y,
    )
}

/// Option-aware hit-test for the `TopBar` keyboard layout indicator (`RUSTYOS-049`).
#[must_use]
pub fn check_top_bar_kbd_click_for_options(
    screen_width: i32,
    clock_text: &str,
    opts: TopBarLayoutOptions,
    x: f64,
    y: f64,
) -> bool {
    if !opts.show_input_source || y < 0.0 || y > f64::from(TOP_BAR_HEIGHT) {
        return false;
    }
    let layout = compute_top_bar_right_layout_with_options(screen_width, clock_text, opts);
    let Some(kbd_x) = layout.kbd_x else {
        return false;
    };
    x >= f64::from(kbd_x) && x < f64::from(kbd_x + layout.kbd_w)
}

/// Telemetry-aware hit-test for the `TopBar` keyboard layout indicator (`RUSTYOS-048`).
#[must_use]
pub fn check_top_bar_kbd_click_for_telemetry(
    screen_width: i32,
    has_battery: bool,
    notification_count: u32,
    clock_text: &str,
    x: f64,
    y: f64,
) -> bool {
    check_top_bar_kbd_click_for_options(
        screen_width,
        clock_text,
        TopBarLayoutOptions {
            has_battery,
            notification_count,
            show_input_source: true,
            ..TopBarLayoutOptions::default()
        },
        x,
        y,
    )
}

/// Checks if `(x, y)` clicked one of the `TopBar` system tray pills and returns its visible index (`RUSTYOS-032`).
#[must_use]
pub fn check_top_bar_tray_click(
    screen_width: i32,
    notification_count: u32,
    tray_count: usize,
    x: f64,
    y: f64,
) -> Option<usize> {
    check_top_bar_tray_click_for_options(
        screen_width,
        "12:00",
        TopBarLayoutOptions {
            notification_count,
            show_tray: true,
            show_input_source: true,
            ..TopBarLayoutOptions::default()
        },
        tray_count,
        x,
        y,
    )
}

/// Option-aware hit-test for `TopBar` system tray icons (`RUSTYOS-049`).
#[must_use]
pub fn check_top_bar_tray_click_for_options(
    screen_width: i32,
    clock_text: &str,
    opts: TopBarLayoutOptions,
    tray_count: usize,
    x: f64,
    y: f64,
) -> Option<usize> {
    if !opts.show_tray || y < 0.0 || y > f64::from(TOP_BAR_HEIGHT) {
        return None;
    }
    for (idx, (bx, _by, bw, _bh)) in
        tray_slot_rects_with_options(screen_width, clock_text, opts, tray_count)
            .into_iter()
            .enumerate()
    {
        if x >= f64::from(bx) && x < f64::from(bx + bw) {
            return Some(idx);
        }
    }
    None
}

/// Telemetry-aware hit-test for `TopBar` system tray icons (`RUSTYOS-048`).
#[must_use]
pub fn check_top_bar_tray_click_for_telemetry(
    screen_width: i32,
    has_battery: bool,
    notification_count: u32,
    clock_text: &str,
    tray_count: usize,
    x: f64,
    y: f64,
) -> Option<usize> {
    check_top_bar_tray_click_for_options(
        screen_width,
        clock_text,
        TopBarLayoutOptions {
            has_battery,
            notification_count,
            show_tray: true,
            ..TopBarLayoutOptions::default()
        },
        tray_count,
        x,
        y,
    )
}

/// Option-aware hit-test for the right `TopBar` Wi-Fi / Ethernet indicator (`RUSTYOS-108`).
#[must_use]
pub fn check_top_bar_wifi_click_for_options(
    screen_width: i32,
    clock_text: &str,
    opts: TopBarLayoutOptions,
    x: f64,
    y: f64,
) -> bool {
    if y < 0.0 || y > f64::from(TOP_BAR_HEIGHT) {
        return false;
    }
    let right = compute_top_bar_right_layout_with_options(screen_width, clock_text, opts);
    x >= f64::from(right.wifi_x - 3) && x < f64::from(right.wifi_x + 20 + 3)
}

/// Option-aware hit-test for the right `TopBar` Software Updates indicator (`RUSTYOS-076`).
#[must_use]
pub fn check_top_bar_updates_click_for_options(
    screen_width: i32,
    clock_text: &str,
    opts: TopBarLayoutOptions,
    x: f64,
    y: f64,
) -> bool {
    if !opts.show_updates || y < 0.0 || y > f64::from(TOP_BAR_HEIGHT) {
        return false;
    }
    let right = compute_top_bar_right_layout_with_options(screen_width, clock_text, opts);
    let Some(ux) = right.updates_x else {
        return false;
    };
    x >= f64::from(ux - 3) && x < f64::from(ux + right.updates_w + 3)
}

/// Option-aware hit-test for the right `TopBar` Spotlight Search loupe (`RUSTYOS-049`).
#[must_use]
pub fn check_top_bar_search_click_for_options(
    screen_width: i32,
    clock_text: &str,
    opts: TopBarLayoutOptions,
    x: f64,
    y: f64,
) -> bool {
    if y < 0.0 || y > f64::from(TOP_BAR_HEIGHT) {
        return false;
    }
    let right = compute_top_bar_right_layout_with_options(screen_width, clock_text, opts);
    x >= f64::from(right.search_x - 3) && x < f64::from(right.search_x + right.search_w + 3)
}

/// Checks if `(x, y)` clicked the right `TopBar` Spotlight Search loupe (`RUSTYOS-048`).
#[must_use]
pub fn check_top_bar_search_click_for_telemetry(
    screen_width: i32,
    has_battery: bool,
    notification_count: u32,
    clock_text: &str,
    x: f64,
    y: f64,
) -> bool {
    check_top_bar_search_click_for_options(
        screen_width,
        clock_text,
        TopBarLayoutOptions {
            has_battery,
            notification_count,
            ..TopBarLayoutOptions::default()
        },
        x,
        y,
    )
}

/// Checks if `(x, y)` clicked the right `TopBar` Control Center / status cluster.
#[must_use]
pub fn check_top_bar_status_click(screen_width: i32, x: f64, y: f64) -> bool {
    check_top_bar_status_click_for_telemetry(screen_width, false, 0, "12:00", x, y)
}

/// Option-aware hit-test for the `TopBar` status / Control Center cluster (`RUSTYOS-049`).
#[must_use]
pub fn check_top_bar_status_click_for_options(
    screen_width: i32,
    clock_text: &str,
    opts: TopBarLayoutOptions,
    x: f64,
    y: f64,
) -> bool {
    if y < 0.0 || y > f64::from(TOP_BAR_HEIGHT) {
        return false;
    }
    let right = compute_top_bar_right_layout_with_options(screen_width, clock_text, opts);
    x >= f64::from(right.status_hit_x) && x < f64::from(right.status_hit_x + right.status_hit_w)
}

/// Telemetry-aware hit-test for the `TopBar` status / Control Center cluster (`RUSTYOS-048`).
#[must_use]
pub fn check_top_bar_status_click_for_telemetry(
    screen_width: i32,
    has_battery: bool,
    notification_count: u32,
    clock_text: &str,
    x: f64,
    y: f64,
) -> bool {
    check_top_bar_status_click_for_options(
        screen_width,
        clock_text,
        TopBarLayoutOptions {
            has_battery,
            notification_count,
            ..TopBarLayoutOptions::default()
        },
        x,
        y,
    )
}

/// Option-aware hit-test for the `TopBar` Clock / date label toggling Notification Center (`RUSTYOS-109`).
#[must_use]
pub fn check_top_bar_clock_click_for_options(
    screen_width: i32,
    clock_text: &str,
    opts: TopBarLayoutOptions,
    x: f64,
    y: f64,
) -> bool {
    if y < 0.0 || y > f64::from(TOP_BAR_HEIGHT) {
        return false;
    }
    let right = compute_top_bar_right_layout_with_options(screen_width, clock_text, opts);
    x >= f64::from(right.clock_x - 2) && x < f64::from(right.clock_x + right.clock_w + 8)
}

fn push_top_bar_network_glyph(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    right: &TopBarRightLayout,
    t: &TopBarTelemetry<'_>,
    ink: [u8; 4],
    light_opaque: bool,
) {
    use crate::surfaces::UiGlyphKind;

    if t.wired_connected && !t.wifi_connected {
        if let Some(el) = shape_cache.ui_glyph_element(
            renderer,
            UiGlyphKind::Ethernet,
            20,
            ink,
            Point::from((right.wifi_x, TOP_BAR_ITEM_Y)),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
        return;
    }

    let wifi_tint = if t.wifi_connected || t.wifi_enabled {
        ink
    } else if light_opaque {
        [28, 29, 34, 130]
    } else {
        [255, 255, 255, 148]
    };
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        UiGlyphKind::Wifi {
            connected: t.wifi_connected,
            signal_percent: t.wifi_signal_percent,
        },
        20,
        wifi_tint,
        Point::from((right.wifi_x, TOP_BAR_ITEM_Y)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
}

/// Draws monochrome macOS Menu Bar glyphs (`[Volume] · [Battery] · Wi-Fi · Search · Control Center`) (`RUSTYOS-036`, `RUSTYOS-049`).
pub(crate) fn push_top_bar_right_glyphs(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    right: &TopBarRightLayout,
    t: &TopBarTelemetry<'_>,
) {
    use crate::surfaces::UiGlyphKind;

    let light_opaque =
        t.reduce_transparency && !crate::shell_render::tokens::ThemePalette::active().is_dark;
    let white = if light_opaque {
        [28, 29, 34, 255]
    } else {
        [255, 255, 255, 255]
    };

    if let Some(ux) = right.updates_x
        && let Some(el) = shape_cache.ui_glyph_element(
            renderer,
            UiGlyphKind::Updates {
                available: t.update_count > 0 || t.rustyos_update_available,
            },
            18,
            white,
            Point::from((ux + 2, TOP_BAR_ITEM_Y + 1)),
        )
    {
        out.push(CompositorRenderElement::Memory(el));
    }
    if let Some(vx) = right.volume_x
        && let Some(el) = shape_cache.ui_glyph_element(
            renderer,
            UiGlyphKind::Volume {
                level_percent: t.volume_percent,
                muted: t.audio_muted,
            },
            18,
            white,
            Point::from((vx, TOP_BAR_ITEM_Y + 1)),
        )
    {
        out.push(CompositorRenderElement::Memory(el));
    }
    if let Some(bx) = right.battery_x
        && let Some(el) = shape_cache.ui_glyph_rect_element(
            renderer,
            UiGlyphKind::Battery {
                percent: t.battery_percent,
                charging: t.ac_online || t.battery_charging,
            },
            26,
            20,
            white,
            Point::from((bx, TOP_BAR_ITEM_Y)),
        )
    {
        out.push(CompositorRenderElement::Memory(el));
    }
    push_top_bar_network_glyph(out, shape_cache, renderer, right, t, white, light_opaque);
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        UiGlyphKind::Search,
        18,
        white,
        Point::from((right.search_x, TOP_BAR_ITEM_Y + 1)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    let cc_color = if t.quick_settings_open {
        [255, 255, 255, 255]
    } else {
        white
    };
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        UiGlyphKind::ControlCenter,
        20,
        cc_color,
        Point::from((right.control_center_x, TOP_BAR_ITEM_Y)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
}
