//! `TopBar` Wi-Fi & Ethernet `Liquid Glass` popover layout, hit-testing, and rendering (`RUSTYOS-108`).

use rusty_services::{NetworkStatus, WifiAccessPoint};
use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::{Physical, Point, Rectangle, Size};

use crate::surfaces::{GlassPanelStyle, UiGlyphKind};

use super::CompositorRenderElement;
use super::tokens::{
    MENU_POPOVER_RADIUS, TOP_BAR_HEIGHT, ThemePalette, make_solid_element,
    push_liquid_glass_panel_elevation, push_rounded_glass_box, truncate_ascii,
};

const WIFI_POPOVER_WIDTH: i32 = 280;
const HOVER_PILL_INSET_H: i32 = 5;
const HOVER_PILL_INSET_V: i32 = 1;
const HOVER_PILL_RADIUS: i32 = rusty_ui::menu::HOVER_PILL_RADIUS;
const MAX_KNOWN_ROWS: usize = 4;
const MAX_OTHER_ROWS: usize = 6;

/// Interactive hit-test regions inside [`ShellOverlayView::Wifi`] (`RUSTYOS-108`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WifiPopoverHit {
    /// Toggle the Wi-Fi radio switch in the popover header.
    ToggleWifi,
    /// Connect to `network_status.wifi_networks[idx]`.
    SelectNetwork(usize),
    /// Launch / focus `rusty-settings` on the `Wi-Fi` pane.
    OpenWifiSettings,
    /// Click fell inside the popover panel background.
    InsidePopover,
    /// Click fell outside the popover (dismisses popover).
    OutsidePopover,
}

/// Live view state driving [`ShellOverlayView::Wifi`] (`RUSTYOS-108`).
#[derive(Debug, Clone, Copy)]
pub struct WifiPopoverRenderState<'a> {
    /// Global glass opacity factor (`0.35..=1.0`).
    pub glass_opacity: f32,
    /// High-contrast opaque shell mode.
    pub reduce_transparency: bool,
    /// Live separated Ethernet & Wi-Fi status snapshot.
    pub network: &'a NetworkStatus,
    /// Right anchor X coordinate in `TopBar` coordinates near the network glyph.
    pub anchor_right_x: i32,
    /// Currently hovered interactive element inside the popover.
    pub hovered_hit: Option<WifiPopoverHit>,
}

/// Precomputed row rectangle bound to an index in `NetworkStatus::wifi_networks` (`RUSTYOS-108`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WifiPopoverRowSlot {
    /// Index into [`NetworkStatus::wifi_networks`].
    pub network_index: usize,
    /// Full interactive row rectangle.
    pub rect: Rectangle<i32, Physical>,
    /// Whether this row belongs to the `Known Network(s)` group (`28 px` tall with circular badge).
    pub is_known: bool,
}

/// Precomputed physical coordinates for the Wi-Fi & Ethernet `TopBar` popover (`RUSTYOS-108`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WifiPopoverLayout {
    /// Outer popover card frame (`r = 13`).
    pub panel_rect: Rectangle<i32, Physical>,
    /// Interactive Wi-Fi header pill toggle switch (`38x20`).
    pub switch_rect: Rectangle<i32, Physical>,
    /// Informational wired Ethernet status row when a cable is active.
    pub wired_rect: Option<Rectangle<i32, Physical>>,
    /// `"Known Network"` / `"Known Networks"` section header rect.
    pub known_header_rect: Option<Rectangle<i32, Physical>>,
    /// Discovered known/connected Wi-Fi rows (`<= MAX_KNOWN_ROWS`).
    pub known_rows: Vec<WifiPopoverRowSlot>,
    /// `"Other Networks"` section header rect.
    pub other_header_rect: Option<Rectangle<i32, Physical>>,
    /// Discovered unknown/public Wi-Fi rows (`<= MAX_OTHER_ROWS`).
    pub other_rows: Vec<WifiPopoverRowSlot>,
    /// Fallback empty row when Wi-Fi is on but no APs are visible.
    pub empty_rect: Option<Rectangle<i32, Physical>>,
    /// Footer `"Wi-Fi Settings..."` action row (`24 px`).
    pub settings_rect: Rectangle<i32, Physical>,
    /// Y coordinates of horizontal `1 px` section dividers.
    pub separators: Vec<i32>,
}

