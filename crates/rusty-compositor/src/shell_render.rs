//! Visual shell elements rendering (`TopBar`, `Dock`, Desktop background) (`RUSTYOS-012`, ADR-0007).
//!
//! Renders macOS-inspired top status bar and bottom floating dock using Smithay's
//! solid color render elements, adhering to design tokens in `DESIGN.md` (Version 2.0 "RustyOS Purple Glass").

use smithay::backend::renderer::Color32F;
use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
use smithay::backend::renderer::element::solid::SolidColorRenderElement;
use smithay::backend::renderer::element::{Id, Kind};
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::renderer::utils::CommitCounter;
use smithay::utils::{Point, Rectangle, Size};

/// `TopBar` height in logical pixels.
pub const TOP_BAR_HEIGHT: i32 = 32;

/// Server-Side Decoration (SSD) titlebar height in logical pixels (`RUSTYOS-019`).
pub const TITLEBAR_HEIGHT: i32 = 28;

/// Dock height in logical pixels.
pub const DOCK_HEIGHT: i32 = 56;

/// Dock bottom margin from screen edge.
pub const DOCK_MARGIN_BOTTOM: i32 = 12;

/// Workspace button width and stride for topbar indicators and click handling.
pub const WS_BTN_WIDTH: i32 = 26;
pub const WS_BTN_GAP: i32 = 4;
pub const WS_BTN_STRIDE: i32 = WS_BTN_WIDTH + WS_BTN_GAP;
pub const WS_START_X: i32 = 12;

const fn premul(r: f32, g: f32, b: f32, a: f32) -> Color32F {
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

fn make_solid_element(
    geo: Rectangle<i32, smithay::utils::Physical>,
    color: Color32F,
) -> SolidColorRenderElement {
    SolidColorRenderElement::new(
        Id::new(),
        geo,
        CommitCounter::default(),
        color,
        Kind::Unspecified,
    )
}

/// Live hardware/session telemetry displayed inside `TopBar` (`RUSTYOS-021`).
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct TopBarTelemetry<'a> {
    /// Local wall-clock time (`HH:MM`).
    pub clock_hhmm: &'a str,
    /// Battery percentage (`0..=100`).
    pub battery_percent: u8,
    /// Whether a hardware battery is present.
    pub has_battery: bool,
    /// Whether an active network interface is up.
    pub net_connected: bool,
    /// Master output volume (`0..=100`).
    pub volume_percent: u8,
    /// Whether master audio output is muted.
    pub audio_muted: bool,
    /// Whether the Quick Settings popover is currently open.
    pub quick_settings_open: bool,
}

impl Default for TopBarTelemetry<'static> {
    fn default() -> Self {
        Self {
            clock_hhmm: "12:00",
            battery_percent: 100,
            has_battery: false,
            net_connected: true,
            volume_percent: 75,
            audio_muted: false,
            quick_settings_open: false,
        }
    }
}

/// Live data for the Quick Settings Control Center popover (`RUSTYOS-021`).
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct QuickSettingsRenderState<'a> {
    /// Active workspace layout mode (`"floating"` or `"master-stack"`).
    pub layout_mode: &'a str,
    /// Accessibility reduced-motion flag.
    pub reduced_motion: bool,
    /// Human-readable network label (SSID / Ethernet / Disconnected).
    pub network_label: &'a str,
    /// Whether network interface is up.
    pub net_connected: bool,
    /// Battery charge percentage.
    pub battery_percent: u8,
    /// Whether a hardware battery was detected.
    pub has_battery: bool,
    /// Whether AC adapter is connected.
    pub ac_online: bool,
    /// Master output volume percentage (`0..=100`).
    pub volume_percent: u8,
    /// Master audio mute flag.
    pub audio_muted: bool,
    /// Median frame interval in microseconds (`p50`).
    pub frame_p50_us: u64,
    /// Number of visible windows on active workspace.
    pub window_count: usize,
}

/// Active modal/popover overlay rendered on top of the compositor shell.
#[derive(Debug, Clone, Copy)]
pub enum ShellOverlayView<'a> {
    /// No overlay open.
    None,
    /// Spotlight application launcher overlay.
    Spotlight(&'a SpotlightRenderState<'a>),
    /// Quick Settings Control Center popover (`RUSTYOS-021`).
    QuickSettings(&'a QuickSettingsRenderState<'a>),
}

/// Interactive click target inside or outside the Quick Settings popover (`RUSTYOS-021`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickSettingsHit {
    /// Mute / unmute audio output.
    ToggleMute,
    /// Decrease audio volume.
    VolumeDown,
    /// Increase audio volume.
    VolumeUp,
    /// Toggle workspace layout (`Floating <-> MasterStack`).
    ToggleLayout,
    /// Toggle accessibility `reduced_motion`.
    ToggleReducedMotion,
    /// Switch to Spotlight application launcher.
    OpenSpotlight,
    /// Trigger immediate background telemetry refresh.
    RefreshMetrics,
    /// Click inside popover frame (absorb event, keep open).
    InsidePopover,
    /// Click outside popover (dismiss Quick Settings).
    OutsidePopover,
}

fn status_pill_rect(screen_width: i32) -> (i32, i32, i32, i32) {
    let status_pill_width = 186.min((screen_width - 370).max(120));
    let status_x = screen_width - status_pill_width - 12;
    (status_x, 6, status_pill_width, 20)
}

fn quick_settings_rect(width: i32, height: i32) -> (i32, i32, i32, i32) {
    let pop_w = 304.min(width - 24).max(240);
    let pop_h = 286.min(height - TOP_BAR_HEIGHT - 24).max(200);
    let pop_x = width - pop_w - 12;
    let pop_y = TOP_BAR_HEIGHT + 8;
    (pop_x, pop_y, pop_w, pop_h)
}

/// Checks if `(x, y)` clicked the `TopBar` Layout badge (`Float` / `Tile`).
#[must_use]
pub fn check_top_bar_layout_click(x: f64, y: f64) -> bool {
    if y > f64::from(TOP_BAR_HEIGHT) {
        return false;
    }
    let layout_x = f64::from(WS_START_X + 9 * WS_BTN_STRIDE + 8);
    x >= layout_x && x < layout_x + 56.0
}

