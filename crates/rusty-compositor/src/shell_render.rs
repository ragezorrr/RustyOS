//! Visual shell elements rendering (`TopBar`, `Dock`, Desktop background) (`RUSTYOS-012`, ADR-0007).
//!
//! Renders macOS-inspired top status bar and bottom floating dock using Smithay's
//! solid color render elements, adhering to design tokens in `DESIGN.md` (Version 2.0 "RustyOS Purple Glass").

use smithay::backend::renderer::Color32F;
use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
use smithay::backend::renderer::element::solid::SolidColorRenderElement;
use smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement;
use smithay::backend::renderer::element::texture::TextureRenderElement;
use smithay::backend::renderer::element::utils::{RelocateRenderElement, RescaleRenderElement};
use smithay::backend::renderer::element::{Id, Kind};
use smithay::backend::renderer::gles::{GlesRenderer, GlesTexture};
use smithay::backend::renderer::utils::CommitCounter;
use smithay::utils::{Point, Rectangle, Size};

smithay::backend::renderer::element::render_elements! {
    /// Unified render element type emitted across shell, SSD frames, text, and Wayland surfaces.
    pub CompositorRenderElement<=GlesRenderer>;
    /// Client Wayland surface element.
    Surface = WaylandSurfaceRenderElement<GlesRenderer>,
    /// Solid or translucent colored rectangle.
    Solid = SolidColorRenderElement,
    /// Scaled solid rectangle mapped from logical shell UI coordinates to HiDPI physical pixels.
    ScaledSolid = RescaleRenderElement<SolidColorRenderElement>,
    /// CPU-rasterized, GPU-cached buffer (text glyphs, rounded SDF corners, circles, shadows).
    Memory = MemoryRenderBufferRenderElement<GlesRenderer>,
    /// Relocated memory buffer aligned to HiDPI physical output coordinates.
    ScaledMemory = RelocateRenderElement<MemoryRenderBufferRenderElement<GlesRenderer>>,
    /// Client-supplied Wayland texture element (e.g. `xdg_toplevel_icon_v1` SHM icon).
    Texture = TextureRenderElement<GlesTexture>,
    /// Relocated Wayland texture element aligned to HiDPI physical output coordinates.
    ScaledTexture = RelocateRenderElement<TextureRenderElement<GlesTexture>>,
}

fn resolved_icon_to_element(icon: crate::icons::ResolvedIconElement) -> CompositorRenderElement {
    match icon {
        crate::icons::ResolvedIconElement::Memory(el) => CompositorRenderElement::Memory(*el),
        crate::icons::ResolvedIconElement::Texture(el) => CompositorRenderElement::Texture(el),
    }
}

/// Resize grab border thickness in logical pixels (`RUSTYOS-022`).
pub const RESIZE_GRAB_BORDER: f64 = 6.0;

/// `TopBar` height in logical pixels.
pub const TOP_BAR_HEIGHT: i32 = 26;

/// Vertical offset of `TopBar` pills/labels inside the bar (`RUSTYOS-036`).
pub const TOP_BAR_ITEM_Y: i32 = 3;

/// Height of `TopBar` pills/labels (`RUSTYOS-036`).
pub const TOP_BAR_ITEM_H: i32 = 20;

/// Server-Side Decoration (SSD) titlebar height in logical pixels (`RUSTYOS-019`).
pub const TITLEBAR_HEIGHT: i32 = 28;

/// Dock height in logical pixels (`RUSTYOS-035`: 56 px tile + 8 px padding on both sides).
pub const DOCK_HEIGHT: i32 = 72;

/// Dock bottom margin from screen edge.
pub const DOCK_MARGIN_BOTTOM: i32 = 8;

/// Dock tile edge length in logical pixels (`RUSTYOS-035`).
pub const DOCK_ITEM_SIZE: i32 = 56;

/// Gap between Dock tiles in logical pixels (`RUSTYOS-035`).
pub const DOCK_ITEM_GAP: i32 = 8;

/// Dock capsule inner padding in logical pixels (`RUSTYOS-035`).
pub const DOCK_PADDING: i32 = 8;

/// Dock capsule corner radius in logical pixels (`DESIGN.md` §12: `DOCK_HEIGHT / 2 = 36`).
pub const DOCK_RADIUS: i32 = 36;

/// Extra horizontal spacing allocated for the vertical divider between pinned apps and running windows (`RUSTYOS-048`).
pub const DOCK_DIVIDER_GAP: i32 = 12;

/// App icon edge length inside a Dock tile (`RUSTYOS-035`).
pub const DOCK_ICON_SIZE: i32 = 48;

/// SSD window frame & glass popover corner radius in logical pixels (`DESIGN.md` §12.3).
pub const WINDOW_RADIUS: i32 = 20;

/// `TopBar` Left Menu metrics (macOS 27 Active App & Global Menus) (`RUSTYOS-038`).
pub const TOP_BAR_LOGO_X: i32 = 12;
pub const TOP_BAR_LOGO_W: i32 = 18;
pub const TOP_BAR_APP_X: i32 = 34;
pub const TOP_BAR_APP_MAX_W: i32 = 78;
pub const TOP_BAR_MENUS_START_X: i32 = 118;
pub const TOP_BAR_MENU_FILE_W: i32 = 28;
pub const TOP_BAR_MENU_EDIT_W: i32 = 28;
pub const TOP_BAR_MENU_VIEW_W: i32 = 32;
pub const TOP_BAR_MENU_WINDOW_W: i32 = 50;
pub const TOP_BAR_MENUS_GAP: i32 = 10;
pub const TOP_BAR_SEPARATOR_X: i32 = 296;

/// Workspace button width, stride, and start position for compact indicators (`RUSTYOS-038`).
pub const WS_BTN_WIDTH: i32 = 18;
pub const WS_BTN_GAP: i32 = 3;
pub const WS_BTN_STRIDE: i32 = WS_BTN_WIDTH + WS_BTN_GAP;
pub const WS_START_X: i32 = 308;

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

/// Live hardware/session telemetry displayed inside `TopBar` (`RUSTYOS-021`, `RUSTYOS-048`).
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct TopBarTelemetry<'a> {
    /// Local wall-clock date/time label shown in `TopBar` (`HH:MM` or `"Thu 12 Nov  14:32"`).
    pub clock_hhmm: &'a str,
    /// Battery percentage (`0..=100`).
    pub battery_percent: u8,
    /// Whether a hardware battery is present.
    pub has_battery: bool,
    /// Whether an active network interface is up.
    pub net_connected: bool,
    /// Master output volume (`0..=100`).
    pub volume_percent: u8,
    /// Master audio output is muted.
    pub audio_muted: bool,
    /// Whether the Quick Settings popover is currently open.
    pub quick_settings_open: bool,
    /// Number of active/unread desktop notifications (`RUSTYOS-024`).
    pub notification_count: u32,
    /// Active keyboard layout badge code (`"EN"`, `"RU"`, etc.) (`RUSTYOS-029`).
    pub kbd_layout: &'a str,
    /// Registered `StatusNotifierItem` tray badges rendered left of the keyboard indicator (`RUSTYOS-032`).
    pub tray_items: &'a [TopBarTrayItem<'a>],
    /// Currently open `TopBar` left menu dropdown if any (`RUSTYOS-048`).
    pub active_menu: Option<TopBarMenuHit>,
    /// Global shell glass opacity multiplier (`0.35..=1.0`, default `0.78`) (`RUSTYOS-048`).
    pub glass_opacity: f32,
    /// High-contrast opaque shell panels mode (`RUSTYOS-048`).
    pub reduce_transparency: bool,
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
            notification_count: 0,
            kbd_layout: "EN",
            tray_items: &[],
            active_menu: None,
            glass_opacity: crate::config::DEFAULT_GLASS_OPACITY,
            reduce_transparency: false,
        }
    }
}

/// Active notification toast view rendered in the top-right Notification HUD (`RUSTYOS-024`).
#[derive(Debug, Clone)]
pub struct NotificationToastView<'a> {
    /// Unique notification identifier.
    pub id: u64,
    /// Urgency level controlling accent color.
    pub urgency: rusty_ipc::Urgency,
    /// Short notification title.
    pub title: &'a str,
    /// Notification body text.
    pub body: &'a str,
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
    /// Whether a Bluetooth adapter is available (`RUSTYOS-027`).
    pub bt_available: bool,
    /// Whether Bluetooth adapter power is enabled (`RUSTYOS-027`).
    pub bt_powered: bool,
    /// Concise Bluetooth status label (`RUSTYOS-027`).
    pub bt_label: &'a str,
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
    /// Animated reveal progress (`0.0` closed -> `1.0` open) (`RUSTYOS-025`).
    pub progress: f64,
    /// Global shell glass opacity scalar (`0.35..=1.0`) (`RUSTYOS-048`).
    pub glass_opacity: f32,
    /// High-contrast opaque shell panels mode (`RUSTYOS-048`).
    pub reduce_transparency: bool,
}

/// Metadata for a single managed window entry rendered in Dock and Workspace Overview (`RUSTYOS-023`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockWindowEntry {
    /// Window manager identifier.
    pub id: rusty_wm::WindowId,
    /// Human-readable title displayed in Overview cards.
    pub title: String,
    /// Compact 1-2 character monogram displayed inside the Dock tile.
    pub badge: String,
    /// Optional resolved XDG icon name or PNG path (`RUSTYOS-033`).
    pub icon_name: Option<String>,
    /// Optional client-provided `xdg_toplevel_icon_v1` icon (`RUSTYOS-047`).
    pub toplevel_icon: Option<crate::icons::ShmToplevelIcon>,
    /// Dimensions label (e.g. `"800x600"`).
    pub dimensions: String,
    /// Whether this window is currently focused.
    pub focused: bool,
    /// Whether this window is currently minimized to the Dock (`RUSTYOS-032`).
    pub minimized: bool,
}

/// Computes a concise 2-character badge from a window title (`RUSTYOS-023`).
#[must_use]
pub fn make_window_badge(title: &str) -> String {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return "Wn".to_owned();
    }
    let words: Vec<&str> = trimmed
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    if words.len() >= 2 {
        let mut badge = String::new();
        if let Some(c1) = words[0].chars().next() {
            badge.push(c1.to_ascii_uppercase());
        }
        if let Some(c2) = words[1].chars().next() {
            badge.push(c2.to_ascii_uppercase());
        }
        if !badge.is_empty() {
            return badge;
        }
    }
    trimmed.chars().take(2).collect()
}

/// Extracts a clean application name from a window title (`RUSTYOS-038`).
#[must_use]
pub fn extract_app_name(title: &str) -> String {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return "RustyOS".to_owned();
    }
    // Check for standard XDG window title delimiters " — " or " - "
    if let Some((_doc, app)) = trimmed.rsplit_once(" — ") {
        let app_trimmed = app.trim();
        if !app_trimmed.is_empty() {
            return truncate_ascii(app_trimmed, 14);
        }
    }
    if let Some((_doc, app)) = trimmed.rsplit_once(" - ") {
        let app_trimmed = app.trim();
        if !app_trimmed.is_empty() {
            return truncate_ascii(app_trimmed, 14);
        }
    }
    truncate_ascii(trimmed, 14)
}

/// Interactive click target inside the `TopBar` Left Menu area (`RUSTYOS-038`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopBarMenuHit {
    /// System emblem (`UiGlyphKind::RustyLogo`).
    SystemLogo,
    /// Active application name menu.
    ActiveApp,
    /// Global File menu.
    File,
    /// Global Edit menu.
    Edit,
    /// Global View menu.
    View,
    /// Global Window menu.
    Window,
}

/// Action triggered from an open `TopBar` menu dropdown popover (`RUSTYOS-048`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopBarMenuAction {
    /// Show concise RustyOS notification card.
    AboutRustyOs,
    /// Toggle Workspace Overview.
    OpenOverview,
    /// Toggle Control Center (`QuickSettings`).
    OpenControlCenter,
    /// Toggle Spotlight Search.
    OpenSpotlight,
    /// Launch preferred terminal emulator.
    NewTerminal,
    /// Launch preferred file manager.
    OpenFiles,
    /// Switch between Floating and Tiling layout modes.
    ToggleLayout,
    /// Toggle `reduced_motion`.
    ToggleReducedMotion,
    /// Toggle `reduce_transparency`.
    ToggleReduceTransparency,
    /// Request non-blocking telemetry refresh.
    RefreshMetrics,
    /// Engage session Lock Screen.
    LockSession,
    /// Cycle focus to next window.
    FocusNextWindow,
    /// Maximize / restore focused window.
    ToggleMaximizeFocused,
    /// Minimize focused window.
    MinimizeFocused,
    /// Close focused window.
    CloseFocused,
}

/// Hit-test outcome inside an open `TopBar` menu dropdown popover (`RUSTYOS-048`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopBarMenuPopoverClick {
    /// Specific menu action row clicked.
    Action(TopBarMenuAction),
    /// Click inside popover frame (absorb).
    Inside,
    /// Click outside popover frame (dismiss).
    Outside,
}

/// Dock view state describing open window tiles, hovered slot, and overview toggle highlight (`RUSTYOS-023`).
#[derive(Debug, Clone, Copy)]
pub struct DockViewState<'a> {
    /// Optional XDG icon names for pinned `[Terminal, Files]` Dock slots (`RUSTYOS-033`).
    pub pinned_icons: [Option<&'a str>; 2],
    /// Open visible windows on the active workspace in stable slot order.
    pub windows: &'a [DockWindowEntry],
    /// Currently hovered Dock slot index (`0..3 + windows.len()`), if any.
    pub hover_slot: Option<usize>,
    /// Whether Workspace Overview is active (highlights Dock Overview squircle tile).
    pub overview_active: bool,
    /// Smooth 120 ms hover lift factor (`0.0..=1.0`) (`RUSTYOS-048`).
    pub hover_progress: f64,
    /// Prefer instant subtle lift without animated spring (`RUSTYOS-048`).
    pub reduced_motion: bool,
}

/// Interactive click target inside the bottom floating Dock (`RUSTYOS-023`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockHit {
    /// Pinned Terminal launcher (`>_`).
    PinnedTerminal,
    /// Pinned Files launcher (`Fs`).
    PinnedFiles,
    /// Workspace Overview toggle button (`Ov`).
    ToggleOverview,
    /// Running window tile at `index` inside `ordered_visible_windows`.
    WindowSlot(usize),
}

/// Live data for the Workspace Overview / Expose card grid (`RUSTYOS-023`).
#[derive(Debug, Clone)]
pub struct OverviewRenderState<'a> {
    /// Active workspace number (`1..=9`).
    pub active_workspace: u8,
    /// Active layout mode string (`"floating"` or `"master-stack"`).
    pub layout_mode: &'a str,
    /// Open visible windows on the active workspace in stable order.
    pub windows: &'a [DockWindowEntry],
    /// Animated reveal progress (`0.0` closed -> `1.0` open) (`RUSTYOS-025`).
    pub progress: f64,
}

/// Live data for the interactive Lock Screen curtain & card (`RUSTYOS-025`).
#[derive(Debug, Clone)]
pub struct LockScreenRenderState<'a> {
    /// Local wall-clock time (`HH:MM`).
    pub clock_hhmm: &'a str,
    /// Network status label.
    pub network_label: &'a str,
    /// Battery charge percentage (`0..=100`).
    pub battery_percent: u8,
    /// Whether hardware battery is present.
    pub has_battery: bool,
    /// Number of user windows paused behind the lock curtain.
    pub window_count: usize,
    /// Animated reveal progress (`0.0` unlocked -> `1.0` locked).
    pub progress: f64,
}

/// Interactive click target while Workspace Overview is open (`RUSTYOS-023`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverviewHit {
    /// Focus and raise the window at card index `usize`.
    Card(usize),
    /// Dismiss Overview without changing window focus.
    Dismiss,
}

/// Single window card inside the `Alt+Tab` window switcher HUD (`RUSTYOS-033`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowSwitcherCard<'a> {
    /// Window title displayed at card bottom.
    pub title: &'a str,
    /// Fallback 1-2 char monogram if no PNG icon resolves.
    pub badge: &'a str,
    /// Managed window identifier used as the GPU SHM icon cache key (`RUSTYOS-047`).
    pub window_id: rusty_wm::WindowId,
    /// Optional resolved XDG icon name / PNG path.
    pub icon_name: Option<&'a str>,
    /// Optional client-provided `xdg_toplevel_icon_v1` icon (`RUSTYOS-047`).
    pub toplevel_icon: Option<&'a crate::icons::ShmToplevelIcon>,
    /// True if minimized to the Dock.
    pub minimized: bool,
}

/// Active `Alt+Tab` window switcher HUD render state (`RUSTYOS-033`).
#[derive(Debug, Clone, Copy)]
pub struct WindowSwitcherRenderState<'a> {
    /// Candidate window cards in presentation order.
    pub cards: &'a [WindowSwitcherCard<'a>],
    /// Currently highlighted card index.
    pub selected: usize,
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
    /// Workspace Overview / Expose window grid (`RUSTYOS-023`).
    Overview(&'a OverviewRenderState<'a>),
    /// Interactive `Alt+Tab` window switcher HUD (`RUSTYOS-033`).
    Switcher(&'a WindowSwitcherRenderState<'a>),
}

/// Active transient On-Screen Display (OSD) indicator pill (`RUSTYOS-031`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OsdRenderState<'a> {
    /// Primary label (e.g., `"Volume"`, `"Brightness"`, `"Play / Pause"`, `"Screenshot"`).
    pub label: &'a str,
    /// Optional numeric percentage (`0..=100`) rendered as a progress bar.
    pub value_percent: Option<u8>,
    /// True when audio is muted (grays out the bar fill).
    pub muted: bool,
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
    /// Directly set volume percentage (`0..=100`) along the Sound capsule slider (`RUSTYOS-048`).
    SetVolumePercent(u8),
    /// Toggle `reduce_transparency` high-contrast mode (`RUSTYOS-048`).
    ToggleReduceTransparency,
    /// Directly set shell glass opacity (`35..=100` percent) along the Glass slider (`RUSTYOS-048`).
    SetGlassOpacityPercent(u8),
    /// Toggle workspace layout (`Floating <-> MasterStack`).
    ToggleLayout,
    /// Toggle accessibility `reduced_motion`.
    ToggleReducedMotion,
    /// Switch to Spotlight application launcher.
    OpenSpotlight,
    /// Trigger immediate background telemetry refresh.
    RefreshMetrics,
    /// Toggle Bluetooth adapter power via BlueZ (`RUSTYOS-027`).
    ToggleBluetooth,
    /// Engage desktop Lock Screen from Quick Settings (`RUSTYOS-027`).
    LockSession,
    /// Click inside popover frame (absorb event, keep open).
    InsidePopover,
    /// Click outside popover (dismiss Quick Settings).
    OutsidePopover,
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
fn anim_offset(progress: f64, max_offset: f64) -> i32 {
    ((1.0 - progress.clamp(0.0, 1.0)) * max_offset).round() as i32
}