fn point_in_rect(rect: Rectangle<i32, Physical>, x: f64, y: f64) -> bool {
    x >= f64::from(rect.loc.x)
        && x < f64::from(rect.loc.x + rect.size.w)
        && y >= f64::from(rect.loc.y)
        && y < f64::from(rect.loc.y + rect.size.h)
}

fn split_known_other(networks: &[WifiAccessPoint]) -> (Vec<usize>, Vec<usize>) {
    let mut known = Vec::new();
    let mut other = Vec::new();
    for (idx, ap) in networks.iter().enumerate() {
        if ap.connected || ap.known {
            if known.len() < MAX_KNOWN_ROWS {
                known.push(idx);
            }
        } else if other.len() < MAX_OTHER_ROWS {
            other.push(idx);
        }
    }
    (known, other)
}

/// Computes `Golden Gate` `Liquid Glass` popover geometry below the `TopBar` Wi-Fi/Ethernet glyph (`RUSTYOS-108`).
#[must_use]
pub fn compute_wifi_popover_layout(
    screen_size: Size<i32, Physical>,
    state: &WifiPopoverRenderState<'_>,
) -> WifiPopoverLayout {
    let w = WIFI_POPOVER_WIDTH.min((screen_size.w - 24).max(240));
    let px = (state.anchor_right_x - w + 10).clamp(12, (screen_size.w - w - 12).max(12));
    let py = TOP_BAR_HEIGHT + 8;

    let switch_rect = Rectangle::new(
        Point::from((px + w - 14 - 38, py + 10)),
        Size::from((38, 20)),
    );
    let mut cur_y = py + 36;
    let mut separators = Vec::new();

    let wired_rect = if state.network.wired_connected {
        separators.push(cur_y + 4);
        cur_y += 9;
        let rect = Rectangle::new(Point::from((px, cur_y)), Size::from((w, 26)));
        cur_y += 26;
        Some(rect)
    } else {
        None
    };

    let mut known_header_rect = None;
    let mut known_rows = Vec::new();
    let mut other_header_rect = None;
    let mut other_rows = Vec::new();
    let mut empty_rect = None;

    if state.network.wifi_enabled {
        let (known_idxs, other_idxs) = split_known_other(&state.network.wifi_networks);
        separators.push(cur_y + 4);
        cur_y += 9;

        if !known_idxs.is_empty() {
            known_header_rect = Some(Rectangle::new(
                Point::from((px + 14, cur_y + 2)),
                Size::from((w - 28, 18)),
            ));
            cur_y += 20;
            for idx in known_idxs {
                known_rows.push(WifiPopoverRowSlot {
                    network_index: idx,
                    rect: Rectangle::new(Point::from((px, cur_y)), Size::from((w, 28))),
                    is_known: true,
                });
                cur_y += 28;
            }
        }

        if !other_idxs.is_empty() {
            if !known_rows.is_empty() {
                separators.push(cur_y + 4);
                cur_y += 9;
            }
            other_header_rect = Some(Rectangle::new(
                Point::from((px + 14, cur_y + 2)),
                Size::from((w - 28, 18)),
            ));
            cur_y += 20;
            for idx in other_idxs {
                other_rows.push(WifiPopoverRowSlot {
                    network_index: idx,
                    rect: Rectangle::new(Point::from((px, cur_y)), Size::from((w, 24))),
                    is_known: false,
                });
                cur_y += 24;
            }
        }

        if known_rows.is_empty() && other_rows.is_empty() {
            empty_rect = Some(Rectangle::new(
                Point::from((px, cur_y)),
                Size::from((w, 26)),
            ));
            cur_y += 26;
        }
    }

    separators.push(cur_y + 4);
    cur_y += 9;
    let settings_rect = Rectangle::new(Point::from((px, cur_y)), Size::from((w, 24)));
    cur_y += 24 + 5;

    WifiPopoverLayout {
        panel_rect: Rectangle::new(Point::from((px, py)), Size::from((w, cur_y - py))),
        switch_rect,
        wired_rect,
        known_header_rect,
        known_rows,
        other_header_rect,
        other_rows,
        empty_rect,
        settings_rect,
        separators,
    }
}