/// Checks if `(x, y)` clicked the right `TopBar` telemetry status pill.
#[must_use]
pub fn check_top_bar_status_click(screen_width: i32, x: f64, y: f64) -> bool {
    if y > f64::from(TOP_BAR_HEIGHT) {
        return false;
    }
    let (sx, _sy, sw, _sh) = status_pill_rect(screen_width);
    x >= f64::from(sx) && x < f64::from(sx + sw)
}

/// Hit-tests `(x, y)` against the open Quick Settings popover controls (`RUSTYOS-021`).
#[must_use]
pub fn check_quick_settings_click(
    screen_size: Size<i32, smithay::utils::Physical>,
    x: f64,
    y: f64,
) -> QuickSettingsHit {
    let (px, py, pw, ph) = quick_settings_rect(screen_size.w, screen_size.h);
    let fx = f64::from(px);
    let fy = f64::from(py);
    let fw = f64::from(pw);
    let fh = f64::from(ph);

    if x < fx || x >= fx + fw || y < fy || y >= fy + fh {
        return QuickSettingsHit::OutsidePopover;
    }

    let pad = 14;
    let inner_w = pw - pad * 2;
    let half_w = (inner_w - 8) / 2;

    // Audio control buttons row: y in py + 136 .. py + 158
    let audio_btn_y = f64::from(py + 136);
    if y >= audio_btn_y && y < audio_btn_y + 22.0 {
        let mute_x = f64::from(px + pw - pad - 116);
        let minus_x = f64::from(px + pw - pad - 62);
        let plus_x = f64::from(px + pw - pad - 30);
        if x >= mute_x && x < mute_x + 48.0 {
            return QuickSettingsHit::ToggleMute;
        }
        if x >= minus_x && x < minus_x + 28.0 {
            return QuickSettingsHit::VolumeDown;
        }
        if x >= plus_x && x < plus_x + 28.0 {
            return QuickSettingsHit::VolumeUp;
        }
    }

    let left_x = f64::from(px + pad);
    let right_x = f64::from(px + pad + half_w + 8);
    let hw = f64::from(half_w);

    // Action row 1: py + 202 .. py + 234
    let row1_y = f64::from(py + 202);
    if y >= row1_y && y < row1_y + 32.0 {
        if x >= left_x && x < left_x + hw {
            return QuickSettingsHit::ToggleLayout;
        }
        if x >= right_x && x < right_x + hw {
            return QuickSettingsHit::ToggleReducedMotion;
        }
    }

    // Action row 2: py + 242 .. py + 274
    let row2_y = f64::from(py + 242);
    if y >= row2_y && y < row2_y + 32.0 {
        if x >= left_x && x < left_x + hw {
            return QuickSettingsHit::OpenSpotlight;
        }
        if x >= right_x && x < right_x + hw {
            return QuickSettingsHit::RefreshMetrics;
        }
    }

    QuickSettingsHit::InsidePopover
}

fn format_status_pill_label(t: &TopBarTelemetry<'_>) -> String {
    let vol_part = if t.audio_muted {
        "Mute".to_owned()
    } else {
        format!("V{}%", t.volume_percent)
    };
    if t.has_battery {
        format!("{vol_part} • {}% • {}", t.battery_percent, t.clock_hhmm)
    } else {
        let net_part = if t.net_connected { "Online" } else { "Offline" };
        format!("{net_part} • {vol_part} • {}", t.clock_hhmm)
    }
}