fn lock_screen_card_rect(screen_size: Size<i32, smithay::utils::Physical>) -> (i32, i32, i32, i32) {
    let w = 360.min((screen_size.w - 40).max(260));
    let h = 216.min((screen_size.h - 60).max(160));
    let x = (screen_size.w - w) / 2;
    let y = (screen_size.h - h) / 2;
    (x, y, w, h)
}

fn lock_screen_unlock_button_rect(
    screen_size: Size<i32, smithay::utils::Physical>,
    card_dy: i32,
) -> Rectangle<i32, smithay::utils::Physical> {
    let (cx, cy, cw, ch) = lock_screen_card_rect(screen_size);
    let btn_w = 188.min(cw - 48);
    let btn_h = 36;
    let btn_x = cx + (cw - btn_w) / 2;
    let btn_y = cy + card_dy + ch - 68;
    Rectangle::new(Point::from((btn_x, btn_y)), Size::from((btn_w, btn_h)))
}

/// Hit-tests `(x, y)` against the interactive Lock Screen `"Unlock Session"` pill button (`RUSTYOS-025`).
#[must_use]
pub fn check_lock_screen_unlock_click(
    screen_size: Size<i32, smithay::utils::Physical>,
    x: f64,
    y: f64,
) -> bool {
    let btn = lock_screen_unlock_button_rect(screen_size, 0);
    let bx = f64::from(btn.loc.x);
    let by = f64::from(btn.loc.y);
    let bw = f64::from(btn.size.w);
    let bh = f64::from(btn.size.h);
    x >= bx && x < bx + bw && y >= by && y < by + bh
}

/// Dynamic left-side geometry for `TopBar` app name, global menus, workspace strip, and layout badge (`RUSTYOS-048`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TopBarLeftLayout {
    pub app_x: i32,
    pub app_w: i32,
    pub file_x: i32,
    pub edit_x: i32,
    pub view_x: i32,
    pub win_x: i32,
    pub separator_x: i32,
    pub ws_start_x: i32,
    pub layout_x: i32,
    pub layout_w: i32,
}

/// Computes `TopBar` left-side x coordinates, expanding cleanly when the active app name is wide (`RUSTYOS-048`).
#[must_use]
pub fn compute_top_bar_left_layout(active_app_name: &str) -> TopBarLeftLayout {
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let chars = active_app_name.chars().count() as i32;
    let app_w = (chars * 7 + 14).clamp(TOP_BAR_APP_MAX_W, 138);
    let delta = app_w - TOP_BAR_APP_MAX_W;
    let file_x = TOP_BAR_MENUS_START_X + delta;
    let edit_x = file_x + TOP_BAR_MENU_FILE_W + TOP_BAR_MENUS_GAP;
    let view_x = edit_x + TOP_BAR_MENU_EDIT_W + TOP_BAR_MENUS_GAP;
    let win_x = view_x + TOP_BAR_MENU_VIEW_W + TOP_BAR_MENUS_GAP;
    let separator_x = TOP_BAR_SEPARATOR_X + delta;
    let ws_start_x = WS_START_X + delta;
    let layout_x = ws_start_x + 9 * WS_BTN_STRIDE + 8;
    TopBarLeftLayout {
        app_x: TOP_BAR_APP_X,
        app_w,
        file_x,
        edit_x,
        view_x,
        win_x,
        separator_x,
        ws_start_x,
        layout_x,
        layout_w: 52,
    }
}

/// Dynamic right-side geometry for `TopBar` clock, Control Center, glyphs, Search, Kbd, Notifications, and Tray (`RUSTYOS-048`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TopBarRightLayout {
    pub search_x: i32,
    pub search_w: i32,
    pub wifi_x: i32,
    pub volume_x: i32,
    pub battery_x: Option<i32>,
    pub control_center_x: i32,
    pub control_center_w: i32,
    pub clock_x: i32,
    pub clock_w: i32,
    pub status_pill_x: i32,
    pub status_pill_w: i32,
    pub notif_x: i32,
    pub notif_w: i32,
    pub kbd_x: i32,
    pub kbd_w: i32,
}

/// Computes compact right-aligned `TopBar` positions collapsing the battery gap when `has_battery == false` (`RUSTYOS-048`).
#[must_use]
pub fn compute_top_bar_right_layout(
    screen_width: i32,
    has_battery: bool,
    notification_count: u32,
    clock_text: &str,
) -> TopBarRightLayout {
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let clock_chars = clock_text.chars().count() as i32;
    let clock_w = (clock_chars * 6 + 12).clamp(44, 122);
    let right_edge = screen_width - 10;
    let clock_x = (right_edge - clock_w).max(screen_width - 134);

    let control_center_w = 20;
    let control_center_x = clock_x - 6 - control_center_w;

    let (battery_x, next_left) = if has_battery {
        let bx = control_center_x - 6 - 20;
        (Some(bx), bx)
    } else {
        (None, control_center_x)
    };

    let volume_x = next_left - 6 - 16;
    let wifi_x = volume_x - 6 - 16;
    let search_w = 18;
    let search_x = wifi_x - 10 - search_w;

    let status_pill_x = wifi_x - 4;
    let status_pill_w = (right_edge - status_pill_x).max(96);

    let (notif_x, notif_w, kbd_anchor) = if notification_count > 0 {
        let nw = 38;
        let nx = search_x - 8 - nw;
        (nx, nw, nx)
    } else {
        (search_x, 0, search_x)
    };

    let kbd_w = 30;
    let kbd_x = kbd_anchor - 8 - kbd_w;

    TopBarRightLayout {
        search_x,
        search_w,
        wifi_x,
        volume_x,
        battery_x,
        control_center_x,
        control_center_w,
        clock_x,
        clock_w,
        status_pill_x,
        status_pill_w,
        notif_x,
        notif_w,
        kbd_x,
        kbd_w,
    }
}

fn status_pill_rect(screen_width: i32) -> (i32, i32, i32, i32) {
    let layout = compute_top_bar_right_layout(screen_width, false, 0, "12:00");
    (
        layout.status_pill_x,
        TOP_BAR_ITEM_Y,
        layout.status_pill_w,
        TOP_BAR_ITEM_H,
    )
}

#[cfg(test)]
fn notification_badge_rect(screen_width: i32) -> (i32, i32, i32, i32) {
    let layout = compute_top_bar_right_layout(screen_width, false, 1, "12:00");
    (
        layout.notif_x,
        TOP_BAR_ITEM_Y,
        layout.notif_w,
        TOP_BAR_ITEM_H,
    )
}

#[cfg(test)]
fn kbd_badge_rect(screen_width: i32, notification_count: u32) -> (i32, i32, i32, i32) {
    let layout = compute_top_bar_right_layout(screen_width, false, notification_count, "12:00");
    (layout.kbd_x, TOP_BAR_ITEM_Y, layout.kbd_w, TOP_BAR_ITEM_H)
}

/// Maximum number of system tray applets displayed simultaneously in `TopBar` (`RUSTYOS-032`).
pub const MAX_TOP_BAR_TRAY_ITEMS: usize = 6;

/// Computes `TopBar` tray pill rectangles positioned immediately left of the keyboard indicator (`RUSTYOS-032`).
#[must_use]
pub fn tray_slot_rects(
    screen_width: i32,
    notification_count: u32,
    tray_count: usize,
) -> Vec<(i32, i32, i32, i32)> {
    tray_slot_rects_with_layout(screen_width, false, notification_count, "12:00", tray_count)
}

/// Computes `TopBar` tray pill rectangles using exact telemetry-driven right-cluster layout (`RUSTYOS-048`).
#[must_use]
pub fn tray_slot_rects_with_layout(
    screen_width: i32,
    has_battery: bool,
    notification_count: u32,
    clock_text: &str,
    tray_count: usize,
) -> Vec<(i32, i32, i32, i32)> {
    if tray_count == 0 {
        return Vec::new();
    }
    let min_left_x = WS_START_X + 9 * WS_BTN_STRIDE + 8 + 56 + 10;
    let right =
        compute_top_bar_right_layout(screen_width, has_battery, notification_count, clock_text);
    let kbd_x = right.kbd_x;
    let avail = (kbd_x - 8 - min_left_x).max(0);
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
    let first_x = kbd_x - 8 - (count_i32 * stride - gap);
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

/// Maximum number of floating notification toasts shown simultaneously (`RUSTYOS-024`).
pub const MAX_VISIBLE_NOTIFICATIONS: usize = 4;

/// Computes top-right stacked rectangles for up to [`MAX_VISIBLE_NOTIFICATIONS`] active toasts (`RUSTYOS-024`).
#[must_use]
pub fn notification_toast_rects(
    screen_size: Size<i32, smithay::utils::Physical>,
    count: usize,
) -> Vec<Rectangle<i32, smithay::utils::Physical>> {
    let visible = count.min(MAX_VISIBLE_NOTIFICATIONS);
    if visible == 0 {
        return Vec::new();
    }
    let width = screen_size.w.max(320);
    let toast_w = 320.min((width - 32).max(200));
    let toast_h = 68;
    let gap = 10;
    let toast_x = width - toast_w - 16;
    let start_y = TOP_BAR_HEIGHT + 12;

    let mut rects = Vec::with_capacity(visible);
    for idx in 0..visible {
        let idx_i32 = i32::try_from(idx).unwrap_or(0);
        let y = start_y + idx_i32 * (toast_h + gap);
        rects.push(Rectangle::new(
            Point::from((toast_x, y)),
            Size::from((toast_w, toast_h)),
        ));
    }
    rects
}

/// Hit-tests `(x, y)` against visible top-right Notification HUD toasts (`RUSTYOS-024`).
#[must_use]
pub fn check_notification_click(
    screen_size: Size<i32, smithay::utils::Physical>,
    visible_count: usize,
    x: f64,
    y: f64,
) -> Option<usize> {
    let rects = notification_toast_rects(screen_size, visible_count);
    for (idx, rect) in rects.into_iter().enumerate() {
        let rx = f64::from(rect.loc.x);
        let ry = f64::from(rect.loc.y);
        let rw = f64::from(rect.size.w);
        let rh = f64::from(rect.size.h);
        if x >= rx && x < rx + rw && y >= ry && y < ry + rh {
            return Some(idx);
        }
    }
    None
}

/// Checks if `(x, y)` clicked the `TopBar` unread notification badge pill (`RUSTYOS-024`).
#[must_use]
pub fn check_top_bar_notification_click(
    screen_width: i32,
    notification_count: u32,
    x: f64,
    y: f64,
) -> bool {
    check_top_bar_notification_click_for_telemetry(
        screen_width,
        false,
        notification_count,
        "12:00",
        x,
        y,
    )
}

/// Telemetry-aware hit-test for the `TopBar` unread notification badge (`RUSTYOS-048`).
#[must_use]
pub fn check_top_bar_notification_click_for_telemetry(
    screen_width: i32,
    has_battery: bool,
    notification_count: u32,
    clock_text: &str,
    x: f64,
    y: f64,
) -> bool {
    if notification_count == 0 || y < 0.0 || y > f64::from(TOP_BAR_HEIGHT) {
        return false;
    }
    let layout =
        compute_top_bar_right_layout(screen_width, has_battery, notification_count, clock_text);
    x >= f64::from(layout.notif_x) && x < f64::from(layout.notif_x + layout.notif_w)
}

/// Checks if `(x, y)` clicked the `TopBar` keyboard layout pill (`"EN"` / `"RU"`) (`RUSTYOS-029`).
#[must_use]
pub fn check_top_bar_kbd_click(screen_width: i32, notification_count: u32, x: f64, y: f64) -> bool {
    check_top_bar_kbd_click_for_telemetry(screen_width, false, notification_count, "12:00", x, y)
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
    if y < 0.0 || y > f64::from(TOP_BAR_HEIGHT) {
        return false;
    }
    let layout =
        compute_top_bar_right_layout(screen_width, has_battery, notification_count, clock_text);
    x >= f64::from(layout.kbd_x) && x < f64::from(layout.kbd_x + layout.kbd_w)
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
    check_top_bar_tray_click_for_telemetry(
        screen_width,
        false,
        notification_count,
        "12:00",
        tray_count,
        x,
        y,
    )
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
    if y < 0.0 || y > f64::from(TOP_BAR_HEIGHT) {
        return None;
    }
    for (idx, (bx, _by, bw, _bh)) in tray_slot_rects_with_layout(
        screen_width,
        has_battery,
        notification_count,
        clock_text,
        tray_count,
    )
    .into_iter()
    .enumerate()
    {
        if x >= f64::from(bx) && x < f64::from(bx + bw) {
            return Some(idx);
        }
    }
    None
}

fn quick_settings_rect(width: i32, height: i32) -> (i32, i32, i32, i32) {
    let pop_w = 320.min(width - 24).max(240);
    let pop_h = 392.min(height - TOP_BAR_HEIGHT - 24).max(220);
    let pop_x = width - pop_w - 12;
    let pop_y = TOP_BAR_HEIGHT + 8;
    (pop_x, pop_y, pop_w, pop_h)
}

#[must_use]
fn dock_layout_bounds(width: i32, height: i32, window_count: usize) -> (i32, i32, i32, usize) {
    let items_count = (3 + window_count).max(3);
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let count_i32 = items_count as i32;
    let divider_extra = i32::from(window_count > 0) * DOCK_DIVIDER_GAP;
    let raw_width = count_i32 * DOCK_ITEM_SIZE
        + (count_i32 - 1) * DOCK_ITEM_GAP
        + DOCK_PADDING * 2
        + divider_extra;
    let dock_width = raw_width.min((width - 48).max(160));
    let dock_x = (width - dock_width) / 2;
    let dock_y = height - DOCK_HEIGHT - DOCK_MARGIN_BOTTOM;
    (dock_x, dock_y, dock_width, items_count)
}

#[must_use]
fn dock_slot_rect(
    width: i32,
    height: i32,
    window_count: usize,
    slot_idx: usize,
) -> Option<Rectangle<i32, smithay::utils::Physical>> {
    let (dock_x, dock_y, dock_width, items_count) = dock_layout_bounds(width, height, window_count);
    if slot_idx >= items_count {
        return None;
    }
    let item_size = DOCK_ITEM_SIZE;
    let slot_i32 = i32::try_from(slot_idx).ok()?;
    let divider_shift = if window_count > 0 && slot_idx >= 3 {
        DOCK_DIVIDER_GAP
    } else {
        0
    };
    let item_x = dock_x + DOCK_PADDING + slot_i32 * (item_size + DOCK_ITEM_GAP) + divider_shift;
    if item_x + item_size > dock_x + dock_width {
        return None;
    }
    Some(Rectangle::new(
        Point::from((item_x, dock_y + DOCK_PADDING)),
        Size::from((item_size, item_size)),
    ))
}

/// Hit-tests `(x, y)` against Dock slots and returns the hovered/clicked slot index (`RUSTYOS-023`).
#[must_use]
pub fn check_dock_slot(
    screen_size: Size<i32, smithay::utils::Physical>,
    window_count: usize,
    x: f64,
    y: f64,
) -> Option<usize> {
    let items_count = (3 + window_count).max(3);
    for idx in 0..items_count {
        if let Some(rect) = dock_slot_rect(screen_size.w, screen_size.h, window_count, idx) {
            let rx = f64::from(rect.loc.x);
            let ry = f64::from(rect.loc.y);
            let rw = f64::from(rect.size.w);
            let rh = f64::from(rect.size.h);
            if x >= rx && x < rx + rw && y >= ry && y < ry + rh {
                return Some(idx);
            }
        }
    }
    None
}

/// Hit-tests a pointer click at `(x, y)` against the bottom Dock (`RUSTYOS-023`).
#[must_use]
pub fn check_dock_click(
    screen_size: Size<i32, smithay::utils::Physical>,
    window_count: usize,
    x: f64,
    y: f64,
) -> Option<DockHit> {
    let slot = check_dock_slot(screen_size, window_count, x, y)?;
    match slot {
        0 => Some(DockHit::PinnedTerminal),
        1 => Some(DockHit::PinnedFiles),
        2 => Some(DockHit::ToggleOverview),
        n => {
            let win_idx = n - 3;
            (win_idx < window_count).then_some(DockHit::WindowSlot(win_idx))
        }
    }
}

/// Computes centered Workspace Overview preview card rectangles for `count` windows (`RUSTYOS-023`).
#[must_use]
pub fn overview_card_rects(
    screen_size: Size<i32, smithay::utils::Physical>,
    count: usize,
) -> Vec<Rectangle<i32, smithay::utils::Physical>> {
    if count == 0 {
        return Vec::new();
    }
    let width = screen_size.w.max(320);
    let height = screen_size.h.max(240);
    let area_top = TOP_BAR_HEIGHT + 70;
    let area_bottom = (height - DOCK_HEIGHT - DOCK_MARGIN_BOTTOM - 22).max(area_top + 120);
    let avail_w = (width - 64).max(200);
    let avail_h = (area_bottom - area_top).max(120);

    let cols = match count {
        1 => 1,
        2..=4 => 2,
        _ => 3,
    };
    let rows = count.div_ceil(cols);
    let gap = 18;
    let cols_i32 = i32::try_from(cols).unwrap_or(1);
    let rows_i32 = i32::try_from(rows).unwrap_or(1);

    let card_w = ((avail_w - gap * (cols_i32 - 1)) / cols_i32).clamp(140, 340);
    let card_h = ((avail_h - gap * (rows_i32 - 1)) / rows_i32).clamp(96, 210);

    let total_grid_h = rows_i32 * card_h + (rows_i32 - 1) * gap;
    let start_y = area_top + ((avail_h - total_grid_h) / 2).max(0);

    let mut rects = Vec::with_capacity(count);
    for idx in 0..count {
        let row = idx / cols;
        let col = idx % cols;
        let items_in_row = if row + 1 == rows {
            count - row * cols
        } else {
            cols
        };
        let row_cols_i32 = i32::try_from(items_in_row).unwrap_or(1);
        let row_w = row_cols_i32 * card_w + (row_cols_i32 - 1) * gap;
        let row_start_x = (width - row_w) / 2;

        let cx = row_start_x + i32::try_from(col).unwrap_or(0) * (card_w + gap);
        let cy = start_y + i32::try_from(row).unwrap_or(0) * (card_h + gap);
        rects.push(Rectangle::new(
            Point::from((cx, cy)),
            Size::from((card_w, card_h)),
        ));
    }
    rects
}

/// Hit-tests `(x, y)` inside Workspace Overview (`RUSTYOS-023`).
#[must_use]
pub fn check_overview_click(
    screen_size: Size<i32, smithay::utils::Physical>,
    window_count: usize,
    x: f64,
    y: f64,
) -> OverviewHit {
    let cards = overview_card_rects(screen_size, window_count);
    for (idx, rect) in cards.into_iter().enumerate() {
        let rx = f64::from(rect.loc.x);
        let ry = f64::from(rect.loc.y);
        let rw = f64::from(rect.size.w);
        let rh = f64::from(rect.size.h);
        if x >= rx && x < rx + rw && y >= ry && y < ry + rh {
            return OverviewHit::Card(idx);
        }
    }
    OverviewHit::Dismiss
}

/// Computes `(panel_rect, card_rects)` for the centered `Alt+Tab` window switcher HUD (`RUSTYOS-033`).
#[must_use]
pub fn switcher_layout(
    screen_size: Size<i32, smithay::utils::Physical>,
    count: usize,
) -> (
    Rectangle<i32, smithay::utils::Physical>,
    Vec<Rectangle<i32, smithay::utils::Physical>>,
) {
    let width = screen_size.w.max(320);
    let height = screen_size.h.max(240);
    if count == 0 {
        let rect = Rectangle::new(Point::from((width / 2, height / 2)), Size::from((0, 0)));
        return (rect, Vec::new());
    }

    let pad = 16;
    let gap = 12;
    let count_i32 = i32::try_from(count).unwrap_or(1);
    let max_panel_w = (width - 48).max(200);
    let raw_card_w = (max_panel_w - pad * 2 - gap * (count_i32 - 1)) / count_i32;
    let card_w = raw_card_w.clamp(76, 128);
    let card_h = 104;
    let panel_w = (pad * 2 + count_i32 * card_w + (count_i32 - 1) * gap).min(max_panel_w);
    let panel_h = card_h + pad * 2;
    let panel_x = (width - panel_w) / 2;
    let panel_y = ((height - panel_h) / 2).max(TOP_BAR_HEIGHT + 16);

    let mut cards = Vec::with_capacity(count);
    for idx in 0..count {
        let i = i32::try_from(idx).unwrap_or(0);
        let cx = panel_x + pad + i * (card_w + gap);
        if cx + card_w > panel_x + panel_w - pad + 2 {
            break;
        }
        cards.push(Rectangle::new(
            Point::from((cx, panel_y + pad)),
            Size::from((card_w, card_h)),
        ));
    }

    (
        Rectangle::new(
            Point::from((panel_x, panel_y)),
            Size::from((panel_w, panel_h)),
        ),
        cards,
    )
}

/// Hit-tests `(x, y)` against visible `Alt+Tab` window switcher cards (`RUSTYOS-033`).
#[must_use]
pub fn check_switcher_click(
    screen_size: Size<i32, smithay::utils::Physical>,
    count: usize,
    x: f64,
    y: f64,
) -> Option<usize> {
    let (_panel, cards) = switcher_layout(screen_size, count);
    for (idx, rect) in cards.into_iter().enumerate() {
        let rx = f64::from(rect.loc.x);
        let ry = f64::from(rect.loc.y);
        let rw = f64::from(rect.size.w);
        let rh = f64::from(rect.size.h);
        if x >= rx && x < rx + rw && y >= ry && y < ry + rh {
            return Some(idx);
        }
    }
    None
}

/// Checks if `(x, y)` clicked any item in the `TopBar` Left Menu area (`RUSTYOS-038`).
/// Returns menu dropdown actions for an open `TopBar` left menu item (`RUSTYOS-048`).
#[must_use]
pub const fn top_bar_menu_items(
    menu: TopBarMenuHit,
) -> &'static [(&'static str, TopBarMenuAction)] {
    match menu {
        TopBarMenuHit::SystemLogo => &[
            ("About RustyOS", TopBarMenuAction::AboutRustyOs),
            ("Workspace Overview", TopBarMenuAction::OpenOverview),
            ("Control Center", TopBarMenuAction::OpenControlCenter),
            ("Lock Screen", TopBarMenuAction::LockSession),
        ],
        TopBarMenuHit::ActiveApp => &[
            ("New Terminal", TopBarMenuAction::NewTerminal),
            ("Open Files", TopBarMenuAction::OpenFiles),
            ("Control Center", TopBarMenuAction::OpenControlCenter),
        ],
        TopBarMenuHit::File => &[
            ("New Terminal", TopBarMenuAction::NewTerminal),
            ("Open Files", TopBarMenuAction::OpenFiles),
            ("Spotlight Search", TopBarMenuAction::OpenSpotlight),
        ],
        TopBarMenuHit::Edit => &[
            ("Toggle Float / Tile", TopBarMenuAction::ToggleLayout),
            (
                "Toggle Reduced Motion",
                TopBarMenuAction::ToggleReducedMotion,
            ),
            (
                "Toggle Opaque Panels",
                TopBarMenuAction::ToggleReduceTransparency,
            ),
        ],
        TopBarMenuHit::View => &[
            ("Workspace Overview", TopBarMenuAction::OpenOverview),
            ("Control Center", TopBarMenuAction::OpenControlCenter),
            ("Refresh Metrics", TopBarMenuAction::RefreshMetrics),
        ],
        TopBarMenuHit::Window => &[
            ("Focus Next Window", TopBarMenuAction::FocusNextWindow),
            (
                "Maximize / Restore",
                TopBarMenuAction::ToggleMaximizeFocused,
            ),
            ("Minimize Window", TopBarMenuAction::MinimizeFocused),
            ("Close Window", TopBarMenuAction::CloseFocused),
        ],
    }
}