/// Hit-tests pointer coordinates against [`ShellOverlayView::Wifi`] (`RUSTYOS-108`).
#[must_use]
pub fn hit_test_wifi_popover(
    screen_size: Size<i32, Physical>,
    state: &WifiPopoverRenderState<'_>,
    x: f64,
    y: f64,
) -> WifiPopoverHit {
    let layout = compute_wifi_popover_layout(screen_size, state);
    if point_in_rect(layout.switch_rect, x, y) {
        return WifiPopoverHit::ToggleWifi;
    }
    for slot in layout.known_rows.iter().chain(layout.other_rows.iter()) {
        if point_in_rect(slot.rect, x, y) {
            return WifiPopoverHit::SelectNetwork(slot.network_index);
        }
    }
    if point_in_rect(layout.settings_rect, x, y) {
        return WifiPopoverHit::OpenWifiSettings;
    }
    if point_in_rect(layout.panel_rect, x, y) {
        return WifiPopoverHit::InsidePopover;
    }
    WifiPopoverHit::OutsidePopover
}

fn push_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    text: &str,
    size: f32,
    rgba: [u8; 4],
    pos: (i32, i32),
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    if let Some(el) = text_cache.render_label(renderer, text, size, rgba, Point::from(pos)) {
        elements.push(el);
    }
}

fn push_bold_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    text: &str,
    size: f32,
    rgba: [u8; 4],
    pos: (i32, i32),
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    if let Some(el) = text_cache.render_label_weighted(
        renderer,
        text,
        crate::text::FontWeight::Bold,
        size,
        rgba,
        Point::from(pos),
    ) {
        elements.push(el);
    }
}

fn render_wifi_network_rows_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    layout: &WifiPopoverLayout,
    state: &WifiPopoverRenderState<'_>,
    pal: &ThemePalette,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let px = layout.panel_rect.loc.x;
    if let Some(kh) = layout.known_header_rect {
        let title = if layout.known_rows.len() == 1 {
            "Known Network"
        } else {
            "Known Networks"
        };
        push_bold_text(
            text_cache,
            renderer,
            title,
            11.0,
            pal.popover_text_muted,
            (kh.loc.x, kh.loc.y),
            elements,
        );
    }

    for slot in &layout.known_rows {
        let Some(ap) = state.network.wifi_networks.get(slot.network_index) else {
            continue;
        };
        let hovered = state.hovered_hit == Some(WifiPopoverHit::SelectNetwork(slot.network_index));
        let fg = if hovered {
            [255, 255, 255, 255]
        } else {
            pal.popover_text_primary
        };
        let ssid_label = truncate_ascii(&ap.ssid, 22);
        if ap.connected {
            push_bold_text(
                text_cache,
                renderer,
                &ssid_label,
                13.0,
                fg,
                (px + 38, slot.rect.loc.y + 6),
                elements,
            );
        } else {
            push_text(
                text_cache,
                renderer,
                &ssid_label,
                13.0,
                fg,
                (px + 38, slot.rect.loc.y + 6),
                elements,
            );
        }
    }

    if let Some(oh) = layout.other_header_rect {
        push_bold_text(
            text_cache,
            renderer,
            "Other Networks",
            11.0,
            pal.popover_text_muted,
            (oh.loc.x, oh.loc.y),
            elements,
        );
    }

    for slot in &layout.other_rows {
        let Some(ap) = state.network.wifi_networks.get(slot.network_index) else {
            continue;
        };
        let hovered = state.hovered_hit == Some(WifiPopoverHit::SelectNetwork(slot.network_index));
        let fg = if hovered {
            [255, 255, 255, 255]
        } else {
            pal.popover_text_primary
        };
        push_text(
            text_cache,
            renderer,
            &truncate_ascii(&ap.ssid, 22),
            13.0,
            fg,
            (px + 35, slot.rect.loc.y + 4),
            elements,
        );
    }
}