fn truncate_ascii(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        text.to_owned()
    } else {
        let mut out: String = text.chars().take(max_chars.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

fn render_top_bar(
    elements: &mut Vec<SolidColorRenderElement>,
    width: i32,
    active_workspace: u8,
    layout_mode: &str,
    telemetry: &TopBarTelemetry<'_>,
    has_text: bool,
) {
    // 1. TopBar Background (full width, height 32, elevated surface)
    let top_bar_geo = Rectangle::new(Point::from((0, 0)), Size::from((width, TOP_BAR_HEIGHT)));
    elements.push(make_solid_element(top_bar_geo, COLOR_SURFACE_ELEVATED));

    // 1b. TopBar 1px bottom border (subtle translucency)
    let top_bar_border =
        Rectangle::new(Point::from((0, TOP_BAR_HEIGHT - 1)), Size::from((width, 1)));
    elements.push(make_solid_element(top_bar_border, COLOR_BORDER));

    // 2. Workspaces indicators in TopBar (1..=9)
    let ws_btn_height = 20;
    let ws_y = 6;
    let mut start_x = WS_START_X;

    for ws in 1..=9 {
        let is_active = ws == active_workspace;
        let ws_geo = Rectangle::new(
            Point::from((start_x, ws_y)),
            Size::from((WS_BTN_WIDTH, ws_btn_height)),
        );

        if is_active {
            // Bright neon violet pill with bottom highlight pip
            elements.push(make_solid_element(ws_geo, COLOR_ACCENT));
            let pip_geo = Rectangle::new(
                Point::from((start_x + (WS_BTN_WIDTH - 8) / 2, ws_y + ws_btn_height - 2)),
                Size::from((8, 2)),
            );
            elements.push(make_solid_element(pip_geo, COLOR_TEXT));
        } else {
            // Contrast elevated overlay pill
            elements.push(make_solid_element(ws_geo, COLOR_SURFACE_OVERLAY));
            if !has_text {
                let pip_geo = Rectangle::new(
                    Point::from((
                        start_x + (WS_BTN_WIDTH - 6) / 2,
                        ws_y + (ws_btn_height - 2) / 2,
                    )),
                    Size::from((6, 2)),
                );
                elements.push(make_solid_element(pip_geo, COLOR_SUBTLE));
            }
        }
        start_x += WS_BTN_STRIDE;
    }

    // 3. Layout indicator in TopBar
    let layout_bg = Rectangle::new(
        Point::from((start_x + 8, ws_y)),
        Size::from((56, ws_btn_height)),
    );
    elements.push(make_solid_element(layout_bg, COLOR_SURFACE_OVERLAY));

    // Left accent strip for layout indicator
    let layout_accent = if layout_mode == "master-stack" {
        COLOR_ACCENT
    } else {
        COLOR_SUBTLE
    };
    let layout_strip = Rectangle::new(
        Point::from((start_x + 8, ws_y)),
        Size::from((3, ws_btn_height)),
    );
    elements.push(make_solid_element(layout_strip, layout_accent));

    // 4. Status area on the right side of TopBar (battery, network, audio, clock)
    let (status_x, status_y, status_pill_width, status_h) = status_pill_rect(width);
    let status_pill_geo = Rectangle::new(
        Point::from((status_x, status_y)),
        Size::from((status_pill_width, status_h)),
    );
    let pill_bg = if telemetry.quick_settings_open {
        COLOR_ACCENT_MUTED
    } else {
        COLOR_SURFACE_OVERLAY
    };
    elements.push(make_solid_element(status_pill_geo, pill_bg));

    // Top subtle highlight on status pill
    let status_highlight = Rectangle::new(
        Point::from((status_x, status_y)),
        Size::from((status_pill_width, 1)),
    );
    let highlight_col = if telemetry.quick_settings_open {
        COLOR_ACCENT
    } else {
        COLOR_BORDER
    };
    elements.push(make_solid_element(status_highlight, highlight_col));

    // Status dot (green if online, amber if offline)
    let dot_geo = Rectangle::new(Point::from((status_x + 8, ws_y + 6)), Size::from((8, 8)));
    let dot_color = if telemetry.net_connected {
        COLOR_SUCCESS
    } else {
        COLOR_WARNING
    };
    elements.push(make_solid_element(dot_geo, dot_color));

    if !has_text {
        // Accent status bars fallback when no font is available
        let bar1_geo = Rectangle::new(Point::from((status_x + 24, ws_y + 8)), Size::from((40, 4)));
        elements.push(make_solid_element(bar1_geo, COLOR_MUTED));
        let bar2_geo = Rectangle::new(Point::from((status_x + 72, ws_y + 6)), Size::from((46, 8)));
        elements.push(make_solid_element(bar2_geo, COLOR_ACCENT_MUTED));
    }
}

fn render_dock(
    elements: &mut Vec<SolidColorRenderElement>,
    width: i32,
    height: i32,
    window_count: usize,
    has_focused_window: bool,
) {
    // 5. Dock Background (floating centered pill at the bottom with outer rim)
    let dock_items_count = (3 + window_count).max(3); // 3 pinned apps + open windows
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let dock_width = ((dock_items_count as i32) * 52 + 16).min(width - 48);
    let dock_x = (width - dock_width) / 2;
    let dock_y = height - DOCK_HEIGHT - DOCK_MARGIN_BOTTOM;

    // 5a. Outer glass highlight rim (1px around dock)
    let dock_rim_geo = Rectangle::new(
        Point::from((dock_x - 1, dock_y - 1)),
        Size::from((dock_width + 2, DOCK_HEIGHT + 2)),
    );
    elements.push(make_solid_element(dock_rim_geo, COLOR_BORDER_HIGHLIGHT));

    // 5b. Dock body
    let dock_geo = Rectangle::new(
        Point::from((dock_x, dock_y)),
        Size::from((dock_width, DOCK_HEIGHT)),
    );
    elements.push(make_solid_element(dock_geo, COLOR_SURFACE_ELEVATED));

    // 6. Dock Item slots
    let mut item_x = dock_x + 8;
    let item_y = dock_y + 8;
    let item_size = 40;

    for i in 0..dock_items_count {
        let is_running_window = i >= 3;
        let is_focused = is_running_window && has_focused_window;

        // Base icon tile
        let item_geo = Rectangle::new(
            Point::from((item_x, item_y)),
            Size::from((item_size, item_size)),
        );
        elements.push(make_solid_element(item_geo, COLOR_SURFACE_OVERLAY));

        // Inner app glyph plate
        let inner_size = 28;
        let inner_offset = (item_size - inner_size) / 2;
        let inner_geo = Rectangle::new(
            Point::from((item_x + inner_offset, item_y + inner_offset)),
            Size::from((inner_size, inner_size)),
        );
        let inner_color = match i {
            0 => COLOR_ACCENT,       // Terminal / Core
            1 => COLOR_ACCENT_MUTED, // Files
            2 => COLOR_SUBTLE,       // Browser / Settings
            _ if is_focused => COLOR_ACCENT_HOVER,
            _ => COLOR_MUTED,
        };
        elements.push(make_solid_element(inner_geo, inner_color));

        // Active running indicator pip under running windows
        if is_running_window || i == 0 {
            let pip_width = 8;
            let pip_geo = Rectangle::new(
                Point::from((
                    item_x + (item_size - pip_width) / 2,
                    dock_y + DOCK_HEIGHT - 5,
                )),
                Size::from((pip_width, 2)),
            );
            elements.push(make_solid_element(pip_geo, COLOR_ACCENT));
        }

        item_x += item_size + 12;
    }
}

/// Live Spotlight view state passed into shell rendering (`RUSTYOS-018b`).
#[derive(Debug, Clone)]
pub struct SpotlightRenderState<'a> {
    /// Current search string entered by the user.
    pub query: &'a str,
    /// Highlighted row offset within `items` (`0..3`).
    pub selected_in_window: usize,
    /// Horizontal offset of the cursor relative to search text origin.
    pub cursor_offset_x: i32,
    /// Up to 3 visible filtered candidates to render.
    pub items: &'a [crate::launcher::LauncherCandidate],
}

/// Interactive click target inside or outside the Spotlight modal (`RUSTYOS-018b`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpotlightHit {
    /// Clicked one of the visible result rows (`0..3`).
    Row(usize),
    /// Clicked inside the modal frame/search box (keep Spotlight open).
    InsideModal,
    /// Clicked outside the modal (dismiss Spotlight).
    OutsideModal,
}

#[must_use]
fn spotlight_modal_rect(width: i32, height: i32) -> (i32, i32, i32, i32) {
    let modal_width = 560.min(width - 40).max(280);
    let modal_height = 240.min(height - 80).max(140);
    let modal_x = (width - modal_width) / 2;
    let modal_y = (height - modal_height) / 3;
    (modal_x, modal_y, modal_width, modal_height)
}