/// Computes the glass dropdown popover rectangle anchored beneath `menu` (`RUSTYOS-048`).
#[must_use]
pub fn top_bar_menu_popover_rect(
    active_app_name: &str,
    menu: TopBarMenuHit,
) -> (i32, i32, i32, i32) {
    let left = compute_top_bar_left_layout(active_app_name);
    let anchor_x = match menu {
        TopBarMenuHit::SystemLogo => TOP_BAR_LOGO_X,
        TopBarMenuHit::ActiveApp => left.app_x,
        TopBarMenuHit::File => left.file_x,
        TopBarMenuHit::Edit => left.edit_x,
        TopBarMenuHit::View => left.view_x,
        TopBarMenuHit::Window => left.win_x,
    };
    let items_len = i32::try_from(top_bar_menu_items(menu).len()).unwrap_or(3);
    let pop_w = 212;
    let pop_h = 12 + items_len * 28;
    (anchor_x, TOP_BAR_HEIGHT + 6, pop_w, pop_h)
}

/// Hit-tests `(x, y)` against an open `TopBar` menu dropdown popover (`RUSTYOS-048`).
#[must_use]
pub fn check_top_bar_menu_popover_click(
    active_app_name: &str,
    menu: TopBarMenuHit,
    x: f64,
    y: f64,
) -> TopBarMenuPopoverClick {
    let (px, py, pw, ph) = top_bar_menu_popover_rect(active_app_name, menu);
    let fx = f64::from(px);
    let fy = f64::from(py);
    if x < fx || x >= fx + f64::from(pw) || y < fy || y >= fy + f64::from(ph) {
        return TopBarMenuPopoverClick::Outside;
    }
    let items = top_bar_menu_items(menu);
    let row_y = y - fy - 6.0;
    if row_y >= 0.0 {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let idx = (row_y / 28.0).floor() as usize;
        if let Some((_, action)) = items.get(idx) {
            return TopBarMenuPopoverClick::Action(*action);
        }
    }
    TopBarMenuPopoverClick::Inside
}

/// Hit-tests `(x, y)` against the macOS 27 `TopBar` Left Menu (`Logo`, `App`, `File`, `Edit`, `View`, `Window`) (`RUSTYOS-038`).
#[must_use]
pub fn check_top_bar_menu_click(x: f64, y: f64) -> Option<TopBarMenuHit> {
    check_top_bar_menu_click_for_app("RustyOS", x, y)
}

/// Active-app-width-aware hit-test for the `TopBar` Left Menu (`RUSTYOS-048`).
#[must_use]
pub fn check_top_bar_menu_click_for_app(
    active_app_name: &str,
    x: f64,
    y: f64,
) -> Option<TopBarMenuHit> {
    if y < 0.0 || y > f64::from(TOP_BAR_HEIGHT) {
        return None;
    }
    let left = compute_top_bar_left_layout(active_app_name);
    if x >= f64::from(TOP_BAR_LOGO_X) && x < f64::from(TOP_BAR_LOGO_X + TOP_BAR_LOGO_W) {
        return Some(TopBarMenuHit::SystemLogo);
    }
    if x >= f64::from(left.app_x) && x < f64::from(left.app_x + left.app_w) {
        return Some(TopBarMenuHit::ActiveApp);
    }
    if x >= f64::from(left.file_x) && x < f64::from(left.file_x + TOP_BAR_MENU_FILE_W) {
        return Some(TopBarMenuHit::File);
    }
    if x >= f64::from(left.edit_x) && x < f64::from(left.edit_x + TOP_BAR_MENU_EDIT_W) {
        return Some(TopBarMenuHit::Edit);
    }
    if x >= f64::from(left.view_x) && x < f64::from(left.view_x + TOP_BAR_MENU_VIEW_W) {
        return Some(TopBarMenuHit::View);
    }
    if x >= f64::from(left.win_x) && x < f64::from(left.win_x + TOP_BAR_MENU_WINDOW_W) {
        return Some(TopBarMenuHit::Window);
    }
    None
}

/// Checks if `(x, y)` clicked the `TopBar` Layout badge (`Float` / `Tile`).
#[must_use]
pub fn check_top_bar_layout_click(x: f64, y: f64) -> bool {
    check_top_bar_layout_click_for_app("RustyOS", x, y)
}

/// Active-app-width-aware hit-test for the `TopBar` Layout badge (`RUSTYOS-048`).
#[must_use]
pub fn check_top_bar_layout_click_for_app(active_app_name: &str, x: f64, y: f64) -> bool {
    if y < 0.0 || y > f64::from(TOP_BAR_HEIGHT) {
        return false;
    }
    let left = compute_top_bar_left_layout(active_app_name);
    x >= f64::from(left.layout_x) && x < f64::from(left.layout_x + left.layout_w)
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
    if y < 0.0 || y > f64::from(TOP_BAR_HEIGHT) {
        return false;
    }
    let right =
        compute_top_bar_right_layout(screen_width, has_battery, notification_count, clock_text);
    x >= f64::from(right.search_x - 3) && x < f64::from(right.search_x + right.search_w + 3)
}

/// Checks if `(x, y)` clicked the right `TopBar` telemetry status pill.
#[must_use]
pub fn check_top_bar_status_click(screen_width: i32, x: f64, y: f64) -> bool {
    check_top_bar_status_click_for_telemetry(screen_width, false, 0, "12:00", x, y)
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
    if y < 0.0 || y > f64::from(TOP_BAR_HEIGHT) {
        return false;
    }
    let right =
        compute_top_bar_right_layout(screen_width, has_battery, notification_count, clock_text);
    x >= f64::from(right.status_pill_x) && x < f64::from(right.status_pill_x + right.status_pill_w)
}