/// Appends text labels for [`ShellOverlayView::Wifi`] (`RUSTYOS-108`).
pub(super) fn render_wifi_popover_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, Physical>,
    state: &WifiPopoverRenderState<'_>,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let layout = compute_wifi_popover_layout(screen_size, state);
    let pal = ThemePalette::active();
    let px = layout.panel_rect.loc.x;
    let py = layout.panel_rect.loc.y;
    let w = layout.panel_rect.size.w;

    push_bold_text(
        text_cache,
        renderer,
        "Wi-Fi",
        13.0,
        pal.popover_text_primary,
        (px + 14, py + 12),
        elements,
    );

    if let Some(wired) = layout.wired_rect {
        let iface_title = state
            .network
            .wired_interface
            .as_deref()
            .map_or_else(|| "Ethernet".to_owned(), |i| format!("Ethernet ({i})"));
        push_text(
            text_cache,
            renderer,
            &truncate_ascii(&iface_title, 22),
            12.5,
            pal.popover_text_primary,
            (px + 35, wired.loc.y + 5),
            elements,
        );
        let status_label = "Connected";
        let (sw, _) = text_cache.measure_text(status_label, 11.0);
        push_text(
            text_cache,
            renderer,
            status_label,
            11.0,
            pal.popover_text_muted,
            (px + w - 14 - sw, wired.loc.y + 6),
            elements,
        );
    }

    render_wifi_network_rows_text(text_cache, renderer, &layout, state, &pal, elements);

    if let Some(empty) = layout.empty_rect {
        push_text(
            text_cache,
            renderer,
            "No Wi-Fi networks in range",
            12.0,
            pal.popover_text_muted,
            (px + 14, empty.loc.y + 5),
            elements,
        );
    }

    let settings_fg = if state.hovered_hit == Some(WifiPopoverHit::OpenWifiSettings) {
        [255, 255, 255, 255]
    } else {
        pal.popover_text_primary
    };
    push_text(
        text_cache,
        renderer,
        "Wi-Fi Settings...",
        13.0,
        settings_fg,
        (px + 14, layout.settings_rect.loc.y + 4),
        elements,
    );
}