/// Hit-tests `(x, y)` against the open Spotlight modal and its up to 3 visible rows (`RUSTYOS-018b`).
#[must_use]
pub fn check_spotlight_click(
    screen_size: Size<i32, smithay::utils::Physical>,
    x: f64,
    y: f64,
) -> SpotlightHit {
    let (modal_x, modal_y, modal_width, modal_height) =
        spotlight_modal_rect(screen_size.w, screen_size.h);
    let mx = f64::from(modal_x);
    let my = f64::from(modal_y);
    let mw = f64::from(modal_width);
    let mh = f64::from(modal_height);

    if x < mx || x >= mx + mw || y < my || y >= my + mh {
        return SpotlightHit::OutsideModal;
    }

    let row_h = 44;
    let start_row_y = modal_y + 80;
    let row_x_min = f64::from(modal_x + 16);
    let row_x_max = f64::from(modal_x + modal_width - 16);
    if x >= row_x_min && x < row_x_max {
        for i in 0..3_i32 {
            let ry = f64::from(start_row_y + i * (row_h + 4));
            if y >= ry && y < ry + f64::from(row_h) {
                return SpotlightHit::Row(usize::try_from(i).unwrap_or(0));
            }
        }
    }

    SpotlightHit::InsideModal
}

fn render_spotlight_modal(
    elements: &mut Vec<SolidColorRenderElement>,
    width: i32,
    height: i32,
    spotlight: &SpotlightRenderState<'_>,
    has_text: bool,
) {
    let (modal_x, modal_y, modal_width, modal_height) = spotlight_modal_rect(width, height);

    // 7. Spotlight Glass Box - Outer glass highlight rim
    let rim_geo = Rectangle::new(
        Point::from((modal_x - 1, modal_y - 1)),
        Size::from((modal_width + 2, modal_height + 2)),
    );
    elements.push(make_solid_element(rim_geo, COLOR_BORDER_HIGHLIGHT));

    // 7a. Modal elevated body
    let modal_geo = Rectangle::new(
        Point::from((modal_x, modal_y)),
        Size::from((modal_width, modal_height)),
    );
    elements.push(make_solid_element(modal_geo, COLOR_SURFACE_ELEVATED));

    // 7b. Search input bar plate
    let search_bar_h = 44;
    let search_bar_geo = Rectangle::new(
        Point::from((modal_x + 16, modal_y + 16)),
        Size::from((modal_width - 32, search_bar_h)),
    );
    elements.push(make_solid_element(search_bar_geo, COLOR_SURFACE_OVERLAY));

    // 7c. Search bar accent indicator
    let search_icon_geo = Rectangle::new(
        Point::from((modal_x + 28, modal_y + 26)),
        Size::from((24, 24)),
    );
    elements.push(make_solid_element(search_icon_geo, COLOR_ACCENT));

    // 7d. Dynamic text input cursor placed after typed query
    let cursor_x = (modal_x + 64 + spotlight.cursor_offset_x).min(modal_x + modal_width - 28);
    let cursor_geo = Rectangle::new(Point::from((cursor_x, modal_y + 26)), Size::from((2, 24)));
    elements.push(make_solid_element(cursor_geo, COLOR_TEXT));

    if !has_text {
        // 7e. Placeholder text indicator when no font renderer is active
        let placeholder_geo = Rectangle::new(
            Point::from((modal_x + 70, modal_y + 34)),
            Size::from((140, 8)),
        );
        elements.push(make_solid_element(placeholder_geo, COLOR_MUTED));
    }

    // 7f. Search divider
    let divider_geo = Rectangle::new(
        Point::from((modal_x + 16, modal_y + 70)),
        Size::from((modal_width - 32, 1)),
    );
    elements.push(make_solid_element(divider_geo, COLOR_BORDER));

    // 7g. Search results (up to 3 visible matching applications)
    let row_h = 44;
    let start_row_y = modal_y + 80;
    let row_count = spotlight.items.len().min(3);
    for idx in 0..row_count {
        let i = i32::try_from(idx).unwrap_or(0);
        let row_y = start_row_y + i * (row_h + 4);
        let is_selected = idx == spotlight.selected_in_window;

        let row_bg = Rectangle::new(
            Point::from((modal_x + 16, row_y)),
            Size::from((modal_width - 32, row_h)),
        );
        let bg_color = if is_selected {
            COLOR_SURFACE_OVERLAY
        } else {
            COLOR_SURFACE
        };
        elements.push(make_solid_element(row_bg, bg_color));

        if is_selected {
            let select_indicator =
                Rectangle::new(Point::from((modal_x + 16, row_y)), Size::from((3, row_h)));
            elements.push(make_solid_element(select_indicator, COLOR_ACCENT));
        }

        // App icon badge
        let app_icon = Rectangle::new(
            Point::from((modal_x + 28, row_y + 10)),
            Size::from((24, 24)),
        );
        let icon_color = if is_selected {
            COLOR_ACCENT
        } else if idx == 1 {
            COLOR_ACCENT_MUTED
        } else {
            COLOR_SUBTLE
        };
        elements.push(make_solid_element(app_icon, icon_color));

        if !has_text {
            // App title bar fallback
            let title_bar = Rectangle::new(
                Point::from((modal_x + 62, row_y + 12)),
                Size::from((120 + i * 20, 8)),
            );
            elements.push(make_solid_element(title_bar, COLOR_TEXT));

            // App subtitle bar fallback
            let sub_bar = Rectangle::new(
                Point::from((modal_x + 62, row_y + 26)),
                Size::from((80 + i * 15, 6)),
            );
            elements.push(make_solid_element(sub_bar, COLOR_MUTED));
        }
    }
}