/// Hit-tests `(x, y)` against the open Quick Settings popover controls (`RUSTYOS-021`, `RUSTYOS-037`, `RUSTYOS-048`).
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
    let left_x = f64::from(px + pad);
    let right_x = f64::from(px + pad + half_w + 8);
    let hw = f64::from(half_w);

    // Sound control buttons row: y in py + 128 .. py + 152
    let audio_btn_y = f64::from(py + 128);
    if y >= audio_btn_y && y < audio_btn_y + 24.0 {
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

    // Sound interactive capsule slider track: y in py + 158 .. py + 178
    let track_x = f64::from(px + pad + 12);
    let track_w = f64::from((inner_w - 24).max(1));
    let sound_track_y = f64::from(py + 158);
    if y >= sound_track_y && y < sound_track_y + 20.0 && x >= track_x && x < track_x + track_w {
        let norm = ((x - track_x) / track_w).clamp(0.0, 1.0);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let pct = (norm * 100.0).round() as u8;
        return QuickSettingsHit::SetVolumePercent(pct.min(100));
    }

    // Glass Surface card: reduce_transparency toggle button (py + 200 .. py + 222)
    let glass_btn_x = f64::from(px + pw - pad - 86);
    let glass_btn_y = f64::from(py + 200);
    if y >= glass_btn_y && y < glass_btn_y + 24.0 && x >= glass_btn_x && x < glass_btn_x + 76.0 {
        return QuickSettingsHit::ToggleReduceTransparency;
    }

    // Glass Opacity interactive capsule slider track: y in py + 228 .. py + 248
    let glass_track_y = f64::from(py + 228);
    if y >= glass_track_y && y < glass_track_y + 20.0 && x >= track_x && x < track_x + track_w {
        let norm = ((x - track_x) / track_w).clamp(0.0, 1.0);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let pct = (35.0 + norm * 65.0).round() as u8;
        return QuickSettingsHit::SetGlassOpacityPercent(pct.clamp(35, 100));
    }

    // Action row 1: py + 260 .. py + 294
    let row1_y = f64::from(py + 260);
    if y >= row1_y && y < row1_y + 34.0 {
        if x >= left_x && x < left_x + hw {
            return QuickSettingsHit::ToggleLayout;
        }
        if x >= right_x && x < right_x + hw {
            return QuickSettingsHit::ToggleReducedMotion;
        }
    }

    // Action row 2: py + 302 .. py + 336
    let row2_y = f64::from(py + 302);
    if y >= row2_y && y < row2_y + 34.0 {
        if x >= left_x && x < left_x + hw {
            return QuickSettingsHit::OpenSpotlight;
        }
        if x >= right_x && x < right_x + hw {
            return QuickSettingsHit::RefreshMetrics;
        }
    }

    // Action row 3 (`RUSTYOS-027`): py + 344 .. py + 378
    let row3_y = f64::from(py + 344);
    if y >= row3_y && y < row3_y + 34.0 {
        if x >= left_x && x < left_x + hw {
            return QuickSettingsHit::ToggleBluetooth;
        }
        if x >= right_x && x < right_x + hw {
            return QuickSettingsHit::LockSession;
        }
    }

    QuickSettingsHit::InsidePopover
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

    // 2. Active App Menu & Global Menus in TopBar (macOS 27 style, RUSTYOS-038)
    elements.push(make_solid_element(
        Rectangle::new(
            Point::from((TOP_BAR_LOGO_X, TOP_BAR_ITEM_Y)),
            Size::from((TOP_BAR_LOGO_W, TOP_BAR_ITEM_H)),
        ),
        COLOR_ACCENT_MUTED,
    ));
    if !has_text {
        elements.push(make_solid_element(
            Rectangle::new(
                Point::from((TOP_BAR_APP_X, TOP_BAR_ITEM_Y)),
                Size::from((TOP_BAR_APP_MAX_W, TOP_BAR_ITEM_H)),
            ),
            COLOR_SURFACE_OVERLAY,
        ));
    }
    // Hairline divider before workspace indicators
    elements.push(make_solid_element(
        Rectangle::new(Point::from((TOP_BAR_SEPARATOR_X, 7)), Size::from((1, 12))),
        COLOR_BORDER,
    ));

    // 3. Compact Workspaces indicators in TopBar (1..=9) (RUSTYOS-038)
    let ws_btn_height = TOP_BAR_ITEM_H;
    let ws_y = TOP_BAR_ITEM_Y;
    let mut start_x = WS_START_X;

    for ws in 1..=9 {
        let is_active = ws == active_workspace;
        let ws_geo = Rectangle::new(
            Point::from((start_x, ws_y)),
            Size::from((WS_BTN_WIDTH, ws_btn_height)),
        );

        if is_active {
            elements.push(make_solid_element(ws_geo, COLOR_ACCENT));
        } else {
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

    // 4. Layout indicator in TopBar
    let layout_bg = Rectangle::new(
        Point::from((start_x + 8, ws_y)),
        Size::from((52, ws_btn_height)),
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
    /// Animated reveal progress (`0.0` closed -> `1.0` open) (`RUSTYOS-025`).
    pub progress: f64,
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
    let modal_width = 600.min(width - 48).max(320);
    let modal_height = 350.min(height - 100).max(180);
    let modal_x = (width - modal_width) / 2;
    let modal_y = (height - modal_height) / 4;
    (modal_x, modal_y, modal_width, modal_height)
}

/// Hit-tests `(x, y)` against the open Spotlight modal and its up to 5 visible rows (`RUSTYOS-018b`, `RUSTYOS-039`).
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

    let row_h = 46;
    let start_row_y = modal_y + 80;
    let row_x_min = f64::from(modal_x + 16);
    let row_x_max = f64::from(modal_x + modal_width - 16);
    if x >= row_x_min && x < row_x_max {
        for i in 0..5_i32 {
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

    // 7b. Search input bar plate (macOS 27 Liquid Glass style, RUSTYOS-039)
    let search_bar_h = 52;
    let search_bar_geo = Rectangle::new(
        Point::from((modal_x + 16, modal_y + 14)),
        Size::from((modal_width - 32, search_bar_h)),
    );
    elements.push(make_solid_element(search_bar_geo, COLOR_SURFACE_OVERLAY));

    // 7c. Search bar accent indicator
    let search_icon_geo = Rectangle::new(
        Point::from((modal_x + 28, modal_y + 28)),
        Size::from((24, 24)),
    );
    elements.push(make_solid_element(search_icon_geo, COLOR_ACCENT));

    // 7d. Dynamic text input cursor placed after typed query
    let cursor_x = (modal_x + 60 + spotlight.cursor_offset_x).min(modal_x + modal_width - 28);
    let cursor_geo = Rectangle::new(Point::from((cursor_x, modal_y + 28)), Size::from((2, 24)));
    elements.push(make_solid_element(cursor_geo, COLOR_TEXT));

    if !has_text {
        // 7e. Placeholder text indicator when no font renderer is active
        let placeholder_geo = Rectangle::new(
            Point::from((modal_x + 64, modal_y + 36)),
            Size::from((160, 8)),
        );
        elements.push(make_solid_element(placeholder_geo, COLOR_MUTED));
    }

    // 7f. Search divider
    let divider_geo = Rectangle::new(
        Point::from((modal_x + 16, modal_y + 72)),
        Size::from((modal_width - 32, 1)),
    );
    elements.push(make_solid_element(divider_geo, COLOR_BORDER));

    // 7g. Search results (up to 5 visible matching applications/commands)
    let row_h = 46;
    let start_row_y = modal_y + 80;
    let row_count = spotlight.items.len().min(5);
    for idx in 0..row_count {
        let i = i32::try_from(idx).unwrap_or(0);
        let row_y = start_row_y + i * (row_h + 4);
        let is_selected = idx == spotlight.selected_in_window;

        let row_bg = Rectangle::new(
            Point::from((modal_x + 16, row_y)),
            Size::from((modal_width - 32, row_h)),
        );
        let bg_color = if is_selected {
            COLOR_ACCENT_MUTED
        } else {
            COLOR_SURFACE
        };
        elements.push(make_solid_element(row_bg, bg_color));

        if is_selected {
            let select_indicator = Rectangle::new(
                Point::from((modal_x + 18, row_y + 8)),
                Size::from((3, row_h - 16)),
            );
            elements.push(make_solid_element(select_indicator, COLOR_ACCENT));
        }

        // App icon badge
        let app_icon = Rectangle::new(Point::from((modal_x + 28, row_y + 9)), Size::from((28, 28)));
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
                Point::from((modal_x + 66, row_y + 12)),
                Size::from((120 + i * 20, 8)),
            );
            elements.push(make_solid_element(title_bar, COLOR_TEXT));

            // App subtitle bar fallback
            let sub_bar = Rectangle::new(
                Point::from((modal_x + 66, row_y + 26)),
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
        Rectangle::new(Point::from((px + pad, py + 50)), Size::from((inner_w, 1))),
        COLOR_BORDER,
    ));

    // 2. Network & Power info tiles (macOS Control Center style)
    let left_x = px + pad;
    let right_x = px + pad + half_w + 8;
    elements.push(make_solid_element(
        Rectangle::new(Point::from((left_x, py + 56)), Size::from((half_w, 58))),
        COLOR_SURFACE_OVERLAY,
    ));
    elements.push(make_solid_element(
        Rectangle::new(Point::from((left_x + 12, py + 71)), Size::from((28, 28))),
        if qs.net_connected {
            COLOR_ACCENT_MUTED
        } else {
            COLOR_SURFACE
        },
    ));

    elements.push(make_solid_element(
        Rectangle::new(Point::from((right_x, py + 56)), Size::from((half_w, 58))),
        COLOR_SURFACE_OVERLAY,
    ));
    elements.push(make_solid_element(
        Rectangle::new(Point::from((right_x + 12, py + 71)), Size::from((28, 28))),
        if qs.ac_online || qs.battery_percent > 20 {
            COLOR_SUCCESS
        } else {
            COLOR_WARNING
        },
    ));

    // 3. Audio slider card
    elements.push(make_solid_element(
        Rectangle::new(Point::from((left_x, py + 122)), Size::from((inner_w, 66))),
        COLOR_SURFACE_OVERLAY,
    ));

    // Mute, -, + buttons inside audio card
    let btn_y = py + 128;
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

    // Capsule audio progress bar track + fill
    let track_w = inner_w - 24;
    let track_x = left_x + 12;
    let track_y = py + 158;
    elements.push(make_solid_element(
        Rectangle::new(Point::from((track_x, track_y)), Size::from((track_w, 18))),
        COLOR_SURFACE,
    ));
    let fill_w = (track_w * i32::from(qs.volume_percent.min(100))) / 100;
    if fill_w > 0 {
        elements.push(make_solid_element(
            Rectangle::new(Point::from((track_x, track_y)), Size::from((fill_w, 18))),
            if qs.audio_muted {
                COLOR_SUBTLE
            } else {
                COLOR_ACCENT
            },
        ));
    }

    // 4. Quick Action toggles shifted below Sound + Glass cards (`RUSTYOS-048`)
    let is_tile = qs.layout_mode == "master-stack";
    elements.push(make_solid_element(
        Rectangle::new(Point::from((left_x, py + 260)), Size::from((half_w, 34))),
        if is_tile {
            COLOR_ACCENT_MUTED
        } else {
            COLOR_SURFACE_OVERLAY
        },
    ));
    elements.push(make_solid_element(
        Rectangle::new(Point::from((right_x, py + 260)), Size::from((half_w, 34))),
        if qs.reduced_motion {
            COLOR_ACCENT_MUTED
        } else {
            COLOR_SURFACE_OVERLAY
        },
    ));
    elements.push(make_solid_element(
        Rectangle::new(Point::from((left_x, py + 302)), Size::from((half_w, 34))),
        COLOR_SURFACE_OVERLAY,
    ));
    elements.push(make_solid_element(
        Rectangle::new(Point::from((right_x, py + 302)), Size::from((half_w, 34))),
        COLOR_SURFACE_OVERLAY,
    ));
    elements.push(make_solid_element(
        Rectangle::new(Point::from((left_x, py + 344)), Size::from((half_w, 34))),
        if qs.bt_available && qs.bt_powered {
            COLOR_ACCENT_MUTED
        } else {
            COLOR_SURFACE_OVERLAY
        },
    ));
    elements.push(make_solid_element(
        Rectangle::new(Point::from((right_x, py + 344)), Size::from((half_w, 34))),
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
        ShellOverlayView::None | ShellOverlayView::Overview(_) | ShellOverlayView::Switcher(_) => {}
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
    check_top_bar_workspace_click_for_app("RustyOS", x, y)
}

/// Active-app-width-aware hit-test for the `TopBar` workspaces area (`RUSTYOS-048`).
#[must_use]
pub fn check_top_bar_workspace_click_for_app(active_app_name: &str, x: f64, y: f64) -> Option<u8> {
    if y < 0.0 || y > f64::from(TOP_BAR_HEIGHT) {
        return None;
    }
    let left = compute_top_bar_left_layout(active_app_name);
    let start_x = f64::from(left.ws_start_x);
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

/// Generates GPU-cached real font text labels for `TopBar`, `Dock`, `Spotlight`, `QuickSettings`,
/// `Workspace Overview`, and `Notification HUD` (`RUSTYOS-020`/`021`/`023`/`024`).
#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
pub fn generate_shell_text_elements(
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    active_workspace: u8,
    layout_mode: &str,
    dock_view: &DockViewState<'_>,
    top_telemetry: &TopBarTelemetry<'_>,
    overlay: ShellOverlayView<'_>,
    notifications: &[NotificationToastView<'_>],
) -> Vec<MemoryRenderBufferRenderElement<GlesRenderer>> {
    let mut elements = Vec::new();
    if !text_cache.has_font() {
        return elements;
    }

    let width = screen_size.w;
    let height = screen_size.h;
    let text_bright = [244, 240, 250, 255]; // #F4F0FA
    let text_muted = [166, 155, 181, 255]; // #A69BB5

    // 1. Overlay labels (Spotlight or Quick Settings popover)
    match overlay {
        ShellOverlayView::None => {}
        ShellOverlayView::Spotlight(spot) => {
            let (modal_x, base_modal_y, modal_width, _modal_height) =
                spotlight_modal_rect(width, height);
            let modal_y = base_modal_y + anim_offset(spot.progress, -22.0);

            if spot.query.is_empty() {
                if let Some(el) = text_cache.render_label(
                    renderer,
                    "Spotlight Search",
                    14.5,
                    text_muted,
                    Point::from((modal_x + 60, modal_y + 30)),
                ) {
                    elements.push(el);
                }
            } else if let Some(el) = text_cache.render_label(
                renderer,
                spot.query,
                14.5,
                text_bright,
                Point::from((modal_x + 60, modal_y + 30)),
            ) {
                elements.push(el);
            }

            let start_row_y = modal_y + 80;
            let row_h = 46;
            for (i, item) in spot.items.iter().take(5).enumerate() {
                #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
                let row_y = start_row_y + (i as i32) * (row_h + 4);
                let is_sel = i == spot.selected_in_window;

                let has_icon = item
                    .icon
                    .as_deref()
                    .is_some_and(|name| shape_cache.icons.has_named_icon(name, 28));
                if !has_icon {
                    let badge_bounds = Rectangle::new(
                        Point::from((modal_x + 28, row_y + 9)),
                        Size::from((28, 28)),
                    );
                    if let Some(el) = text_cache.render_label_centered(
                        renderer,
                        &item.badge,
                        11.0,
                        text_bright,
                        badge_bounds,
                    ) {
                        elements.push(el);
                    }
                }

                if let Some(el) = text_cache.render_label(
                    renderer,
                    &item.name,
                    13.5,
                    text_bright,
                    Point::from((modal_x + 68, row_y + 7)),
                ) {
                    elements.push(el);
                }
                if let Some(el) = text_cache.render_label(
                    renderer,
                    &item.subtitle,
                    10.5,
                    text_muted,
                    Point::from((modal_x + 68, row_y + 26)),
                ) {
                    elements.push(el);
                }

                // macOS Spotlight Category Tag on the right (RUSTYOS-039)
                let cat_label = if item.is_custom_command {
                    "Command"
                } else {
                    "Application"
                };
                let cat_bounds = Rectangle::new(
                    Point::from((modal_x + modal_width - 104, row_y + 14)),
                    Size::from((80, 18)),
                );
                let cat_color = if is_sel {
                    [230, 220, 245, 255]
                } else {
                    text_muted
                };
                if let Some(el) = text_cache
                    .render_label_centered(renderer, cat_label, 10.0, cat_color, cat_bounds)
                {
                    elements.push(el);
                }
            }
        }
        ShellOverlayView::QuickSettings(qs) => {
            render_quick_settings_text(text_cache, renderer, width, height, qs, &mut elements);
        }
        ShellOverlayView::Overview(ov) => {
            render_overview_text(text_cache, renderer, screen_size, ov, &mut elements);
        }
        ShellOverlayView::Switcher(sw) => {
            render_switcher_text(
                text_cache,
                shape_cache,
                renderer,
                screen_size,
                sw,
                &mut elements,
            );
        }
    }

    // 1b. TopBar Left Active App & Global Menus (macOS 27 dynamic layout, RUSTYOS-038, RUSTYOS-048)
    let ws_btn_height = TOP_BAR_ITEM_H;
    let ws_y = TOP_BAR_ITEM_Y;

    let active_app_name = dock_view
        .windows
        .iter()
        .find(|w| w.focused)
        .map_or_else(|| "RustyOS".to_owned(), |w| extract_app_name(&w.title));
    let left_layout = compute_top_bar_left_layout(&active_app_name);

    if let Some(el) = text_cache.render_label(
        renderer,
        &active_app_name,
        12.5,
        text_bright,
        Point::from((left_layout.app_x, ws_y + 1)),
    ) {
        elements.push(el);
    }

    let text_menu = [215, 205, 230, 230];
    for (menu_kind, name, x, w) in [
        (
            TopBarMenuHit::File,
            "File",
            left_layout.file_x,
            TOP_BAR_MENU_FILE_W,
        ),
        (
            TopBarMenuHit::Edit,
            "Edit",
            left_layout.edit_x,
            TOP_BAR_MENU_EDIT_W,
        ),
        (
            TopBarMenuHit::View,
            "View",
            left_layout.view_x,
            TOP_BAR_MENU_VIEW_W,
        ),
        (
            TopBarMenuHit::Window,
            "Window",
            left_layout.win_x,
            TOP_BAR_MENU_WINDOW_W,
        ),
    ] {
        let color = if top_telemetry.active_menu == Some(menu_kind) {
            text_bright
        } else {
            text_menu
        };
        let bounds = Rectangle::new(Point::from((x, ws_y)), Size::from((w, ws_btn_height)));
        if let Some(el) = text_cache.render_label_centered(renderer, name, 11.5, color, bounds) {
            elements.push(el);
        }
    }

    // Active TopBar left dropdown menu labels (`RUSTYOS-048`)
    if let Some(open_menu) = top_telemetry.active_menu {
        let (mx, my, mw, _) = top_bar_menu_popover_rect(&active_app_name, open_menu);
        for (idx, (label, _)) in top_bar_menu_items(open_menu).iter().enumerate() {
            let idx_i32 = i32::try_from(idx).unwrap_or(0);
            let row_y = my + 6 + idx_i32 * 28;
            if let Some(el) = text_cache.render_label(
                renderer,
                label,
                11.5,
                text_bright,
                Point::from((mx + 14, row_y + 5)),
            ) {
                elements.push(el);
            }
            let _ = mw;
        }
    }

    // 2. TopBar compact workspace digits (1..=9)
    let mut start_x = left_layout.ws_start_x;
    for ws in 1..=9 {
        let color = if ws == active_workspace {
            [255, 255, 255, 255]
        } else {
            [178, 168, 194, 210]
        };
        let bounds = Rectangle::new(
            Point::from((start_x, ws_y)),
            Size::from((WS_BTN_WIDTH, ws_btn_height)),
        );
        if let Some(el) =
            text_cache.render_label_centered(renderer, &ws.to_string(), 10.5, color, bounds)
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
        Point::from((left_layout.layout_x, ws_y)),
        Size::from((left_layout.layout_w, ws_btn_height)),
    );
    if let Some(el) =
        text_cache.render_label_centered(renderer, layout_label, 11.0, text_bright, layout_bounds)
    {
        elements.push(el);
    }

    // 4. Right status cluster live clock, kbd, notification badge & tray (`RUSTYOS-048`)
    let right_layout = compute_top_bar_right_layout(
        width,
        top_telemetry.has_battery,
        top_telemetry.notification_count,
        top_telemetry.clock_hhmm,
    );
    let clock_bounds = Rectangle::new(
        Point::from((right_layout.clock_x, ws_y)),
        Size::from((right_layout.clock_w, ws_btn_height)),
    );
    if let Some(el) = text_cache.render_label_centered(
        renderer,
        top_telemetry.clock_hhmm,
        10.5,
        text_bright,
        clock_bounds,
    ) {
        elements.push(el);
    }

    // 4a. Keyboard layout indicator & unread notification badge in TopBar (`RUSTYOS-024`/`029`)
    let kbd_bounds = Rectangle::new(
        Point::from((right_layout.kbd_x, ws_y)),
        Size::from((right_layout.kbd_w, ws_btn_height)),
    );
    if let Some(el) = text_cache.render_label_centered(
        renderer,
        top_telemetry.kbd_layout,
        10.5,
        text_bright,
        kbd_bounds,
    ) {
        elements.push(el);
    }

    if top_telemetry.notification_count > 0 {
        let badge_label = if top_telemetry.notification_count > 99 {
            "N:99+".to_owned()
        } else {
            format!("N:{}", top_telemetry.notification_count)
        };
        let badge_bounds = Rectangle::new(
            Point::from((right_layout.notif_x, ws_y)),
            Size::from((right_layout.notif_w, ws_btn_height)),
        );
        if let Some(el) = text_cache.render_label_centered(
            renderer,
            &badge_label,
            10.5,
            text_bright,
            badge_bounds,
        ) {
            elements.push(el);
        }
    }

    // 4aa. System tray badges (`StatusNotifierItem`) left of keyboard layout (`RUSTYOS-032`)
    let tray_rects = tray_slot_rects_with_layout(
        width,
        top_telemetry.has_battery,
        top_telemetry.notification_count,
        top_telemetry.clock_hhmm,
        top_telemetry.tray_items.len(),
    );
    for (item, (tx, _ty, tw, _th)) in top_telemetry.tray_items.iter().zip(tray_rects) {
        let has_tray_icon = item.icon_pixmap.is_some()
            || (!item.icon_name.is_empty() && shape_cache.icons.has_named_icon(item.icon_name, 16));
        if has_tray_icon {
            continue;
        }
        let label_color = if item.attention {
            [255, 210, 214, 255]
        } else if item.passive {
            text_muted
        } else {
            text_bright
        };
        let tray_bounds = Rectangle::new(Point::from((tx, ws_y)), Size::from((tw, ws_btn_height)));
        if let Some(el) =
            text_cache.render_label_centered(renderer, item.label, 9.5, label_color, tray_bounds)
        {
            elements.push(el);
        }
    }

    // 4b. Floating top-right Notification HUD toast labels (`RUSTYOS-024`)
    if !matches!(
        overlay,
        ShellOverlayView::Spotlight(_) | ShellOverlayView::QuickSettings(_)
    ) && !notifications.is_empty()
    {
        let rects = notification_toast_rects(screen_size, notifications.len());
        for (toast, card) in notifications.iter().zip(rects) {
            let tx = card.loc.x;
            let ty = card.loc.y;
            let tw = card.size.w;

            let title_text = truncate_ascii(toast.title, 30);
            if let Some(el) = text_cache.render_label(
                renderer,
                &title_text,
                12.5,
                text_bright,
                Point::from((tx + 22, ty + 11)),
            ) {
                elements.push(el);
            }

            let body_text = truncate_ascii(toast.body, 40);
            if let Some(el) = text_cache.render_label(
                renderer,
                &body_text,
                10.5,
                text_muted,
                Point::from((tx + 22, ty + 34)),
            ) {
                elements.push(el);
            }

            let close_bounds =
                Rectangle::new(Point::from((tx + tw - 26, ty + 10)), Size::from((18, 18)));
            if let Some(el) =
                text_cache.render_label_centered(renderer, "x", 10.5, text_muted, close_bounds)
            {
                elements.push(el);
            }
        }
    }

    // Dock tiles now exclusively use XDG PNG, SHM, or procedural macOS 27 squircles (`RUSTYOS-048`).
    let _ = (dock_view, shape_cache);

    elements
}

fn render_switcher_text(
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    sw: &WindowSwitcherRenderState<'_>,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let text_bright = [244, 240, 250, 255];
    let text_muted = [166, 155, 181, 255];
    let (_panel, cards) = switcher_layout(screen_size, sw.cards.len());
    for (idx, (card, rect)) in sw.cards.iter().zip(cards).enumerate() {
        let is_sel = idx == sw.selected;
        let sw_key = format!("win-{}", card.window_id.0);
        let has_icon = shape_cache.icons.has_window_icon(
            renderer,
            &sw_key,
            card.toplevel_icon,
            card.icon_name,
            36,
        );
        if !has_icon {
            let icon_box = Rectangle::new(
                Point::from((rect.loc.x + (rect.size.w - 44) / 2, rect.loc.y + 14)),
                Size::from((44, 44)),
            );
            if let Some(el) =
                text_cache.render_label_centered(renderer, card.badge, 14.5, text_bright, icon_box)
            {
                elements.push(el);
            }
        }
        let raw_title = if card.minimized {
            format!("Min: {}", card.title)
        } else {
            card.title.to_owned()
        };
        let title_str = truncate_ascii(&raw_title, 13);
        let title_bounds = Rectangle::new(
            Point::from((rect.loc.x + 6, rect.loc.y + rect.size.h - 30)),
            Size::from((rect.size.w - 12, 22)),
        );
        if let Some(el) = text_cache.render_label_centered(
            renderer,
            &title_str,
            10.5,
            if is_sel { text_bright } else { text_muted },
            title_bounds,
        ) {
            elements.push(el);
        }
    }
}

fn render_overview_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    ov: &OverviewRenderState<'_>,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let text_bright = [244, 240, 250, 255];
    let text_muted = [166, 155, 181, 255];
    let width = screen_size.w;

    let heading_dy = anim_offset(ov.progress, 14.0);
    let heading = format!(
        "Workspace {}  •  {} windows  ({})",
        ov.active_workspace,
        ov.windows.len(),
        ov.layout_mode
    );
    if let Some(el) = text_cache.render_label_centered(
        renderer,
        &heading,
        15.0,
        text_bright,
        Rectangle::new(
            Point::from((0, TOP_BAR_HEIGHT + 14 + heading_dy)),
            Size::from((width, 24)),
        ),
    ) {
        elements.push(el);
    }
    if let Some(el) = text_cache.render_label_centered(
        renderer,
        "Click a window card to focus  •  Esc or Dock Ov to close",
        11.0,
        text_muted,
        Rectangle::new(
            Point::from((0, TOP_BAR_HEIGHT + 38 + heading_dy)),
            Size::from((width, 18)),
        ),
    ) {
        elements.push(el);
    }

    if ov.windows.is_empty() {
        let empty_msg = format!(
            "Workspace {} is empty — launch an app from Dock >_ or Alt+Space",
            ov.active_workspace
        );
        if let Some(el) = text_cache.render_label_centered(
            renderer,
            &empty_msg,
            13.0,
            text_muted,
            Rectangle::new(
                Point::from((32, screen_size.h / 2 - 20 + heading_dy)),
                Size::from((width - 64, 28)),
            ),
        ) {
            elements.push(el);
        }
        return;
    }

    let card_dy = anim_offset(ov.progress, 22.0);
    let cards = overview_card_rects(screen_size, ov.windows.len());
    for (win, card) in ov.windows.iter().zip(cards) {
        let header_bounds = Rectangle::new(
            Point::from((card.loc.x + 10, card.loc.y + 4 + card_dy)),
            Size::from((card.size.w - 20, 24)),
        );
        if let Some(el) = text_cache.render_label_centered(
            renderer,
            &win.title,
            12.0,
            if win.focused { text_bright } else { text_muted },
            header_bounds,
        ) {
            elements.push(el);
        }
        let body_bounds = Rectangle::new(
            Point::from((card.loc.x + 14, card.loc.y + 34 + card_dy)),
            Size::from((card.size.w - 28, card.size.h - 46)),
        );
        let center_badge = format!("{}  •  {}", win.badge, win.dimensions);
        if let Some(el) = text_cache.render_label_centered(
            renderer,
            &center_badge,
            12.0,
            text_bright,
            body_bounds,
        ) {
            elements.push(el);
        }
    }
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
    let (px, base_py, pw, _ph) = quick_settings_rect(width, height);
    let py = base_py + anim_offset(qs.progress, -16.0);
    let pad = 14;
    let inner_w = pw - pad * 2;
    let half_w = (inner_w - 8) / 2;
    let left_x = px + pad;
    let right_x = px + pad + half_w + 8;

    let text_bright = [244, 240, 250, 255];
    let text_muted = [166, 155, 181, 255];

    if let Some(el) = text_cache.render_label(
        renderer,
        "Control Center",
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
        Point::from((left_x + 46, py + 64)),
    ) {
        elements.push(el);
    }
    let net_sub = truncate_ascii(qs.network_label, 13);
    if let Some(el) = text_cache.render_label(
        renderer,
        &net_sub,
        10.0,
        text_muted,
        Point::from((left_x + 46, py + 86)),
    ) {
        elements.push(el);
    }

    // Power tile
    if let Some(el) = text_cache.render_label(
        renderer,
        "Power",
        11.5,
        text_bright,
        Point::from((right_x + 46, py + 64)),
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
        10.0,
        text_muted,
        Point::from((right_x + 46, py + 86)),
    ) {
        elements.push(el);
    }

    // Audio row
    let vol_title = if qs.audio_muted {
        format!("Sound: Muted ({}%)", qs.volume_percent)
    } else {
        format!("Sound: {}%", qs.volume_percent)
    };
    if let Some(el) = text_cache.render_label(
        renderer,
        &vol_title,
        11.5,
        text_bright,
        Point::from((left_x + 12, py + 133)),
    ) {
        elements.push(el);
    }

    let btn_y = py + 128;
    let mute_x = px + pw - pad - 116;
    let minus_x = px + pw - pad - 62;
    let plus_x = px + pw - pad - 30;
    let mute_label = if qs.audio_muted { "Unmute" } else { "Mute" };
    if let Some(el) = text_cache.render_label_centered(
        renderer,
        mute_label,
        10.0,
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

    // Glass Surface card (`RUSTYOS-048`)
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let glass_pct = (crate::config::clamp_glass_opacity(qs.glass_opacity) * 100.0).round() as u8;
    let glass_title = if qs.reduce_transparency {
        "Glass: Opaque Mode".to_owned()
    } else {
        format!("Glass Opacity: {glass_pct}%")
    };
    if let Some(el) = text_cache.render_label(
        renderer,
        &glass_title,
        11.0,
        text_bright,
        Point::from((left_x + 12, py + 202)),
    ) {
        elements.push(el);
    }
    let transp_btn_label = if qs.reduce_transparency {
        "Glass"
    } else {
        "Opaque"
    };
    let glass_btn_x = px + pw - pad - 86;
    if let Some(el) = text_cache.render_label_centered(
        renderer,
        transp_btn_label,
        10.0,
        text_bright,
        Rectangle::new(Point::from((glass_btn_x, py + 200)), Size::from((76, 22))),
    ) {
        elements.push(el);
    }

    // Quick toggle labels (2 columns, 3 rows shifted below Glass card)
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
        Rectangle::new(Point::from((left_x, py + 260)), Size::from((half_w, 34))),
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
        Rectangle::new(Point::from((right_x, py + 260)), Size::from((half_w, 34))),
    ) {
        elements.push(el);
    }
    if let Some(el) = text_cache.render_label_centered(
        renderer,
        "Open Spotlight",
        11.0,
        text_bright,
        Rectangle::new(Point::from((left_x, py + 302)), Size::from((half_w, 34))),
    ) {
        elements.push(el);
    }
    if let Some(el) = text_cache.render_label_centered(
        renderer,
        "Refresh Metrics",
        11.0,
        text_bright,
        Rectangle::new(Point::from((right_x, py + 302)), Size::from((half_w, 34))),
    ) {
        elements.push(el);
    }
    let bt_text = truncate_ascii(qs.bt_label, 17);
    if let Some(el) = text_cache.render_label_centered(
        renderer,
        &bt_text,
        11.0,
        if qs.bt_available {
            text_bright
        } else {
            text_muted
        },
        Rectangle::new(Point::from((left_x, py + 344)), Size::from((half_w, 34))),
    ) {
        elements.push(el);
    }
    if let Some(el) = text_cache.render_label_centered(
        renderer,
        "Lock Session",
        11.0,
        text_bright,
        Rectangle::new(Point::from((right_x, py + 344)), Size::from((half_w, 34))),
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

const RGBA_SURFACE: [u8; 4] = [22, 16, 35, 255]; // #161023
const RGBA_SURFACE_ELEVATED: [u8; 4] = [32, 24, 46, 245]; // #20182E @ 0.96
const RGBA_SURFACE_OVERLAY: [u8; 4] = [41, 32, 55, 250]; // #292037 @ 0.98
const RGBA_ACCENT: [u8; 4] = [165, 110, 255, 255]; // #A56EFF
const RGBA_ACCENT_HOVER: [u8; 4] = [185, 138, 255, 255]; // #B98AFF
const RGBA_ACCENT_MUTED: [u8; 4] = [118, 80, 181, 255]; // #7650B5
const RGBA_SUBTLE: [u8; 4] = [118, 107, 133, 255]; // #766B85
const RGBA_BORDER: [u8; 4] = [255, 255, 255, 31];
const RGBA_BORDER_HIGHLIGHT: [u8; 4] = [255, 255, 255, 56];
const RGBA_DANGER: [u8; 4] = [229, 107, 117, 255];
const RGBA_WARNING: [u8; 4] = [232, 196, 119, 255];
const RGBA_SUCCESS: [u8; 4] = [131, 214, 160, 255];

fn rgba_to_premul_color32f(rgba: [u8; 4]) -> Color32F {
    let a = f32::from(rgba[3]) / 255.0;
    let r = f32::from(rgba[0]) / 255.0;
    let g = f32::from(rgba[1]) / 255.0;
    let b = f32::from(rgba[2]) / 255.0;
    Color32F::new(r * a, g * a, b * a, a)
}

fn blend_border_over_fill_color(border: [u8; 4], fill: [u8; 4]) -> Color32F {
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
fn push_rounded_glass_box(
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
fn push_soft_shadow(
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

/// Draws monochrome status glyphs (signal, volume, battery) left of the clock (`RUSTYOS-036`).
fn push_top_bar_right_glyphs(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    right: &TopBarRightLayout,
    t: &TopBarTelemetry<'_>,
) {
    use crate::surfaces::UiGlyphKind;

    let white = [244, 240, 250, 255];
    let dim_white = [220, 212, 236, 225];
    let glyph_y = TOP_BAR_ITEM_Y + (TOP_BAR_ITEM_H - 16) / 2;

    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        UiGlyphKind::Search,
        15,
        dim_white,
        Point::from((right.search_x + 1, glyph_y + 1)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        UiGlyphKind::Wifi {
            connected: t.net_connected,
        },
        16,
        white,
        Point::from((right.wifi_x, glyph_y)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        UiGlyphKind::Volume {
            level_percent: t.volume_percent,
            muted: t.audio_muted,
        },
        16,
        white,
        Point::from((right.volume_x, glyph_y)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    if let Some(bx) = right.battery_x
        && let Some(el) = shape_cache.ui_glyph_element(
            renderer,
            UiGlyphKind::Battery {
                percent: t.battery_percent,
                charging: t.battery_percent >= 98,
            },
            18,
            white,
            Point::from((bx + 1, glyph_y - 1)),
        )
    {
        out.push(CompositorRenderElement::Memory(el));
    }
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        UiGlyphKind::ControlCenter,
        16,
        if t.quick_settings_open {
            [185, 138, 255, 255]
        } else {
            white
        },
        Point::from((right.control_center_x + 2, glyph_y)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
}

fn title_hue_seed(title: &str, id: u64) -> u8 {
    let base = u8::try_from(id & 0xff).unwrap_or(0);
    title.bytes().fold(base, u8::wrapping_add) % 4
}

/// Generates unified front-to-back shell elements with rounded corners, circular status dots,
/// soft shadows, interactive Dock badges, Overview cards, Notification HUD, and GPU-cached typography (`RUSTYOS-022`/`023`/`024`).
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub fn generate_shell_composite_elements(
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    active_workspace: u8,
    layout_mode: &str,
    dock_view: &DockViewState<'_>,
    top_telemetry: &TopBarTelemetry<'_>,
    overlay: ShellOverlayView<'_>,
    notifications: &[NotificationToastView<'_>],
) -> Vec<CompositorRenderElement> {
    let mut out = Vec::new();
    let width = screen_size.w;
    let height = screen_size.h;
    let has_text = text_cache.has_font();

    // 1. All shell text labels on the topmost plane
    let all_text = generate_shell_text_elements(
        text_cache,
        shape_cache,
        renderer,
        screen_size,
        active_workspace,
        layout_mode,
        dock_view,
        top_telemetry,
        overlay,
        notifications,
    );
    for el in all_text {
        out.push(CompositorRenderElement::Memory(el));
    }

    // 2. Active modal Overlay (Spotlight or QuickSettings) above TopBar/Dock/windows
    match overlay {
        ShellOverlayView::None | ShellOverlayView::Overview(_) => {}
        ShellOverlayView::Spotlight(spot) => {
            let (mx, base_my, mw, mh) = spotlight_modal_rect(width, height);
            let my = base_my + anim_offset(spot.progress, -22.0);
            let glass_op = top_telemetry.glass_opacity;
            let reduce_tr = top_telemetry.reduce_transparency;

            // SDF Search loupe & cursor inside search input bar (`RUSTYOS-048`)
            if let Some(el) = shape_cache.ui_glyph_element(
                renderer,
                crate::surfaces::UiGlyphKind::Search,
                18,
                [215, 205, 235, 255],
                Point::from((mx + 30, my + 31)),
            ) {
                out.push(CompositorRenderElement::Memory(el));
            }
            let cursor_x = (mx + 60 + spot.cursor_offset_x).min(mx + mw - 28);
            out.push(CompositorRenderElement::Solid(make_solid_element(
                Rectangle::new(Point::from((cursor_x, my + 28)), Size::from((2, 24))),
                COLOR_ACCENT,
            )));
            if !has_text {
                out.push(CompositorRenderElement::Solid(make_solid_element(
                    Rectangle::new(Point::from((mx + 64, my + 36)), Size::from((160, 8))),
                    COLOR_MUTED,
                )));
            }

            // Search input bar plate (macOS 27 Liquid Glass style, RUSTYOS-039)
            push_rounded_glass_box(
                &mut out,
                shape_cache,
                renderer,
                Rectangle::new(Point::from((mx + 16, my + 14)), Size::from((mw - 32, 52))),
                14,
                RGBA_SURFACE_OVERLAY,
                Some(RGBA_BORDER),
            );
            out.push(CompositorRenderElement::Solid(make_solid_element(
                Rectangle::new(Point::from((mx + 16, my + 72)), Size::from((mw - 32, 1))),
                COLOR_BORDER,
            )));

            let row_h = 46;
            let start_row_y = my + 80;
            for idx in 0..spot.items.len().min(5) {
                let i = i32::try_from(idx).unwrap_or(0);
                let row_y = start_row_y + i * (row_h + 4);
                let is_sel = idx == spot.selected_in_window;

                if is_sel {
                    out.push(CompositorRenderElement::Solid(make_solid_element(
                        Rectangle::new(
                            Point::from((mx + 18, row_y + 8)),
                            Size::from((3, row_h - 16)),
                        ),
                        COLOR_ACCENT,
                    )));
                }
                let icon_el = spot
                    .items
                    .get(idx)
                    .and_then(|item| item.icon.as_deref())
                    .and_then(|icon_name| {
                        shape_cache.icons.icon_element_by_name(
                            renderer,
                            icon_name,
                            28,
                            Point::from((mx + 28, row_y + 9)),
                        )
                    });
                if let Some(el) = icon_el {
                    out.push(CompositorRenderElement::Memory(el));
                } else {
                    let icon_rgba = if is_sel {
                        RGBA_ACCENT
                    } else if idx == 1 {
                        RGBA_ACCENT_MUTED
                    } else {
                        RGBA_SUBTLE
                    };
                    push_rounded_glass_box(
                        &mut out,
                        shape_cache,
                        renderer,
                        Rectangle::new(Point::from((mx + 28, row_y + 9)), Size::from((28, 28))),
                        7,
                        icon_rgba,
                        None,
                    );
                }
                if !has_text {
                    out.push(CompositorRenderElement::Solid(make_solid_element(
                        Rectangle::new(
                            Point::from((mx + 66, row_y + 12)),
                            Size::from((120 + i * 20, 8)),
                        ),
                        COLOR_TEXT,
                    )));
                }
                let row_rgba = if is_sel {
                    RGBA_ACCENT_MUTED
                } else {
                    RGBA_SURFACE
                };
                push_rounded_glass_box(
                    &mut out,
                    shape_cache,
                    renderer,
                    Rectangle::new(Point::from((mx + 16, row_y)), Size::from((mw - 32, row_h))),
                    12,
                    row_rgba,
                    if is_sel { Some(RGBA_ACCENT) } else { None },
                );
            }

            let modal_rect = Rectangle::new(Point::from((mx, my)), Size::from((mw, mh)));
            push_rounded_glass_box(
                &mut out,
                shape_cache,
                renderer,
                modal_rect,
                WINDOW_RADIUS,
                scale_glass_rgba(RGBA_SURFACE_ELEVATED, glass_op, reduce_tr),
                Some(RGBA_BORDER_HIGHLIGHT),
            );
            push_soft_shadow(
                &mut out,
                shape_cache,
                renderer,
                modal_rect,
                WINDOW_RADIUS,
                28,
                108,
            );
        }
        ShellOverlayView::QuickSettings(qs) => {
            let (px, base_py, pw, ph) = quick_settings_rect(width, height);
            let py = base_py + anim_offset(qs.progress, -16.0);
            let pad = 14;
            let inner_w = pw - pad * 2;
            let half_w = (inner_w - 8) / 2;
            let left_x = px + pad;
            let right_x = px + pad + half_w + 8;

            // 1. Primary round toggle buttons & SDF status glyphs inside top cards
            let net_circle_fill = if qs.net_connected {
                RGBA_ACCENT
            } else {
                RGBA_SURFACE
            };
            let net_circle_border = Some(if qs.net_connected {
                RGBA_BORDER_HIGHLIGHT
            } else {
                RGBA_BORDER
            });
            if let Some(el) = shape_cache.ui_glyph_element(
                renderer,
                crate::surfaces::UiGlyphKind::Wifi {
                    connected: qs.net_connected,
                },
                16,
                [248, 245, 255, 255],
                Point::from((left_x + 15, py + 77)),
            ) {
                out.push(CompositorRenderElement::Memory(el));
            }
            if let Some(el) = shape_cache.circle_element(
                renderer,
                30,
                net_circle_fill,
                net_circle_border,
                Point::from((left_x + 8, py + 70)),
            ) {
                out.push(CompositorRenderElement::Memory(el));
            }

            let power_fill = if qs.ac_online {
                RGBA_SUCCESS
            } else if qs.battery_percent > 20 {
                RGBA_ACCENT_MUTED
            } else {
                RGBA_WARNING
            };
            if let Some(el) = shape_cache.ui_glyph_element(
                renderer,
                crate::surfaces::UiGlyphKind::Battery {
                    percent: qs.battery_percent,
                    charging: qs.ac_online,
                },
                16,
                [248, 245, 255, 255],
                Point::from((right_x + 15, py + 77)),
            ) {
                out.push(CompositorRenderElement::Memory(el));
            }
            if let Some(el) = shape_cache.circle_element(
                renderer,
                30,
                power_fill,
                Some(RGBA_BORDER_HIGHLIGHT),
                Point::from((right_x + 8, py + 70)),
            ) {
                out.push(CompositorRenderElement::Memory(el));
            }

            // Leading SDF speaker glyph inside Sound capsule slider
            let track_w = inner_w - 24;
            let track_x = left_x + 12;
            let track_y = py + 158;
            let fill_w = (track_w * i32::from(qs.volume_percent.min(100))) / 100;
            let vol_glyph_rgba = if fill_w > 24 {
                [22, 16, 35, 255]
            } else {
                [244, 240, 250, 255]
            };
            if let Some(el) = shape_cache.ui_glyph_element(
                renderer,
                crate::surfaces::UiGlyphKind::Volume {
                    level_percent: qs.volume_percent,
                    muted: qs.audio_muted,
                },
                14,
                vol_glyph_rgba,
                Point::from((track_x + 5, track_y + 2)),
            ) {
                out.push(CompositorRenderElement::Memory(el));
            }

            // Sound control pill buttons
            let btn_y = py + 128;
            let mute_x = px + pw - pad - 116;
            let minus_x = px + pw - pad - 62;
            let plus_x = px + pw - pad - 30;
            push_rounded_glass_box(
                &mut out,
                shape_cache,
                renderer,
                Rectangle::new(Point::from((mute_x, btn_y)), Size::from((48, 22))),
                11,
                if qs.audio_muted {
                    RGBA_DANGER
                } else {
                    RGBA_SURFACE
                },
                Some(RGBA_BORDER),
            );
            push_rounded_glass_box(
                &mut out,
                shape_cache,
                renderer,
                Rectangle::new(Point::from((minus_x, btn_y)), Size::from((28, 22))),
                11,
                RGBA_SURFACE,
                Some(RGBA_BORDER),
            );
            push_rounded_glass_box(
                &mut out,
                shape_cache,
                renderer,
                Rectangle::new(Point::from((plus_x, btn_y)), Size::from((28, 22))),
                11,
                RGBA_SURFACE,
                Some(RGBA_BORDER),
            );

            // Sound capsule slider fill & track (`DESIGN.md` §12.4)
            if fill_w > 0 {
                push_rounded_glass_box(
                    &mut out,
                    shape_cache,
                    renderer,
                    Rectangle::new(Point::from((track_x, track_y)), Size::from((fill_w, 18))),
                    fill_w.min(9),
                    if qs.audio_muted {
                        RGBA_SUBTLE
                    } else {
                        RGBA_ACCENT
                    },
                    None,
                );
            }
            push_rounded_glass_box(
                &mut out,
                shape_cache,
                renderer,
                Rectangle::new(Point::from((track_x, track_y)), Size::from((track_w, 18))),
                9,
                RGBA_SURFACE,
                Some(RGBA_BORDER),
            );

            // Sound card
            push_rounded_glass_box(
                &mut out,
                shape_cache,
                renderer,
                Rectangle::new(Point::from((left_x, py + 122)), Size::from((inner_w, 66))),
                16,
                RGBA_SURFACE_OVERLAY,
                Some(RGBA_BORDER),
            );

            // Glass Surface card (`RUSTYOS-048`): toggle + capsule opacity slider (`0.35..=1.0`)
            let glass_btn_x = px + pw - pad - 86;
            push_rounded_glass_box(
                &mut out,
                shape_cache,
                renderer,
                Rectangle::new(Point::from((glass_btn_x, py + 200)), Size::from((76, 22))),
                11,
                if qs.reduce_transparency {
                    RGBA_ACCENT_MUTED
                } else {
                    RGBA_SURFACE
                },
                Some(if qs.reduce_transparency {
                    RGBA_ACCENT
                } else {
                    RGBA_BORDER
                }),
            );
            let glass_norm = (crate::config::clamp_glass_opacity(qs.glass_opacity)
                - crate::config::MIN_GLASS_OPACITY)
                / (1.0 - crate::config::MIN_GLASS_OPACITY);
            #[allow(
                clippy::cast_precision_loss,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss
            )]
            let glass_fill_w = ((track_w as f32) * glass_norm.clamp(0.0, 1.0)).round() as i32;
            let glass_track_y = py + 228;
            if glass_fill_w > 0 {
                push_rounded_glass_box(
                    &mut out,
                    shape_cache,
                    renderer,
                    Rectangle::new(
                        Point::from((track_x, glass_track_y)),
                        Size::from((glass_fill_w, 16)),
                    ),
                    glass_fill_w.min(8),
                    if qs.reduce_transparency {
                        RGBA_SUBTLE
                    } else {
                        RGBA_ACCENT_HOVER
                    },
                    None,
                );
            }
            push_rounded_glass_box(
                &mut out,
                shape_cache,
                renderer,
                Rectangle::new(
                    Point::from((track_x, glass_track_y)),
                    Size::from((track_w, 16)),
                ),
                8,
                RGBA_SURFACE,
                Some(RGBA_BORDER),
            );
            push_rounded_glass_box(
                &mut out,
                shape_cache,
                renderer,
                Rectangle::new(Point::from((left_x, py + 196)), Size::from((inner_w, 56))),
                16,
                RGBA_SURFACE_OVERLAY,
                Some(RGBA_BORDER),
            );

            // Network & Power cards
            push_rounded_glass_box(
                &mut out,
                shape_cache,
                renderer,
                Rectangle::new(Point::from((left_x, py + 56)), Size::from((half_w, 58))),
                16,
                RGBA_SURFACE_OVERLAY,
                Some(RGBA_BORDER),
            );
            push_rounded_glass_box(
                &mut out,
                shape_cache,
                renderer,
                Rectangle::new(Point::from((right_x, py + 56)), Size::from((half_w, 58))),
                16,
                RGBA_SURFACE_OVERLAY,
                Some(RGBA_BORDER),
            );

            // Quick Action toggles (2 columns, 3 rows shifted below Glass Surface card)
            let is_tile = qs.layout_mode == "master-stack";
            push_rounded_glass_box(
                &mut out,
                shape_cache,
                renderer,
                Rectangle::new(Point::from((left_x, py + 260)), Size::from((half_w, 34))),
                14,
                if is_tile {
                    RGBA_ACCENT_MUTED
                } else {
                    RGBA_SURFACE_OVERLAY
                },
                Some(if is_tile { RGBA_ACCENT } else { RGBA_BORDER }),
            );
            push_rounded_glass_box(
                &mut out,
                shape_cache,
                renderer,
                Rectangle::new(Point::from((right_x, py + 260)), Size::from((half_w, 34))),
                14,
                if qs.reduced_motion {
                    RGBA_ACCENT_MUTED
                } else {
                    RGBA_SURFACE_OVERLAY
                },
                Some(if qs.reduced_motion {
                    RGBA_ACCENT
                } else {
                    RGBA_BORDER
                }),
            );
            push_rounded_glass_box(
                &mut out,
                shape_cache,
                renderer,
                Rectangle::new(Point::from((left_x, py + 302)), Size::from((half_w, 34))),
                14,
                RGBA_SURFACE_OVERLAY,
                Some(RGBA_BORDER),
            );
            push_rounded_glass_box(
                &mut out,
                shape_cache,
                renderer,
                Rectangle::new(Point::from((right_x, py + 302)), Size::from((half_w, 34))),
                14,
                RGBA_SURFACE_OVERLAY,
                Some(RGBA_BORDER),
            );
            let bt_active = qs.bt_available && qs.bt_powered;
            push_rounded_glass_box(
                &mut out,
                shape_cache,
                renderer,
                Rectangle::new(Point::from((left_x, py + 344)), Size::from((half_w, 34))),
                14,
                if bt_active {
                    RGBA_ACCENT_MUTED
                } else {
                    RGBA_SURFACE_OVERLAY
                },
                Some(if bt_active { RGBA_ACCENT } else { RGBA_BORDER }),
            );
            push_rounded_glass_box(
                &mut out,
                shape_cache,
                renderer,
                Rectangle::new(Point::from((right_x, py + 344)), Size::from((half_w, 34))),
                14,
                RGBA_SURFACE_OVERLAY,
                Some(RGBA_BORDER),
            );

            // Hairline separator
            out.push(CompositorRenderElement::Solid(make_solid_element(
                Rectangle::new(Point::from((left_x, py + 50)), Size::from((inner_w, 1))),
                COLOR_BORDER,
            )));

            // Outer glass popover (radius: 20, soft drop shadow, opacity scaled by Control Center slider)
            let qs_rect = Rectangle::new(Point::from((px, py)), Size::from((pw, ph)));
            push_rounded_glass_box(
                &mut out,
                shape_cache,
                renderer,
                qs_rect,
                WINDOW_RADIUS,
                scale_glass_rgba(
                    RGBA_SURFACE_ELEVATED,
                    qs.glass_opacity,
                    qs.reduce_transparency,
                ),
                Some(RGBA_BORDER_HIGHLIGHT),
            );
            push_soft_shadow(
                &mut out,
                shape_cache,
                renderer,
                qs_rect,
                WINDOW_RADIUS,
                24,
                102,
            );
        }
        ShellOverlayView::Switcher(sw) => {
            let (panel_rect, cards) = switcher_layout(screen_size, sw.cards.len());
            for (idx, (card_view, card)) in sw.cards.iter().zip(cards).enumerate() {
                let is_sel = idx == sw.selected;
                let icon_loc = Point::from((card.loc.x + (card.size.w - 36) / 2, card.loc.y + 18));
                let sw_key = format!("win-{}", card_view.window_id.0);
                if let Some(icon_el) = shape_cache.icons.window_icon_element(
                    renderer,
                    &sw_key,
                    card_view.toplevel_icon,
                    card_view.icon_name,
                    36,
                    icon_loc,
                ) {
                    out.push(resolved_icon_to_element(icon_el));
                } else {
                    let well_rect = Rectangle::new(
                        Point::from((card.loc.x + (card.size.w - 44) / 2, card.loc.y + 14)),
                        Size::from((44, 44)),
                    );
                    push_rounded_glass_box(
                        &mut out,
                        shape_cache,
                        renderer,
                        well_rect,
                        10,
                        if is_sel {
                            RGBA_ACCENT
                        } else {
                            RGBA_ACCENT_MUTED
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

                push_rounded_glass_box(
                    &mut out,
                    shape_cache,
                    renderer,
                    card,
                    10,
                    if is_sel {
                        RGBA_SURFACE_OVERLAY
                    } else {
                        RGBA_SURFACE
                    },
                    Some(if is_sel { RGBA_ACCENT } else { RGBA_BORDER }),
                );
            }

            if panel_rect.size.w > 0 {
                push_rounded_glass_box(
                    &mut out,
                    shape_cache,
                    renderer,
                    panel_rect,
                    14,
                    RGBA_SURFACE_ELEVATED,
                    Some(RGBA_BORDER_HIGHLIGHT),
                );
                push_soft_shadow(&mut out, shape_cache, renderer, panel_rect, 14, 18, 110);
            }
        }
    }

    // 2b. Floating top-right Notification HUD toasts (`RUSTYOS-024`)
    if !matches!(
        overlay,
        ShellOverlayView::Spotlight(_) | ShellOverlayView::QuickSettings(_)
    ) && !notifications.is_empty()
    {
        let rects = notification_toast_rects(screen_size, notifications.len());
        for (toast, card) in notifications.iter().zip(rects) {
            let tx = card.loc.x;
            let ty = card.loc.y;
            let tw = card.size.w;
            let th = card.size.h;

            let stripe_rgba = match toast.urgency {
                rusty_ipc::Urgency::Low => RGBA_SUCCESS,
                rusty_ipc::Urgency::Normal => RGBA_ACCENT,
                rusty_ipc::Urgency::Critical => RGBA_DANGER,
            };
            push_rounded_glass_box(
                &mut out,
                shape_cache,
                renderer,
                Rectangle::new(Point::from((tx + 10, ty + 12)), Size::from((4, th - 24))),
                2,
                stripe_rgba,
                None,
            );
            push_rounded_glass_box(
                &mut out,
                shape_cache,
                renderer,
                Rectangle::new(Point::from((tx + tw - 26, ty + 10)), Size::from((18, 18))),
                5,
                RGBA_SURFACE_OVERLAY,
                Some(RGBA_BORDER),
            );
            push_rounded_glass_box(
                &mut out,
                shape_cache,
                renderer,
                card,
                10,
                RGBA_SURFACE_ELEVATED,
                Some(RGBA_BORDER_HIGHLIGHT),
            );
            push_soft_shadow(&mut out, shape_cache, renderer, card, 10, 12, 88);
        }
    }

    // 3. TopBar SDF glyphs, dynamic left/right clusters, menu popover & translucent gradient scrim (`RUSTYOS-048`)
    let active_app_name = dock_view
        .windows
        .iter()
        .find(|w| w.focused)
        .map_or_else(|| "RustyOS".to_owned(), |w| extract_app_name(&w.title));
    let left_layout = compute_top_bar_left_layout(&active_app_name);
    let right_layout = compute_top_bar_right_layout(
        width,
        top_telemetry.has_battery,
        top_telemetry.notification_count,
        top_telemetry.clock_hhmm,
    );

    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        crate::surfaces::UiGlyphKind::RustyLogo,
        16,
        [200, 165, 255, 255],
        Point::from((TOP_BAR_LOGO_X + 1, TOP_BAR_ITEM_Y + 2)),
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

    if let Some(open_menu) = top_telemetry.active_menu {
        let (anchor_x, anchor_w) = match open_menu {
            TopBarMenuHit::SystemLogo => (TOP_BAR_LOGO_X - 2, TOP_BAR_LOGO_W + 4),
            TopBarMenuHit::ActiveApp => (left_layout.app_x - 4, left_layout.app_w + 4),
            TopBarMenuHit::File => (left_layout.file_x - 2, TOP_BAR_MENU_FILE_W + 4),
            TopBarMenuHit::Edit => (left_layout.edit_x - 2, TOP_BAR_MENU_EDIT_W + 4),
            TopBarMenuHit::View => (left_layout.view_x - 2, TOP_BAR_MENU_VIEW_W + 4),
            TopBarMenuHit::Window => (left_layout.win_x - 2, TOP_BAR_MENU_WINDOW_W + 4),
        };
        push_rounded_glass_box(
            &mut out,
            shape_cache,
            renderer,
            Rectangle::new(
                Point::from((anchor_x, TOP_BAR_ITEM_Y)),
                Size::from((anchor_w, TOP_BAR_ITEM_H)),
            ),
            7,
            [255, 255, 255, 34],
            None,
        );
        let (mx, my, mw, mh) = top_bar_menu_popover_rect(&active_app_name, open_menu);
        let menu_rect = Rectangle::new(Point::from((mx, my)), Size::from((mw, mh)));
        push_rounded_glass_box(
            &mut out,
            shape_cache,
            renderer,
            menu_rect,
            12,
            scale_glass_rgba(
                RGBA_SURFACE_ELEVATED,
                top_telemetry.glass_opacity,
                top_telemetry.reduce_transparency,
            ),
            Some(RGBA_BORDER_HIGHLIGHT),
        );
        push_soft_shadow(&mut out, shape_cache, renderer, menu_rect, 12, 18, 96);
    }

    let tray_rects = tray_slot_rects_with_layout(
        width,
        top_telemetry.has_battery,
        top_telemetry.notification_count,
        top_telemetry.clock_hhmm,
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
                    shape_cache
                        .icons
                        .icon_element_by_name(renderer, item.icon_name, 16, icon_loc)
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

    if top_telemetry.notification_count > 0 {
        push_rounded_glass_box(
            &mut out,
            shape_cache,
            renderer,
            Rectangle::new(
                Point::from((right_layout.notif_x, TOP_BAR_ITEM_Y)),
                Size::from((right_layout.notif_w, TOP_BAR_ITEM_H)),
            ),
            10,
            RGBA_ACCENT_MUTED,
            Some(RGBA_ACCENT),
        );
    }
    if top_telemetry.quick_settings_open {
        push_rounded_glass_box(
            &mut out,
            shape_cache,
            renderer,
            Rectangle::new(
                Point::from((right_layout.status_pill_x, TOP_BAR_ITEM_Y)),
                Size::from((right_layout.status_pill_w, TOP_BAR_ITEM_H)),
            ),
            10,
            [255, 255, 255, 42],
            None,
        );
    }

    // Hairline divider before compact workspace indicators
    out.push(CompositorRenderElement::Solid(make_solid_element(
        Rectangle::new(
            Point::from((left_layout.separator_x, 7)),
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

    push_rounded_glass_box(
        &mut out,
        shape_cache,
        renderer,
        Rectangle::new(
            Point::from((left_layout.layout_x, ws_y)),
            Size::from((left_layout.layout_w, ws_h)),
        ),
        9,
        [255, 255, 255, 14],
        (layout_mode == "master-stack").then_some(RGBA_ACCENT),
    );

    // Soft top translucent glass scrim scaled by glass_opacity & reduce_transparency (`RUSTYOS-048`)
    for (offset, scrim_h, alpha) in [(0, 10, 118_u8), (10, 8, 76), (18, TOP_BAR_HEIGHT - 18, 36)] {
        let scaled = scale_glass_rgba(
            [15, 10, 25, alpha],
            top_telemetry.glass_opacity,
            top_telemetry.reduce_transparency,
        );
        out.push(CompositorRenderElement::Solid(make_solid_element(
            Rectangle::new(Point::from((0, offset)), Size::from((width, scrim_h))),
            rgba_to_premul_color32f(scaled),
        )));
    }

    // 4. Dock macOS 27 glass capsule, vertical separator, hover lift, squircles & soft drop shadow (`RUSTYOS-048`)
    let (dock_x, dock_y, dock_width, dock_items_count) =
        dock_layout_bounds(width, height, dock_view.windows.len());

    if !dock_view.windows.is_empty()
        && let Some(ov_slot) = dock_slot_rect(width, height, dock_view.windows.len(), 2)
    {
        let sep_x = ov_slot.loc.x + ov_slot.size.w + DOCK_ITEM_GAP / 2 + DOCK_DIVIDER_GAP / 2;
        out.push(CompositorRenderElement::Solid(make_solid_element(
            Rectangle::new(Point::from((sep_x, dock_y + 18)), Size::from((1, 36))),
            COLOR_BORDER,
        )));
    }

    for i in 0..dock_items_count {
        let Some(slot_rect) = dock_slot_rect(width, height, dock_view.windows.len(), i) else {
            continue;
        };
        let item_x = slot_rect.loc.x;
        let item_size = slot_rect.size.w;
        let hovered = dock_view.hover_slot == Some(i);
        #[allow(clippy::cast_possible_truncation)]
        let lift = if hovered {
            if dock_view.reduced_motion {
                2
            } else {
                (6.0 * dock_view.hover_progress.clamp(0.35, 1.0)).round() as i32
            }
        } else {
            0
        };
        let item_y = slot_rect.loc.y - lift;

        let win_entry = (i >= 3).then(|| dock_view.windows.get(i - 3)).flatten();
        let is_minimized = win_entry.is_some_and(|w| w.minimized);
        let is_active = match i {
            2 => dock_view.overview_active,
            n if n >= 3 => win_entry.is_some_and(|w| w.focused && !w.minimized),
            _ => false,
        };
        let has_running_dot = win_entry.is_some() || (i == 2 && dock_view.overview_active);
        if has_running_dot {
            let pip_color = if is_minimized {
                RGBA_WARNING
            } else if is_active {
                RGBA_ACCENT
            } else {
                RGBA_ACCENT_MUTED
            };
            if let Some(dot) = shape_cache.circle_element(
                renderer,
                4,
                pip_color,
                None,
                Point::from((item_x + (item_size - 4) / 2, dock_y + DOCK_HEIGHT - 7)),
            ) {
                out.push(CompositorRenderElement::Memory(dot));
            }
        }

        let inner_size = DOCK_ICON_SIZE;
        let inner_offset = (item_size - inner_size) / 2;
        let icon_loc = Point::from((item_x + inner_offset, item_y + inner_offset - 2));
        let slot_icon_el = match i {
            0 => shape_cache
                .icons
                .builtin_squircle_icon_element(
                    renderer,
                    crate::icons::BuiltinAppIcon::Terminal,
                    inner_size,
                    icon_loc,
                )
                .map(|el| crate::icons::ResolvedIconElement::Memory(Box::new(el))),
            1 => shape_cache
                .icons
                .builtin_squircle_icon_element(
                    renderer,
                    crate::icons::BuiltinAppIcon::Files,
                    inner_size,
                    icon_loc,
                )
                .map(|el| crate::icons::ResolvedIconElement::Memory(Box::new(el))),
            2 => shape_cache
                .icons
                .builtin_squircle_icon_element(
                    renderer,
                    crate::icons::BuiltinAppIcon::Overview,
                    inner_size,
                    icon_loc,
                )
                .map(|el| crate::icons::ResolvedIconElement::Memory(Box::new(el))),
            _ => win_entry.and_then(|w| {
                let key = format!("win-{}", w.id.0);
                shape_cache
                    .icons
                    .window_icon_element(
                        renderer,
                        &key,
                        w.toplevel_icon.as_ref(),
                        w.icon_name.as_deref(),
                        inner_size,
                        icon_loc,
                    )
                    .or_else(|| {
                        let hue_seed = title_hue_seed(&w.title, w.id.0);
                        shape_cache
                            .icons
                            .builtin_squircle_icon_element(
                                renderer,
                                crate::icons::BuiltinAppIcon::WindowFallback {
                                    hue_seed,
                                    active: is_active,
                                },
                                inner_size,
                                icon_loc,
                            )
                            .map(|el| crate::icons::ResolvedIconElement::Memory(Box::new(el)))
                    })
            }),
        };
        if let Some(el) = slot_icon_el {
            out.push(resolved_icon_to_element(el));
        }

        if hovered || is_active {
            push_rounded_glass_box(
                &mut out,
                shape_cache,
                renderer,
                Rectangle::new(
                    Point::from((item_x, item_y)),
                    Size::from((item_size, item_size)),
                ),
                16,
                if hovered {
                    [255, 255, 255, 32]
                } else {
                    [255, 255, 255, 18]
                },
                None,
            );
        }
    }

    let dock_rect = Rectangle::new(
        Point::from((dock_x, dock_y)),
        Size::from((dock_width, DOCK_HEIGHT)),
    );
    push_rounded_glass_box(
        &mut out,
        shape_cache,
        renderer,
        dock_rect,
        DOCK_RADIUS,
        scale_glass_rgba(
            [42, 32, 64, 212],
            top_telemetry.glass_opacity,
            top_telemetry.reduce_transparency,
        ),
        Some(RGBA_BORDER_HIGHLIGHT),
    );
    push_soft_shadow(
        &mut out,
        shape_cache,
        renderer,
        dock_rect,
        DOCK_RADIUS,
        20,
        92,
    );

    // 5. Workspace Overview cards & scrim rendered below TopBar & Dock so both stay crisp and clickable (`RUSTYOS-023`/`025`)
    if let ShellOverlayView::Overview(ov) = overlay {
        let card_dy = anim_offset(ov.progress, 22.0);
        let cards = overview_card_rects(screen_size, ov.windows.len());
        for (win, base_card) in ov.windows.iter().zip(cards) {
            let card = Rectangle::new(
                Point::from((base_card.loc.x, base_card.loc.y + card_dy)),
                base_card.size,
            );
            let preview_rect = Rectangle::new(
                Point::from((card.loc.x + 12, card.loc.y + 32)),
                Size::from((card.size.w - 24, card.size.h - 44)),
            );
            push_rounded_glass_box(
                &mut out,
                shape_cache,
                renderer,
                preview_rect,
                8,
                if win.focused {
                    RGBA_ACCENT_MUTED
                } else {
                    RGBA_SURFACE_OVERLAY
                },
                Some(RGBA_BORDER),
            );
            push_rounded_glass_box(
                &mut out,
                shape_cache,
                renderer,
                card,
                12,
                RGBA_SURFACE_ELEVATED,
                Some(if win.focused {
                    RGBA_ACCENT
                } else {
                    RGBA_BORDER_HIGHLIGHT
                }),
            );
            push_soft_shadow(&mut out, shape_cache, renderer, card, 12, 14, 96);
        }
        #[allow(clippy::cast_possible_truncation)]
        let scrim_a = 0.88 * (ov.progress.clamp(0.0, 1.0) as f32);
        out.push(CompositorRenderElement::Solid(make_solid_element(
            Rectangle::new(
                Point::from((0, TOP_BAR_HEIGHT)),
                Size::from((width, (height - TOP_BAR_HEIGHT).max(0))),
            ),
            premul(0.059, 0.039, 0.098, scrim_a),
        )));
    }

    out
}

/// Generates topmost front-to-back Lock Screen curtain, clock, and unlock button (`RUSTYOS-025`).
#[allow(clippy::cast_possible_truncation, clippy::too_many_lines)]
pub fn generate_lock_screen_elements(
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    lock: &LockScreenRenderState<'_>,
) -> Vec<CompositorRenderElement> {
    let mut out = Vec::new();
    let p = lock.progress.clamp(0.0, 1.0);
    if p <= 0.01 {
        return out;
    }

    let width = screen_size.w;
    let height = screen_size.h;
    let (cx, base_cy, cw, ch) = lock_screen_card_rect(screen_size);
    let card_dy = anim_offset(p, 24.0);
    let cy = base_cy + card_dy;
    let btn_rect = lock_screen_unlock_button_rect(screen_size, card_dy);

    let text_bright = [244, 240, 250, 255];
    let text_muted = [166, 155, 181, 255];

    if p > 0.2 && text_cache.has_font() {
        if let Some(el) = text_cache.render_label_centered(
            renderer,
            lock.clock_hhmm,
            34.0,
            text_bright,
            Rectangle::new(Point::from((cx + 16, cy + 20)), Size::from((cw - 32, 46))),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
        if let Some(el) = text_cache.render_label_centered(
            renderer,
            "RustyOS Session Locked",
            12.5,
            text_bright,
            Rectangle::new(Point::from((cx + 16, cy + 72)), Size::from((cw - 32, 22))),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
        let status = if lock.has_battery {
            format!(
                "{} • {}% • {} win",
                truncate_ascii(lock.network_label, 14),
                lock.battery_percent,
                lock.window_count
            )
        } else {
            format!(
                "{} • {} windows paused",
                truncate_ascii(lock.network_label, 16),
                lock.window_count
            )
        };
        if let Some(el) = text_cache.render_label_centered(
            renderer,
            &status,
            10.5,
            text_muted,
            Rectangle::new(Point::from((cx + 16, cy + 96)), Size::from((cw - 32, 20))),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
        let btn_logical = Rectangle::new(
            Point::from((btn_rect.loc.x, btn_rect.loc.y)),
            Size::from((btn_rect.size.w, btn_rect.size.h)),
        );
        if let Some(el) = text_cache.render_label_centered(
            renderer,
            "Unlock Session",
            12.5,
            text_bright,
            btn_logical,
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
        if let Some(el) = text_cache.render_label_centered(
            renderer,
            "Press Enter, Space, or click Unlock",
            10.0,
            text_muted,
            Rectangle::new(
                Point::from((cx + 16, cy + ch - 26)),
                Size::from((cw - 32, 18)),
            ),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
    }

    push_rounded_glass_box(
        &mut out,
        shape_cache,
        renderer,
        btn_rect,
        8,
        RGBA_ACCENT,
        Some(RGBA_BORDER_HIGHLIGHT),
    );

    let card_rect = Rectangle::new(Point::from((cx, cy)), Size::from((cw, ch)));
    push_rounded_glass_box(
        &mut out,
        shape_cache,
        renderer,
        card_rect,
        16,
        RGBA_SURFACE_ELEVATED,
        Some(RGBA_BORDER_HIGHLIGHT),
    );
    push_soft_shadow(&mut out, shape_cache, renderer, card_rect, 16, 18, 115);

    let curtain_a = 0.92 * (p as f32);
    out.push(CompositorRenderElement::Solid(make_solid_element(
        Rectangle::new(Point::from((0, 0)), Size::from((width, height))),
        premul(0.059, 0.039, 0.098, curtain_a),
    )));

    out
}

/// Generates rounded SSD titlebar, circular macOS traffic lights, and 1px frame rim (`RUSTYOS-022`).
#[allow(clippy::many_single_char_names, clippy::too_many_lines)]
pub fn generate_window_ssd_composite_elements(
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    rect: rusty_wm::Rect,
    title: &str,
    is_focused: bool,
) -> Vec<CompositorRenderElement> {
    use crate::surfaces::CornerQuadrant;

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

    // Circular anti-aliased macOS traffic-light buttons
    let btn_y = y + 8;
    let btn_d = 12;
    let close_rgba = if is_focused { RGBA_DANGER } else { RGBA_SUBTLE };
    let min_rgba = if is_focused {
        RGBA_WARNING
    } else {
        RGBA_SUBTLE
    };
    let max_rgba = if is_focused {
        RGBA_SUCCESS
    } else {
        RGBA_SUBTLE
    };
    let rim_opt = is_focused.then_some([255, 255, 255, 45]);

    for (bx, rgba) in [(x + 12, close_rgba), (x + 30, min_rgba), (x + 48, max_rgba)] {
        if let Some(el) =
            shape_cache.circle_element(renderer, btn_d, rgba, rim_opt, Point::from((bx, btn_y)))
        {
            out.push(CompositorRenderElement::Memory(el));
        }
    }

    // Rounded top corners (8px radius) + seamless 1px glass border around window frame
    let r = WINDOW_RADIUS;
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

    if let Some(el) = shape_cache.corner_element(
        renderer,
        r,
        CornerQuadrant::TopLeft,
        titlebar_rgba,
        Some(border_rgba),
        Point::from((x, y)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    if let Some(el) = shape_cache.corner_element(
        renderer,
        r,
        CornerQuadrant::TopRight,
        titlebar_rgba,
        Some(border_rgba),
        Point::from((x + w - r, y)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }

    // Top border between rounded corners & side/bottom 1px frame rim
    if w > 2 * r {
        out.push(CompositorRenderElement::Solid(make_solid_element(
            Rectangle::new(Point::from((x + r, y)), Size::from((w - 2 * r, 1))),
            top_rim_col,
        )));
        out.push(CompositorRenderElement::Solid(make_solid_element(
            Rectangle::new(Point::from((x + r, y + 1)), Size::from((w - 2 * r, r - 1))),
            titlebar_fill,
        )));
    }
    if h > TITLEBAR_HEIGHT {
        out.push(CompositorRenderElement::Solid(make_solid_element(
            Rectangle::new(
                Point::from((x + 1, y + r)),
                Size::from((w - 2, TITLEBAR_HEIGHT - r - 1)),
            ),
            titlebar_fill,
        )));
    }
    // Titlebar bottom divider + side & bottom 1px rim
    out.push(CompositorRenderElement::Solid(make_solid_element(
        Rectangle::new(
            Point::from((x + 1, y + TITLEBAR_HEIGHT - 1)),
            Size::from((w - 2, 1)),
        ),
        COLOR_BORDER,
    )));
    out.push(CompositorRenderElement::Solid(make_solid_element(
        Rectangle::new(Point::from((x, y + r)), Size::from((1, h - r - 1))),
        edge_border_col,
    )));
    out.push(CompositorRenderElement::Solid(make_solid_element(
        Rectangle::new(Point::from((x + w - 1, y + r)), Size::from((1, h - r - 1))),
        edge_border_col,
    )));
    out.push(CompositorRenderElement::Solid(make_solid_element(
        Rectangle::new(Point::from((x, y + h - 1)), Size::from((w, 1))),
        edge_border_col,
    )));

    out
}

/// Computes the centered floating OSD pill bounds right above the bottom Dock (`RUSTYOS-031`).
#[must_use]
pub fn osd_pill_rect(
    screen_size: Size<i32, smithay::utils::Physical>,
    has_bar: bool,
) -> Rectangle<i32, smithay::utils::Physical> {
    let w = 256.min((screen_size.w - 40).max(180));
    let h = if has_bar { 56 } else { 40 };
    let x = (screen_size.w - w) / 2;
    let y = (screen_size.h - DOCK_HEIGHT - DOCK_MARGIN_BOTTOM - 22 - h).max(TOP_BAR_HEIGHT + 12);
    Rectangle::new(Point::from((x, y)), Size::from((w, h)))
}

/// Generates glass OSD indicator elements (`Volume`, `Brightness`, `Media`, `Screenshot`) (`RUSTYOS-031`).
pub fn generate_osd_elements(
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    osd: &OsdRenderState<'_>,
) -> Vec<CompositorRenderElement> {
    let mut out = Vec::new();
    let pill = osd_pill_rect(screen_size, osd.value_percent.is_some());
    let px = pill.loc.x;
    let py = pill.loc.y;
    let pw = pill.size.w;

    let text_bright = [244, 240, 250, 255];
    let text_muted = [166, 155, 181, 255];

    if let Some(pct) = osd.value_percent {
        if let Some(el) = text_cache.render_label(
            renderer,
            osd.label,
            12.0,
            text_bright,
            Point::from((px + 16, py + 11)),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
        let val_text = if osd.muted {
            "Muted".to_owned()
        } else {
            format!("{pct}%")
        };
        let (vw, _) = text_cache.measure_text(&val_text, 12.0);
        if let Some(el) = text_cache.render_label(
            renderer,
            &val_text,
            12.0,
            if osd.muted { text_muted } else { text_bright },
            Point::from((px + pw - 16 - vw, py + 11)),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }

        let bar_x = px + 16;
        let bar_y = py + 36;
        let bar_w = (pw - 32).max(24);
        let bar_h = 6;
        let fill_w = (bar_w * i32::from(pct.min(100))) / 100;
        if fill_w > 0 {
            let fill_col = if osd.muted {
                COLOR_SUBTLE
            } else {
                COLOR_ACCENT
            };
            out.push(CompositorRenderElement::Solid(make_solid_element(
                Rectangle::new(Point::from((bar_x, bar_y)), Size::from((fill_w, bar_h))),
                fill_col,
            )));
        }
        out.push(CompositorRenderElement::Solid(make_solid_element(
            Rectangle::new(Point::from((bar_x, bar_y)), Size::from((bar_w, bar_h))),
            COLOR_SURFACE,
        )));
    } else if let Some(el) = text_cache.render_label_centered(
        renderer,
        osd.label,
        12.5,
        text_bright,
        Rectangle::new(Point::from((px, py)), Size::from((pw, pill.size.h))),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }

    push_rounded_glass_box(
        &mut out,
        shape_cache,
        renderer,
        pill,
        14,
        RGBA_SURFACE_OVERLAY,
        Some(RGBA_BORDER_HIGHLIGHT),
    );
    push_soft_shadow(&mut out, shape_cache, renderer, pill, 14, 12, 84);
    out
}

/// Generates soft radial drop shadow elements strictly outside a managed window rectangle (`RUSTYOS-022`).
pub fn generate_window_shadow_elements(
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    rect: rusty_wm::Rect,
    is_focused: bool,
) -> Vec<CompositorRenderElement> {
    let mut out = Vec::new();
    let w = rect.width.max(64);
    let h = rect.height.max(TITLEBAR_HEIGHT + 32);
    let spread = if is_focused { 28 } else { 16 };
    let max_alpha = if is_focused { 88 } else { 44 };
    push_soft_shadow(
        &mut out,
        shape_cache,
        renderer,
        Rectangle::new(Point::from((rect.x, rect.y)), Size::from((w, h))),
        WINDOW_RADIUS,
        spread,
        max_alpha,
    );
    out
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
            icon: Some("utilities-terminal".to_owned()),
            is_custom_command: false,
        }];
        let spot = SpotlightRenderState {
            query: "foot",
            selected_in_window: 0,
            cursor_offset_x: 32,
            items: &sample_items,
            progress: 1.0,
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
            bt_available: true,
            bt_powered: true,
            bt_label: "BT: On",
            battery_percent: 88,
            has_battery: true,
            ac_online: true,
            volume_percent: 75,
            audio_muted: false,
            frame_p50_us: 16_667,
            window_count: 2,
            progress: 1.0,
            glass_opacity: crate::config::DEFAULT_GLASS_OPACITY,
            reduce_transparency: false,
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
        assert!(check_top_bar_layout_click(515.0, 14.0));
        assert!(!check_top_bar_layout_click(50.0, 14.0));
        assert!(!check_top_bar_layout_click(300.0, 14.0));

        // Test TopBar Left Menu hits (RUSTYOS-038)
        assert_eq!(
            check_top_bar_menu_click(15.0, 14.0),
            Some(TopBarMenuHit::SystemLogo)
        );
        assert_eq!(
            check_top_bar_menu_click(50.0, 14.0),
            Some(TopBarMenuHit::ActiveApp)
        );
        assert_eq!(
            check_top_bar_menu_click(130.0, 14.0),
            Some(TopBarMenuHit::File)
        );
        assert_eq!(
            check_top_bar_menu_click(165.0, 14.0),
            Some(TopBarMenuHit::Edit)
        );
        assert_eq!(
            check_top_bar_menu_click(205.0, 14.0),
            Some(TopBarMenuHit::View)
        );
        assert_eq!(
            check_top_bar_menu_click(250.0, 14.0),
            Some(TopBarMenuHit::Window)
        );
        assert_eq!(check_top_bar_menu_click(300.0, 14.0), None);
        assert_eq!(check_top_bar_menu_click(15.0, 30.0), None);

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
            check_quick_settings_click(size, f64::from(px + 30), f64::from(py + 275)),
            QuickSettingsHit::ToggleLayout
        );
        assert_eq!(
            check_quick_settings_click(size, f64::from(px + 200), f64::from(py + 275)),
            QuickSettingsHit::ToggleReducedMotion
        );
        assert_eq!(
            check_quick_settings_click(size, f64::from(px + 30), f64::from(py + 317)),
            QuickSettingsHit::OpenSpotlight
        );
        assert_eq!(
            check_quick_settings_click(size, f64::from(px + 200), f64::from(py + 317)),
            QuickSettingsHit::RefreshMetrics
        );
        assert_eq!(
            check_quick_settings_click(size, f64::from(px + 30), f64::from(py + 359)),
            QuickSettingsHit::ToggleBluetooth
        );
        assert_eq!(
            check_quick_settings_click(size, f64::from(px + 200), f64::from(py + 359)),
            QuickSettingsHit::LockSession
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
        // Click below titlebar inside client area returns None (must not hit resize edges)
        assert_eq!(
            check_window_ssd_hit(&windows, 300.0, f64::from(100 + TITLEBAR_HEIGHT + 20)),
            None
        );

        // Edge and corner resize hits (`RUSTYOS-022`)
        assert_eq!(
            check_window_ssd_hit(&windows, 300.0, 101.0),
            Some(SsdHit::Resize(
                win2,
                ResizeEdges {
                    top: true,
                    bottom: false,
                    left: false,
                    right: false,
                }
            ))
        );
        assert_eq!(
            check_window_ssd_hit(&windows, 101.0, 300.0),
            Some(SsdHit::Resize(
                win2,
                ResizeEdges {
                    top: false,
                    bottom: false,
                    left: true,
                    right: false,
                }
            ))
        );
        assert_eq!(
            check_window_ssd_hit(&windows, 599.0, 499.0),
            Some(SsdHit::Resize(
                win2,
                ResizeEdges {
                    top: false,
                    bottom: true,
                    left: false,
                    right: true,
                }
            ))
        );
    }

    #[test]
    fn generate_window_ssd_elements_creates_titlebar_traffic_lights_and_borders() {
        let elements =
            generate_window_ssd_elements(rusty_wm::Rect::new(40, 40, 600, 400), true, false);
        // 4 border strips + titlebar bg + divider + 3 traffic lights + title pill = 10 elements
        assert_eq!(elements.len(), 10);
    }

    #[test]
    fn dock_and_overview_hit_testing_match_rendered_slots_and_cards() {
        let size = Size::from((1280, 800));
        let win_count = 2;

        for (slot_idx, expected) in [
            (0, DockHit::PinnedTerminal),
            (1, DockHit::PinnedFiles),
            (2, DockHit::ToggleOverview),
            (3, DockHit::WindowSlot(0)),
            (4, DockHit::WindowSlot(1)),
        ] {
            let slot_rect = dock_slot_rect(size.w, size.h, win_count, slot_idx)
                .unwrap_or_else(|| Rectangle::from_size(Size::from((0, 0))));
            let cx = f64::from(slot_rect.loc.x + slot_rect.size.w / 2);
            let cy = f64::from(slot_rect.loc.y + slot_rect.size.h / 2);
            assert_eq!(check_dock_slot(size, win_count, cx, cy), Some(slot_idx));
            assert_eq!(check_dock_click(size, win_count, cx, cy), Some(expected));
        }

        // Above Dock returns None
        assert_eq!(check_dock_click(size, win_count, 640.0, 200.0), None);

        // Overview cards
        let cards = overview_card_rects(size, 3);
        assert_eq!(cards.len(), 3);
        for (idx, card) in cards.iter().enumerate() {
            let cx = f64::from(card.loc.x + card.size.w / 2);
            let cy = f64::from(card.loc.y + card.size.h / 2);
            assert_eq!(
                check_overview_click(size, 3, cx, cy),
                OverviewHit::Card(idx)
            );
        }
        assert_eq!(
            check_overview_click(size, 3, 10.0, 100.0),
            OverviewHit::Dismiss
        );
        assert_eq!(make_window_badge("RustyOS_Main : bash — Konsole"), "RM");
        assert_eq!(make_window_badge("Cursor Agents"), "CA");
        assert_eq!(extract_app_name("RustyOS_Main : bash — Konsole"), "Konsole");
        assert_eq!(
            extract_app_name("Document.txt — Text Editor"),
            "Text Editor"
        );
        assert_eq!(
            extract_app_name("Inbox - Mozilla Thunderbird"),
            "Mozilla Thund…"
        );
        assert_eq!(extract_app_name("Alacritty"), "Alacritty");
        assert_eq!(extract_app_name(""), "RustyOS");
    }

    #[test]
    fn notification_toast_and_badge_hit_testing_match_rendered_bounds() {
        let size = Size::from((1280, 800));
        let rects = notification_toast_rects(size, 6);
        // Capped at MAX_VISIBLE_NOTIFICATIONS (4)
        assert_eq!(rects.len(), MAX_VISIBLE_NOTIFICATIONS);

        for (idx, rect) in rects.iter().enumerate() {
            let cx = f64::from(rect.loc.x + rect.size.w / 2);
            let cy = f64::from(rect.loc.y + rect.size.h / 2);
            assert_eq!(check_notification_click(size, 4, cx, cy), Some(idx));
        }

        // Outside toasts returns None
        assert_eq!(check_notification_click(size, 4, 100.0, 200.0), None);
        assert_eq!(check_notification_click(size, 0, 1100.0, 60.0), None);

        // TopBar unread notification badge pill hit-testing
        let (bx, by, bw, bh) = notification_badge_rect(size.w);
        assert!(check_top_bar_notification_click(
            size.w,
            2,
            f64::from(bx + bw / 2),
            f64::from(by + bh / 2)
        ));
        assert!(!check_top_bar_notification_click(
            size.w,
            0,
            f64::from(bx + bw / 2),
            f64::from(by + bh / 2)
        ));

        // TopBar keyboard layout pill (`EN`/`RU`) avoids notification badge when present
        let (kbd_x, kbd_y, kbd_w, kbd_h) = kbd_badge_rect(size.w, 0);
        assert!(check_top_bar_kbd_click(
            size.w,
            0,
            f64::from(kbd_x + kbd_w / 2),
            f64::from(kbd_y + kbd_h / 2)
        ));
        let (notif_kbd_x, notif_kbd_y, notif_kbd_w, notif_kbd_h) = kbd_badge_rect(size.w, 2);
        assert!(notif_kbd_x + notif_kbd_w <= bx);
        assert!(check_top_bar_kbd_click(
            size.w,
            2,
            f64::from(notif_kbd_x + notif_kbd_w / 2),
            f64::from(notif_kbd_y + notif_kbd_h / 2)
        ));

        // TopBar system tray pills (`RUSTYOS-032`) sit cleanly left of keyboard badge
        let tray_rects = tray_slot_rects(size.w, 2, 3);
        assert_eq!(tray_rects.len(), 3);
        assert!(tray_rects[2].0 + tray_rects[2].2 <= notif_kbd_x);
        for (idx, (tx, ty, tw, th)) in tray_rects.into_iter().enumerate() {
            assert_eq!(
                check_top_bar_tray_click(
                    size.w,
                    2,
                    3,
                    f64::from(tx + tw / 2),
                    f64::from(ty + th / 2)
                ),
                Some(idx)
            );
        }
        assert_eq!(check_top_bar_tray_click(size.w, 2, 3, 20.0, 14.0), None);
    }

    #[test]
    fn lock_screen_hit_testing_and_animation_offsets_match_bounds() {
        let size = Size::from((1280, 800));
        let btn = lock_screen_unlock_button_rect(size, 0);
        assert!(check_lock_screen_unlock_click(
            size,
            f64::from(btn.loc.x + btn.size.w / 2),
            f64::from(btn.loc.y + btn.size.h / 2)
        ));
        assert!(!check_lock_screen_unlock_click(size, 40.0, 40.0));

        assert_eq!(anim_offset(1.0, -22.0), 0);
        assert_eq!(anim_offset(0.0, -22.0), -22);
    }

    #[test]
    fn csd_frame_hit_testing_ignores_client_header_and_preserves_outer_resize() {
        let win = SsdHitWindow {
            id: rusty_wm::WindowId(42),
            rect: rusty_wm::Rect::new(100, 80, 600, 420),
            has_ssd: false,
        };
        let list = [win];

        // Interior header spots (macOS traffic-light zone, titlebar interior, corner close button)
        // must pass through to the CSD application.
        assert_eq!(check_window_frame_hit(&list, 118.0, 94.0), None);
        assert_eq!(check_window_frame_hit(&list, 350.0, 94.0), None);
        assert_eq!(check_window_frame_hit(&list, 692.0, 94.0), None);

        // Outer border band outside the client surface still triggers resize.
        assert_eq!(
            check_window_frame_hit(&list, 97.0, 200.0),
            Some(SsdHit::Resize(
                rusty_wm::WindowId(42),
                ResizeEdges {
                    left: true,
                    right: false,
                    top: false,
                    bottom: false,
                }
            ))
        );
    }

    #[test]
    fn osd_pill_bounds_center_above_dock() {
        let out = Size::from((1280, 800));
        let bar_pill = osd_pill_rect(out, true);
        let msg_pill = osd_pill_rect(out, false);
        assert_eq!(bar_pill.size.w, 256);
        assert_eq!(bar_pill.size.h, 56);
        assert_eq!(msg_pill.size.h, 40);
        assert_eq!(bar_pill.loc.x, (1280 - 256) / 2);
        assert!(bar_pill.loc.y + bar_pill.size.h < 800 - DOCK_HEIGHT - DOCK_MARGIN_BOTTOM);
    }

    #[test]
    fn switcher_layout_and_click_hit_testing_match_bounds() {
        let out = Size::from((1280, 800));
        let (panel, cards) = switcher_layout(out, 3);
        assert_eq!(cards.len(), 3);
        assert!(panel.size.w > cards[2].size.w * 3);
        assert_eq!(panel.loc.x + panel.size.w / 2, 640);
        for (idx, rect) in cards.iter().enumerate() {
            assert_eq!(
                check_switcher_click(
                    out,
                    3,
                    f64::from(rect.loc.x + rect.size.w / 2),
                    f64::from(rect.loc.y + rect.size.h / 2)
                ),
                Some(idx)
            );
        }
        assert_eq!(check_switcher_click(out, 3, 20.0, 20.0), None);
    }

    #[test]
    fn macos27_quick_settings_glass_and_volume_slider_hit_testing() {
        let size = Size::from((1280, 800));
        let (px, py, pw, _ph) = quick_settings_rect(size.w, size.h);
        let pad = 14;
        let track_x = f64::from(px + pad + 12);
        let track_w = f64::from(pw - pad * 2 - 24);

        assert_eq!(
            check_quick_settings_click(size, track_x + track_w * 0.5, f64::from(py + 168)),
            QuickSettingsHit::SetVolumePercent(50)
        );
        assert_eq!(
            check_quick_settings_click(size, f64::from(px + pw - pad - 40), f64::from(py + 212)),
            QuickSettingsHit::ToggleReduceTransparency
        );
        assert_eq!(
            check_quick_settings_click(size, track_x + 1.0, f64::from(py + 238)),
            QuickSettingsHit::SetGlassOpacityPercent(35)
        );
        assert_eq!(
            check_quick_settings_click(size, track_x + track_w - 1.0, f64::from(py + 238)),
            QuickSettingsHit::SetGlassOpacityPercent(100)
        );

        let tinted = scale_glass_rgba([20, 14, 34, 200], 0.50, false);
        assert!(tinted[3] < 200 && tinted[3] >= 40);
        let opaque = scale_glass_rgba([20, 14, 34, 180], 0.40, true);
        assert!(opaque[3] >= 246);
    }

    #[test]
    fn macos27_top_bar_dynamic_left_right_and_menu_popover_hit_testing() {
        let without_bat = compute_top_bar_right_layout(1280, false, 0, "Thu 13 Nov  00:15");
        let with_bat = compute_top_bar_right_layout(1280, true, 0, "Thu 13 Nov  00:15");
        assert!(without_bat.battery_x.is_none());
        assert!(with_bat.battery_x.is_some());
        assert!(without_bat.volume_x > with_bat.volume_x);
        assert!(check_top_bar_search_click_for_telemetry(
            1280,
            false,
            0,
            "Thu 13 Nov  00:15",
            f64::from(without_bat.search_x + without_bat.search_w / 2),
            14.0,
        ));

        let short_left = compute_top_bar_left_layout("RustyOS");
        let wide_left = compute_top_bar_left_layout("Visual Studio Code");
        assert!(wide_left.app_w > short_left.app_w);
        assert!(wide_left.ws_start_x > short_left.ws_start_x);
        assert_eq!(
            check_top_bar_workspace_click_for_app(
                "Visual Studio Code",
                f64::from(wide_left.ws_start_x + 4),
                14.0
            ),
            Some(1)
        );
        assert!(check_top_bar_layout_click_for_app(
            "Visual Studio Code",
            f64::from(wide_left.layout_x + 10),
            14.0
        ));

        let (mx, my, _mw, _mh) = top_bar_menu_popover_rect("RustyOS", TopBarMenuHit::File);
        assert_eq!(
            check_top_bar_menu_popover_click(
                "RustyOS",
                TopBarMenuHit::File,
                f64::from(mx + 24),
                f64::from(my + 16),
            ),
            TopBarMenuPopoverClick::Action(TopBarMenuAction::NewTerminal)
        );
        assert_eq!(
            check_top_bar_menu_popover_click("RustyOS", TopBarMenuHit::File, 800.0, 400.0),
            TopBarMenuPopoverClick::Outside
        );
    }

    #[test]
    fn macos27_dock_divider_gap_and_radii_constants() {
        assert_eq!(DOCK_RADIUS, 36);
        assert_eq!(WINDOW_RADIUS, 20);
        let overview_slot_opt = dock_slot_rect(1280, 800, 2, 2);
        let first_win_slot_opt = dock_slot_rect(1280, 800, 2, 3);
        assert!(overview_slot_opt.is_some());
        assert!(first_win_slot_opt.is_some());
        let overview_slot =
            overview_slot_opt.unwrap_or_else(|| Rectangle::from_size(Size::from((0, 0))));
        let first_win_slot =
            first_win_slot_opt.unwrap_or_else(|| Rectangle::from_size(Size::from((0, 0))));
        assert_eq!(
            first_win_slot.loc.x - (overview_slot.loc.x + overview_slot.size.w),
            DOCK_ITEM_GAP + DOCK_DIVIDER_GAP
        );
    }
}