fn push_wifi_glyphs(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    layout: &WifiPopoverLayout,
    state: &WifiPopoverRenderState<'_>,
    pal: &ThemePalette,
) {
    let px = layout.panel_rect.loc.x;
    let w = layout.panel_rect.size.w;

    // Header switch knob (front of switch track)
    let knob_x = if state.network.wifi_enabled {
        layout.switch_rect.loc.x + layout.switch_rect.size.w - 18
    } else {
        layout.switch_rect.loc.x + 2
    };
    if let Some(el) = shape_cache.circle_element(
        renderer,
        16,
        [255, 255, 255, 255],
        None,
        Point::from((knob_x, layout.switch_rect.loc.y + 2)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }

    if let Some(wired) = layout.wired_rect
        && let Some(el) = shape_cache.ui_glyph_element(
            renderer,
            UiGlyphKind::Ethernet,
            14,
            [40, 205, 65, 255],
            Point::from((px + 14, wired.loc.y + 6)),
        )
    {
        out.push(CompositorRenderElement::Memory(el));
    }

    for slot in layout.known_rows.iter().chain(layout.other_rows.iter()) {
        let Some(ap) = state.network.wifi_networks.get(slot.network_index) else {
            continue;
        };
        let hovered = state.hovered_hit == Some(WifiPopoverHit::SelectNetwork(slot.network_index));
        let glyph_tint = if hovered || (slot.is_known && ap.connected) {
            [255, 255, 255, 255]
        } else {
            pal.popover_icon
        };
        let (icon_sz, icon_pos) = if slot.is_known {
            (12, Point::from((px + 16, slot.rect.loc.y + 8)))
        } else {
            (14, Point::from((px + 14, slot.rect.loc.y + 5)))
        };
        if let Some(el) = shape_cache.ui_glyph_element(
            renderer,
            UiGlyphKind::Wifi {
                connected: true,
                signal_percent: Some(ap.signal_percent),
            },
            icon_sz,
            glyph_tint,
            icon_pos,
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }

        if ap.secure {
            let lock_y = slot.rect.loc.y + (slot.rect.size.h - 12) / 2;
            let lock_tint = if hovered {
                [255, 255, 255, 255]
            } else {
                pal.popover_text_muted
            };
            if let Some(el) = shape_cache.ui_glyph_element(
                renderer,
                UiGlyphKind::Lock,
                12,
                lock_tint,
                Point::from((px + w - 26, lock_y)),
            ) {
                out.push(CompositorRenderElement::Memory(el));
            }
        }
    }
}

fn hover_pill_rect(row: Rectangle<i32, Physical>) -> Rectangle<i32, Physical> {
    Rectangle::new(
        Point::from((
            row.loc.x + HOVER_PILL_INSET_H,
            row.loc.y + HOVER_PILL_INSET_V,
        )),
        Size::from((
            (row.size.w - HOVER_PILL_INSET_H * 2).max(12),
            (row.size.h - HOVER_PILL_INSET_V * 2).max(12),
        )),
    )
}

/// Appends Liquid Glass surface, circles, concentric hover pills, and switches for [`ShellOverlayView::Wifi`] (`RUSTYOS-108`).
pub(super) fn render_wifi_popover_composite(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, Physical>,
    state: &WifiPopoverRenderState<'_>,
) {
    let layout = compute_wifi_popover_layout(screen_size, state);
    let pal = ThemePalette::active();
    let px = layout.panel_rect.loc.x;

    // 1. Frontmost icons and switch knob
    push_wifi_glyphs(out, shape_cache, renderer, &layout, state, &pal);

    // 2. Circular badges for Known Networks (behind their white/dark Wi-Fi glyph, in front of hover pill)
    for slot in &layout.known_rows {
        let Some(ap) = state.network.wifi_networks.get(slot.network_index) else {
            continue;
        };
        let hovered = state.hovered_hit == Some(WifiPopoverHit::SelectNetwork(slot.network_index));
        let circle_fill = if hovered {
            [255, 255, 255, 48]
        } else if ap.connected {
            pal.accent_rgba
        } else {
            pal.control_pill_off_rgba
        };
        if let Some(el) = shape_cache.circle_element(
            renderer,
            20,
            circle_fill,
            None,
            Point::from((px + 12, slot.rect.loc.y + 4)),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
    }

    // 3. Switch track pill
    let track_fill = if state.network.wifi_enabled {
        pal.accent_rgba
    } else {
        pal.control_pill_off_rgba
    };
    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        layout.switch_rect,
        10,
        track_fill,
        None,
    );

    // 4. Concentric accent hover pills (`r = 7`, inset `5 px`)
    for slot in layout.known_rows.iter().chain(layout.other_rows.iter()) {
        if state.hovered_hit == Some(WifiPopoverHit::SelectNetwork(slot.network_index)) {
            push_rounded_glass_box(
                out,
                shape_cache,
                renderer,
                hover_pill_rect(slot.rect),
                HOVER_PILL_RADIUS,
                pal.accent_rgba,
                None,
            );
        }
    }
    if state.hovered_hit == Some(WifiPopoverHit::OpenWifiSettings) {
        push_rounded_glass_box(
            out,
            shape_cache,
            renderer,
            hover_pill_rect(layout.settings_rect),
            HOVER_PILL_RADIUS,
            pal.accent_rgba,
            None,
        );
    }

    // 5. Horizontal hairline dividers (`12 px` inset)
    for &sep_y in &layout.separators {
        out.push(CompositorRenderElement::Solid(make_solid_element(
            Rectangle::new(
                Point::from((px + 12, sep_y)),
                Size::from(((layout.panel_rect.size.w - 24).max(12), 1)),
            ),
            pal.popover_separator,
        )));
    }

    // 6. Backmost Liquid Glass Popover panel (`r = 13`, specular rim, 3-lobe MenuOrTooltip shadow)
    push_liquid_glass_panel_elevation(
        out,
        shape_cache,
        renderer,
        layout.panel_rect,
        MENU_POPOVER_RADIUS,
        GlassPanelStyle::Popover,
        state.glass_opacity,
        state.reduce_transparency,
        rusty_ui::shadow::ShadowElevation::MenuOrTooltip,
    );
}