#[allow(clippy::too_many_lines)]
fn render_quick_settings_popover(
    elements: &mut Vec<SolidColorRenderElement>,
    width: i32,
    height: i32,
    qs: &QuickSettingsRenderState<'_>,
) {
    let (px, py, pw, ph) = quick_settings_rect(width, height);
    let pad = 14;
    let inner_w = pw - pad * 2;
    let half_w = (inner_w - 8) / 2;

    // 1. Outer glass rim + elevated body
    elements.push(make_solid_element(
        Rectangle::new(Point::from((px - 1, py - 1)), Size::from((pw + 2, ph + 2))),
        COLOR_BORDER_HIGHLIGHT,
    ));
    elements.push(make_solid_element(
        Rectangle::new(Point::from((px, py)), Size::from((pw, ph))),
        COLOR_SURFACE_ELEVATED,
    ));

    // Header divider
    elements.push(make_solid_element(
        Rectangle::new(Point::from((px + pad, py + 52)), Size::from((inner_w, 1))),
        COLOR_BORDER,
    ));

    // 2. Network & Power info tiles
    let left_x = px + pad;
    let right_x = px + pad + half_w + 8;
    elements.push(make_solid_element(
        Rectangle::new(Point::from((left_x, py + 62)), Size::from((half_w, 56))),
        COLOR_SURFACE_OVERLAY,
    ));
    elements.push(make_solid_element(
        Rectangle::new(Point::from((left_x + 10, py + 72)), Size::from((8, 8))),
        if qs.net_connected {
            COLOR_SUCCESS
        } else {
            COLOR_DANGER
        },
    ));

    elements.push(make_solid_element(
        Rectangle::new(Point::from((right_x, py + 62)), Size::from((half_w, 56))),
        COLOR_SURFACE_OVERLAY,
    ));
    elements.push(make_solid_element(
        Rectangle::new(Point::from((right_x + 10, py + 72)), Size::from((8, 8))),
        if qs.ac_online || qs.battery_percent > 20 {
            COLOR_SUCCESS
        } else {
            COLOR_WARNING
        },
    ));

    // 3. Audio slider card
    elements.push(make_solid_element(
        Rectangle::new(Point::from((left_x, py + 128)), Size::from((inner_w, 64))),
        COLOR_SURFACE_OVERLAY,
    ));

    // Mute, -, + buttons inside audio card
    let btn_y = py + 136;
    let mute_x = px + pw - pad - 116;
    let minus_x = px + pw - pad - 62;
    let plus_x = px + pw - pad - 30;
    elements.push(make_solid_element(
        Rectangle::new(Point::from((mute_x, btn_y)), Size::from((48, 22))),
        if qs.audio_muted {
            COLOR_DANGER
        } else {
            COLOR_SURFACE
        },
    ));
    elements.push(make_solid_element(
        Rectangle::new(Point::from((minus_x, btn_y)), Size::from((28, 22))),
        COLOR_SURFACE,
    ));
    elements.push(make_solid_element(
        Rectangle::new(Point::from((plus_x, btn_y)), Size::from((28, 22))),
        COLOR_SURFACE,
    ));

    // Audio progress bar track + fill
    let track_w = inner_w - 24;
    let track_x = left_x + 12;
    let track_y = py + 170;
    elements.push(make_solid_element(
        Rectangle::new(Point::from((track_x, track_y)), Size::from((track_w, 8))),
        COLOR_SURFACE,
    ));
    let fill_w = (track_w * i32::from(qs.volume_percent.min(100))) / 100;
    if fill_w > 0 {
        elements.push(make_solid_element(
            Rectangle::new(Point::from((track_x, track_y)), Size::from((fill_w, 8))),
            if qs.audio_muted {
                COLOR_SUBTLE
            } else {
                COLOR_ACCENT
            },
        ));
    }

    // 4. Quick Action toggles (2x2 grid)
    let is_tile = qs.layout_mode == "master-stack";
    elements.push(make_solid_element(
        Rectangle::new(Point::from((left_x, py + 202)), Size::from((half_w, 32))),
        if is_tile {
            COLOR_ACCENT_MUTED
        } else {
            COLOR_SURFACE_OVERLAY
        },
    ));
    elements.push(make_solid_element(
        Rectangle::new(Point::from((right_x, py + 202)), Size::from((half_w, 32))),
        if qs.reduced_motion {
            COLOR_ACCENT_MUTED
        } else {
            COLOR_SURFACE_OVERLAY
        },
    ));
    elements.push(make_solid_element(
        Rectangle::new(Point::from((left_x, py + 242)), Size::from((half_w, 32))),
        COLOR_SURFACE_OVERLAY,
    ));
    elements.push(make_solid_element(
        Rectangle::new(Point::from((right_x, py + 242)), Size::from((half_w, 32))),
        COLOR_SURFACE_OVERLAY,
    ));
}

/// Generates custom GLES render elements for `TopBar`, `Dock`, and active shell overlays.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn generate_shell_render_elements(
    screen_size: Size<i32, smithay::utils::Physical>,
    active_workspace: u8,
    layout_mode: &str,
    window_count: usize,
    has_focused_window: bool,
    top_telemetry: &TopBarTelemetry<'_>,
    overlay: ShellOverlayView<'_>,
    has_text: bool,
) -> Vec<SolidColorRenderElement> {
    let mut elements = Vec::new();
    let width = screen_size.w;
    let height = screen_size.h;

    render_top_bar(
        &mut elements,
        width,
        active_workspace,
        layout_mode,
        top_telemetry,
        has_text,
    );
    render_dock(
        &mut elements,
        width,
        height,
        window_count,
        has_focused_window,
    );

    match overlay {
        ShellOverlayView::None => {}
        ShellOverlayView::Spotlight(spot) => {
            render_spotlight_modal(&mut elements, width, height, spot, has_text);
        }
        ShellOverlayView::QuickSettings(qs) => {
            render_quick_settings_popover(&mut elements, width, height, qs);
        }
    }

    // Smithay's render_output expects elements in front-to-back order.
    elements.reverse();
    elements
}

/// Checks if a mouse click at `(x, y)` was inside the `TopBar` workspaces area and returns the target workspace (1..=9).
#[must_use]
pub fn check_top_bar_workspace_click(x: f64, y: f64) -> Option<u8> {
    if y > f64::from(TOP_BAR_HEIGHT) {
        return None;
    }
    let start_x = f64::from(WS_START_X);
    let stride = f64::from(WS_BTN_STRIDE);
    let total_width = stride * 9.0;

    if x < start_x || x >= start_x + total_width {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let idx = ((x - start_x) / stride).floor() as u8 + 1;
    if (1..=9).contains(&idx) {
        Some(idx)
    } else {
        None
    }
}

/// Interactive hit zone on a window's Server-Side Decoration titlebar (`RUSTYOS-019`).
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
}

/// Hit-tests `(x, y)` against visible window titlebars in reverse (top-to-bottom) stacking order.
#[must_use]
pub fn check_window_ssd_hit(
    windows_bottom_to_top: &[(rusty_wm::WindowId, rusty_wm::Rect)],
    x: f64,
    y: f64,
) -> Option<SsdHit> {
    for &(win_id, rect) in windows_bottom_to_top.iter().rev() {
        let rx = f64::from(rect.x);
        let ry = f64::from(rect.y);
        let rw = f64::from(rect.width);
        let th = f64::from(TITLEBAR_HEIGHT);

        if x >= rx && x < rx + rw && y >= ry && y < ry + th {
            // Traffic-light hitboxes on the left of titlebar
            let rel_x = x - rx;
            let rel_y = y - ry;
            if (6.0..=22.0).contains(&rel_y) {
                if (10.0..=26.0).contains(&rel_x) {
                    return Some(SsdHit::Close(win_id));
                }
                if (28.0..=44.0).contains(&rel_x) {
                    return Some(SsdHit::Minimize(win_id));
                }
                if (46.0..=62.0).contains(&rel_x) {
                    return Some(SsdHit::Maximize(win_id));
                }
            }
            return Some(SsdHit::TitlebarDrag(win_id));
        }
    }
    None
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

/// Generates GPU-cached real font text labels for `TopBar`, `Dock`, `Spotlight`, and `QuickSettings` (`RUSTYOS-020`/`021`).
#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
pub fn generate_shell_text_elements(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    active_workspace: u8,
    layout_mode: &str,
    window_count: usize,
    top_telemetry: &TopBarTelemetry<'_>,
    overlay: ShellOverlayView<'_>,
) -> Vec<MemoryRenderBufferRenderElement<GlesRenderer>> {
    let mut elements = Vec::new();
    if !text_cache.has_font() {
        return elements;
    }

    let width = screen_size.w;
    let height = screen_size.h;
    let text_bright = [244, 240, 250, 255]; // #F4F0FA
    let text_muted = [166, 155, 181, 255]; // #A69BB5
    let text_subtle = [118, 107, 133, 255]; // #766B85

    // 1. Overlay labels (Spotlight or Quick Settings popover)
    match overlay {
        ShellOverlayView::None => {}
        ShellOverlayView::Spotlight(spot) => {
            let (modal_x, modal_y, _modal_width, _modal_height) =
                spotlight_modal_rect(width, height);

            // Search icon glyph centered inside accent plate
            let search_icon_bounds = Rectangle::new(
                Point::from((modal_x + 28, modal_y + 26)),
                Size::from((24, 24)),
            );
            if let Some(el) = text_cache.render_label_centered(
                renderer,
                "/",
                13.0,
                text_bright,
                search_icon_bounds,
            ) {
                elements.push(el);
            }

            if spot.query.is_empty() {
                if let Some(el) = text_cache.render_label(
                    renderer,
                    "Search RustyOS...",
                    14.5,
                    text_muted,
                    Point::from((modal_x + 70, modal_y + 28)),
                ) {
                    elements.push(el);
                }
            } else if let Some(el) = text_cache.render_label(
                renderer,
                spot.query,
                14.5,
                text_bright,
                Point::from((modal_x + 64, modal_y + 28)),
            ) {
                elements.push(el);
            }

            let start_row_y = modal_y + 80;
            let row_h = 44;
            for (i, item) in spot.items.iter().take(3).enumerate() {
                #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
                let row_y = start_row_y + (i as i32) * (row_h + 4);

                let badge_bounds = Rectangle::new(
                    Point::from((modal_x + 28, row_y + 10)),
                    Size::from((24, 24)),
                );
                if let Some(el) = text_cache.render_label_centered(
                    renderer,
                    &item.badge,
                    10.5,
                    text_bright,
                    badge_bounds,
                ) {
                    elements.push(el);
                }

                if let Some(el) = text_cache.render_label(
                    renderer,
                    &item.name,
                    13.5,
                    text_bright,
                    Point::from((modal_x + 62, row_y + 6)),
                ) {
                    elements.push(el);
                }
                if let Some(el) = text_cache.render_label(
                    renderer,
                    &item.subtitle,
                    10.5,
                    text_muted,
                    Point::from((modal_x + 62, row_y + 24)),
                ) {
                    elements.push(el);
                }
            }
        }
        ShellOverlayView::QuickSettings(qs) => {
            render_quick_settings_text(text_cache, renderer, width, height, qs, &mut elements);
        }
    }

    // 2. TopBar workspace digits (1..=9)
    let ws_btn_height = 20;
    let ws_y = 6;
    let mut start_x = WS_START_X;
    for ws in 1..=9 {
        let color = if ws == active_workspace {
            text_bright
        } else {
            text_muted
        };
        let bounds = Rectangle::new(
            Point::from((start_x, ws_y)),
            Size::from((WS_BTN_WIDTH, ws_btn_height)),
        );
        if let Some(el) =
            text_cache.render_label_centered(renderer, &ws.to_string(), 12.0, color, bounds)
        {
            elements.push(el);
        }
        start_x += WS_BTN_STRIDE;
    }

    // 3. Layout mode label
    let layout_label = if layout_mode == "master-stack" {
        "Tile"
    } else {
        "Float"
    };
    let layout_bounds = Rectangle::new(
        Point::from((start_x + 11, ws_y)),
        Size::from((53, ws_btn_height)),
    );
    if let Some(el) =
        text_cache.render_label_centered(renderer, layout_label, 11.5, text_bright, layout_bounds)
    {
        elements.push(el);
    }

    // 4. Right status pill live telemetry text
    let (status_x, _status_y, status_pill_width, _status_h) = status_pill_rect(width);
    let status_text = format_status_pill_label(top_telemetry);
    let status_bounds = Rectangle::new(
        Point::from((status_x + 18, ws_y)),
        Size::from((status_pill_width - 22, ws_btn_height)),
    );
    if let Some(el) =
        text_cache.render_label_centered(renderer, &status_text, 10.5, text_bright, status_bounds)
    {
        elements.push(el);
    }

    // 5. Dock app initials
    let dock_items_count = (3 + window_count).max(3);
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let dock_width = ((dock_items_count as i32) * 52 + 16).min(width - 48);
    let dock_x = (width - dock_width) / 2;
    let dock_y = height - DOCK_HEIGHT - DOCK_MARGIN_BOTTOM;
    let mut item_x = dock_x + 8;
    let item_y = dock_y + 8;
    let item_size = 40;

    for i in 0..dock_items_count {
        let label = match i {
            0 => ">_",
            1 => "Fs",
            2 => "GL",
            _ => "App",
        };
        let bounds = Rectangle::new(
            Point::from((item_x, item_y)),
            Size::from((item_size, item_size)),
        );
        let color = if i == 0 { text_bright } else { text_subtle };
        if let Some(el) = text_cache.render_label_centered(renderer, label, 12.5, color, bounds) {
            elements.push(el);
        }
        item_x += item_size + 12;
    }

    elements
}

#[allow(clippy::too_many_lines)]
fn render_quick_settings_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    width: i32,
    height: i32,
    qs: &QuickSettingsRenderState<'_>,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let (px, py, pw, _ph) = quick_settings_rect(width, height);
    let pad = 14;
    let inner_w = pw - pad * 2;
    let half_w = (inner_w - 8) / 2;
    let left_x = px + pad;
    let right_x = px + pad + half_w + 8;

    let text_bright = [244, 240, 250, 255];
    let text_muted = [166, 155, 181, 255];

    if let Some(el) = text_cache.render_label(
        renderer,
        "Quick Settings",
        13.5,
        text_bright,
        Point::from((left_x, py + 12)),
    ) {
        elements.push(el);
    }

    #[allow(clippy::cast_precision_loss)]
    let p50_ms = (qs.frame_p50_us as f64) / 1000.0;
    let perf_sub = format!("{p50_ms:.1} ms p50 • {} win", qs.window_count);
    if let Some(el) = text_cache.render_label(
        renderer,
        &perf_sub,
        10.5,
        text_muted,
        Point::from((left_x, py + 31)),
    ) {
        elements.push(el);
    }

    // Network tile
    if let Some(el) = text_cache.render_label(
        renderer,
        "Network",
        11.5,
        text_bright,
        Point::from((left_x + 24, py + 69)),
    ) {
        elements.push(el);
    }
    let net_sub = truncate_ascii(qs.network_label, 17);
    if let Some(el) = text_cache.render_label(
        renderer,
        &net_sub,
        10.5,
        text_muted,
        Point::from((left_x + 10, py + 92)),
    ) {
        elements.push(el);
    }

    // Power tile
    if let Some(el) = text_cache.render_label(
        renderer,
        "Power",
        11.5,
        text_bright,
        Point::from((right_x + 24, py + 69)),
    ) {
        elements.push(el);
    }
    let power_sub = if qs.has_battery {
        if qs.ac_online {
            format!("Bat {}% (AC)", qs.battery_percent)
        } else {
            format!("Battery {}%", qs.battery_percent)
        }
    } else {
        "AC Power".to_owned()
    };
    if let Some(el) = text_cache.render_label(
        renderer,
        &power_sub,
        10.5,
        text_muted,
        Point::from((right_x + 10, py + 92)),
    ) {
        elements.push(el);
    }

    // Audio row
    let vol_title = if qs.audio_muted {
        format!("Audio: Muted ({}%)", qs.volume_percent)
    } else {
        format!("Audio: {}%", qs.volume_percent)
    };
    if let Some(el) = text_cache.render_label(
        renderer,
        &vol_title,
        11.5,
        text_bright,
        Point::from((left_x + 12, py + 139)),
    ) {
        elements.push(el);
    }

    let btn_y = py + 136;
    let mute_x = px + pw - pad - 116;
    let minus_x = px + pw - pad - 62;
    let plus_x = px + pw - pad - 30;
    let mute_label = if qs.audio_muted { "Unmute" } else { "Mute" };
    if let Some(el) = text_cache.render_label_centered(
        renderer,
        mute_label,
        10.5,
        text_bright,
        Rectangle::new(Point::from((mute_x, btn_y)), Size::from((48, 22))),
    ) {
        elements.push(el);
    }
    if let Some(el) = text_cache.render_label_centered(
        renderer,
        "-",
        13.0,
        text_bright,
        Rectangle::new(Point::from((minus_x, btn_y)), Size::from((28, 22))),
    ) {
        elements.push(el);
    }
    if let Some(el) = text_cache.render_label_centered(
        renderer,
        "+",
        13.0,
        text_bright,
        Rectangle::new(Point::from((plus_x, btn_y)), Size::from((28, 22))),
    ) {
        elements.push(el);
    }

    // Quick toggle labels (2x2 grid)
    let layout_text = if qs.layout_mode == "master-stack" {
        "Layout: Tile"
    } else {
        "Layout: Float"
    };
    if let Some(el) = text_cache.render_label_centered(
        renderer,
        layout_text,
        11.0,
        text_bright,
        Rectangle::new(Point::from((left_x, py + 202)), Size::from((half_w, 32))),
    ) {
        elements.push(el);
    }
    let motion_text = if qs.reduced_motion {
        "Motion: Reduced"
    } else {
        "Motion: Smooth"
    };
    if let Some(el) = text_cache.render_label_centered(
        renderer,
        motion_text,
        11.0,
        text_bright,
        Rectangle::new(Point::from((right_x, py + 202)), Size::from((half_w, 32))),
    ) {
        elements.push(el);
    }
    if let Some(el) = text_cache.render_label_centered(
        renderer,
        "Open Spotlight",
        11.0,
        text_bright,
        Rectangle::new(Point::from((left_x, py + 242)), Size::from((half_w, 32))),
    ) {
        elements.push(el);
    }
    if let Some(el) = text_cache.render_label_centered(
        renderer,
        "Refresh Metrics",
        11.0,
        text_bright,
        Rectangle::new(Point::from((right_x, py + 242)), Size::from((half_w, 32))),
    ) {
        elements.push(el);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usable_work_area_reserves_top_and_bottom_margins() {
        let size = Size::from((1920, 1080));
        let work = usable_work_area(size);
        assert_eq!(work.x, 0);
        assert_eq!(work.y, TOP_BAR_HEIGHT);
        assert_eq!(work.width, 1920);
        let expected_h = 1080 - TOP_BAR_HEIGHT - (DOCK_HEIGHT + DOCK_MARGIN_BOTTOM * 2);
        assert_eq!(work.height, expected_h);
    }

    #[test]
    fn generate_shell_render_elements_produces_non_empty_layout() {
        let size = Size::from((1280, 800));
        let top_tel = TopBarTelemetry::default();
        let elements = generate_shell_render_elements(
            size,
            1,
            "master-stack",
            2,
            true,
            &top_tel,
            ShellOverlayView::None,
            false,
        );
        assert!(!elements.is_empty());

        let sample_items = [crate::launcher::LauncherCandidate {
            name: "Terminal".to_owned(),
            subtitle: "foot".to_owned(),
            exec: "foot".to_owned(),
            badge: ">_".to_owned(),
            is_custom_command: false,
        }];
        let spot = SpotlightRenderState {
            query: "foot",
            selected_in_window: 0,
            cursor_offset_x: 32,
            items: &sample_items,
        };
        let elements_with_launcher = generate_shell_render_elements(
            size,
            1,
            "master-stack",
            2,
            true,
            &top_tel,
            ShellOverlayView::Spotlight(&spot),
            false,
        );
        assert!(elements_with_launcher.len() > elements.len());

        let qs = QuickSettingsRenderState {
            layout_mode: "master-stack",
            reduced_motion: false,
            network_label: "Wi-Fi Connected",
            net_connected: true,
            battery_percent: 88,
            has_battery: true,
            ac_online: true,
            volume_percent: 75,
            audio_muted: false,
            frame_p50_us: 16_667,
            window_count: 2,
        };
        let elements_with_qs = generate_shell_render_elements(
            size,
            1,
            "master-stack",
            2,
            true,
            &top_tel,
            ShellOverlayView::QuickSettings(&qs),
            false,
        );
        assert!(elements_with_qs.len() > elements.len());
    }

    #[test]
    fn top_bar_and_quick_settings_hit_testing_work_accurately() {
        let size = Size::from((1280, 800));
        assert!(check_top_bar_status_click(size.w, 1200.0, 14.0));
        assert!(!check_top_bar_status_click(size.w, 100.0, 14.0));
        assert!(check_top_bar_layout_click(300.0, 14.0));
        assert!(!check_top_bar_layout_click(50.0, 14.0));

        let (px, py, pw, _ph) = quick_settings_rect(size.w, size.h);
        assert_eq!(
            check_quick_settings_click(size, 100.0, 200.0),
            QuickSettingsHit::OutsidePopover
        );
        assert_eq!(
            check_quick_settings_click(size, f64::from(px + 20), f64::from(py + 20)),
            QuickSettingsHit::InsidePopover
        );
        assert_eq!(
            check_quick_settings_click(size, f64::from(px + pw - 14 - 20), f64::from(py + 145)),
            QuickSettingsHit::VolumeUp
        );
        assert_eq!(
            check_quick_settings_click(size, f64::from(px + pw - 14 - 50), f64::from(py + 145)),
            QuickSettingsHit::VolumeDown
        );
        assert_eq!(
            check_quick_settings_click(size, f64::from(px + pw - 14 - 90), f64::from(py + 145)),
            QuickSettingsHit::ToggleMute
        );
        assert_eq!(
            check_quick_settings_click(size, f64::from(px + 30), f64::from(py + 215)),
            QuickSettingsHit::ToggleLayout
        );
        assert_eq!(
            check_quick_settings_click(size, f64::from(px + 200), f64::from(py + 215)),
            QuickSettingsHit::ToggleReducedMotion
        );
        assert_eq!(
            check_quick_settings_click(size, f64::from(px + 30), f64::from(py + 255)),
            QuickSettingsHit::OpenSpotlight
        );
        assert_eq!(
            check_quick_settings_click(size, f64::from(px + 200), f64::from(py + 255)),
            QuickSettingsHit::RefreshMetrics
        );
    }

    #[test]
    fn spotlight_click_detects_rows_inside_and_outside_modal() {
        let size = Size::from((1280, 800));
        let (mx, my, mw, _mh) = spotlight_modal_rect(size.w, size.h);
        assert_eq!(
            check_spotlight_click(size, f64::from(mx + 40), f64::from(my + 95)),
            SpotlightHit::Row(0)
        );
        assert_eq!(
            check_spotlight_click(size, f64::from(mx + 40), f64::from(my + 145)),
            SpotlightHit::Row(1)
        );
        assert_eq!(
            check_spotlight_click(size, f64::from(mx + 40), f64::from(my + 30)),
            SpotlightHit::InsideModal
        );
        assert_eq!(
            check_spotlight_click(size, f64::from(mx + mw + 20), f64::from(my + 30)),
            SpotlightHit::OutsideModal
        );
    }

    #[test]
    fn workspace_click_mapping_accurately_detects_all_nine_workspaces() {
        for ws in 1u8..=9u8 {
            let x = f64::from(WS_START_X) + f64::from(i32::from(ws - 1) * WS_BTN_STRIDE) + 5.0;
            let detected = check_top_bar_workspace_click(x, 15.0);
            assert_eq!(detected, Some(ws));
        }

        // Clicks outside top bar height or beyond bounds return None
        assert_eq!(
            check_top_bar_workspace_click(15.0, f64::from(TOP_BAR_HEIGHT + 10)),
            None
        );
        assert_eq!(check_top_bar_workspace_click(2.0, 10.0), None);
    }

    #[test]
    fn ssd_hit_testing_detects_traffic_lights_and_drag_in_top_to_bottom_order() {
        let win1 = rusty_wm::WindowId(1);
        let win2 = rusty_wm::WindowId(2);
        let windows = vec![
            (win1, rusty_wm::Rect::new(100, 100, 500, 400)),
            (win2, rusty_wm::Rect::new(100, 100, 500, 400)),
        ];

        // Overlapping window win2 is topmost (last in bottom-to-top list)
        assert_eq!(
            check_window_ssd_hit(&windows, 118.0, 114.0),
            Some(SsdHit::Close(win2))
        );
        assert_eq!(
            check_window_ssd_hit(&windows, 136.0, 114.0),
            Some(SsdHit::Minimize(win2))
        );
        assert_eq!(
            check_window_ssd_hit(&windows, 154.0, 114.0),
            Some(SsdHit::Maximize(win2))
        );
        assert_eq!(
            check_window_ssd_hit(&windows, 300.0, 114.0),
            Some(SsdHit::TitlebarDrag(win2))
        );
        // Click below titlebar inside client area returns None
        assert_eq!(
            check_window_ssd_hit(&windows, 300.0, f64::from(100 + TITLEBAR_HEIGHT + 20)),
            None
        );
    }

    #[test]
    fn generate_window_ssd_elements_creates_titlebar_traffic_lights_and_borders() {
        let elements =
            generate_window_ssd_elements(rusty_wm::Rect::new(40, 40, 600, 400), true, false);
        // 4 border strips + titlebar bg + divider + 3 traffic lights + title pill = 10 elements
        assert_eq!(elements.len(), 10);
    }
}
