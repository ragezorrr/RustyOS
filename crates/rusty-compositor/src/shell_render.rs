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

/// `TopBar` Left Menu metrics (`macOS` Menu Bar layout with `Inter` 13 px) (`RUSTYOS-038`/`049`).
pub const TOP_BAR_LOGO_X: i32 = 12;
pub const TOP_BAR_LOGO_W: i32 = 20;
pub const TOP_BAR_APP_X: i32 = 36;

/// Workspace button width, stride, and default start offset when optional strip is enabled (`RUSTYOS-038`/`049`).
pub const WS_BTN_WIDTH: i32 = 18;
pub const WS_BTN_GAP: i32 = 3;
pub const WS_BTN_STRIDE: i32 = WS_BTN_WIDTH + WS_BTN_GAP;

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

/// Visibility and telemetry flags controlling optional items in the `TopBar` (`RUSTYOS-049`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(clippy::struct_excessive_bools)]
pub struct TopBarLayoutOptions {
    /// Whether a hardware battery is present.
    pub has_battery: bool,
    /// Active unread desktop notification count.
    pub notification_count: u32,
    /// Show standalone volume glyph left of Battery/Wi-Fi.
    pub show_volume: bool,
    /// Show keyboard input source indicator (`EN`/`RU`).
    pub show_input_source: bool,
    /// Show workspace numbers `1..=9` and layout badge.
    pub show_workspaces: bool,
    /// Show `StatusNotifierItem` tray applets.
    pub show_tray: bool,
}

/// Live hardware/session telemetry displayed inside `TopBar` (`RUSTYOS-021`, `RUSTYOS-048`, `RUSTYOS-049`).
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct TopBarTelemetry<'a> {
    /// Local wall-clock date/time label shown in `TopBar` (`"Wed Apr 1  9:41 AM"`).
    pub clock_hhmm: &'a str,
    /// Battery percentage (`0..=100`).
    pub battery_percent: u8,
    /// Whether a hardware battery is present.
    pub has_battery: bool,
    /// Whether AC adapter is plugged in (`RUSTYOS-049`).
    pub ac_online: bool,
    /// Whether battery is actively charging (`RUSTYOS-052`).
    pub battery_charging: bool,
    /// Whether an active network interface is up.
    pub net_connected: bool,
    /// Live Wi-Fi signal strength percentage (`0..=100`, `RUSTYOS-052`).
    pub wifi_signal_percent: Option<u8>,
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
    /// Registered `StatusNotifierItem` tray badges rendered when `show_tray == true` (`RUSTYOS-032`).
    pub tray_items: &'a [TopBarTrayItem<'a>],
    /// Currently open `TopBar` left menu dropdown if any (`RUSTYOS-048`).
    pub active_menu: Option<TopBarMenuHit>,
    /// Currently hovered item index (`0..`) inside the open `TopBar` dropdown (`RUSTYOS-051`).
    pub menu_hover_row: Option<usize>,
    /// Global shell glass opacity multiplier (`0.35..=1.0`, default `0.78`) (`RUSTYOS-048`).
    pub glass_opacity: f32,
    /// High-contrast opaque shell panels mode (`RUSTYOS-048`).
    pub reduce_transparency: bool,
    /// Show workspace digits `1..=9` and `Float`/`Tile` pill (`RUSTYOS-049`, default `false`).
    pub show_workspaces: bool,
    /// Show host SNI system tray icons in Menu Bar (`RUSTYOS-049`, default `false`).
    pub show_tray: bool,
    /// Show keyboard layout badge in Menu Bar (`RUSTYOS-049`, default `false`).
    pub show_input_source: bool,
    /// Show standalone speaker volume icon in Menu Bar (`RUSTYOS-049`, default `false`).
    pub show_volume: bool,
}

impl TopBarTelemetry<'_> {
    /// Extracts [`TopBarLayoutOptions`] matching the active telemetry and UI toggles.
    #[must_use]
    pub const fn layout_options(&self) -> TopBarLayoutOptions {
        TopBarLayoutOptions {
            has_battery: self.has_battery,
            notification_count: self.notification_count,
            show_volume: self.show_volume,
            show_input_source: self.show_input_source,
            show_workspaces: self.show_workspaces,
            show_tray: self.show_tray,
        }
    }
}

impl Default for TopBarTelemetry<'static> {
    fn default() -> Self {
        Self {
            clock_hhmm: "Wed Apr 1  9:41 AM",
            battery_percent: 100,
            has_battery: false,
            ac_online: false,
            battery_charging: false,
            net_connected: true,
            wifi_signal_percent: Some(100),
            volume_percent: 75,
            audio_muted: false,
            quick_settings_open: false,
            notification_count: 0,
            kbd_layout: "EN",
            tray_items: &[],
            active_menu: None,
            menu_hover_row: None,
            glass_opacity: crate::config::DEFAULT_GLASS_OPACITY,
            reduce_transparency: false,
            show_workspaces: false,
            show_tray: false,
            show_input_source: false,
            show_volume: false,
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
    /// True when the pointer is hovering this notification card (`RUSTYOS-051`).
    pub hovered: bool,
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
    /// Live Wi-Fi radio enabled flag (`RUSTYOS-053`).
    pub wifi_enabled: bool,
    /// Live Wi-Fi signal strength percentage (`0..=100`, `RUSTYOS-052`).
    pub wifi_signal_percent: Option<u8>,
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
    /// Whether battery is actively charging (`RUSTYOS-052`).
    pub battery_charging: bool,
    /// Whether sysfs display backlight control is available (`RUSTYOS-053`).
    pub brightness_available: bool,
    /// Current hardware backlight brightness percentage (`0..=100`, `RUSTYOS-053`).
    pub brightness_percent: u8,
    /// Master output volume percentage (`0..=100`).
    pub volume_percent: u8,
    /// Master audio mute flag.
    pub audio_muted: bool,
    /// Whether an active MPRIS2 media player is registered (`RUSTYOS-053`).
    pub media_active: bool,
    /// Whether active MPRIS2 media playback is currently playing (`RUSTYOS-053`).
    pub media_playing: bool,
    /// Active MPRIS2 track title (empty when nothing is playing) (`RUSTYOS-053`).
    pub media_title: &'a str,
    /// Whether Focus (Do Not Disturb) notification suppression is active (`RUSTYOS-053`).
    pub focus_mode: bool,
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

/// Interactive click target inside the `TopBar` Left Menu area (`RUSTYOS-038`/`049`).
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
    /// Global Go menu (`RUSTYOS-049`).
    Go,
    /// Global Window menu.
    Window,
    /// Global Help menu (`RUSTYOS-049`).
    Help,
}

/// Action triggered from an open `TopBar` menu dropdown popover (`RUSTYOS-048`/`049`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopBarMenuAction {
    /// Show concise RustyOS notification card.
    AboutRustyOs,
    /// Show concise keyboard shortcuts reference card (`RUSTYOS-049`).
    ShowShortcuts,
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
    /// Switch to next workspace (`RUSTYOS-049`).
    NextWorkspace,
    /// Switch to previous workspace (`RUSTYOS-049`).
    PrevWorkspace,
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

/// Corner radius for floating Menu Bar dropdowns and Notification cards (`DESIGN.md` §12.3–12.6).
pub const MENU_POPOVER_RADIUS: i32 = 16;

/// Rich menu item entry with leading SDF glyph, optional shortcut label, and separator (`RUSTYOS-051`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopBarMenuEntry {
    /// Clickable action row.
    Item {
        /// Visible label.
        label: &'static str,
        /// Leading `4x4` SSAA SDF glyph.
        icon: crate::surfaces::UiGlyphKind,
        /// Optional right-aligned real keybinding hint.
        shortcut: Option<&'static str>,
        /// Target compositor / WM action.
        action: TopBarMenuAction,
    },
    /// Horizontal hairline separator between logical groups.
    Separator,
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
    /// Currently hovered Dock slot index (`0..DOCK_PINNED_COUNT + windows.len() + DOCK_UTILITY_COUNT`), if any.
    pub hover_slot: Option<usize>,
    /// Whether Workspace Overview is active (highlights Dock Overview squircle tile).
    pub overview_active: bool,
    /// Smooth 120 ms hover lift factor (`0.0..=1.0`) (`RUSTYOS-048`).
    pub hover_progress: f64,
    /// Prefer instant subtle lift without animated spring (`RUSTYOS-048`).
    pub reduced_motion: bool,
}

/// Interactive click target inside the bottom floating Dock (`RUSTYOS-023` / `RUSTYOS-050`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockHit {
    /// Pinned Files launcher (`Files`).
    PinnedFiles,
    /// Workspace Overview / App Grid toggle (`Overview`).
    ToggleOverview,
    /// Pinned Terminal launcher (`Terminal`).
    PinnedTerminal,
    /// Pinned Browser launcher (`Browser`).
    PinnedBrowser,
    /// Pinned Notes launcher (`Notes`).
    PinnedNotes,
    /// Pinned System Settings launcher (`Settings`).
    PinnedSettings,
    /// Running window tile at `index` inside `ordered_visible_windows`.
    WindowSlot(usize),
    /// Utility folder shortcut (`Downloads`).
    PinnedDownloads,
    /// Utility wastebasket shortcut (`Trash`).
    PinnedTrash,
}

/// Miniature workspace summary rendered in the top Spaces strip inside Overview (`RUSTYOS-051`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OverviewWorkspaceSpace {
    /// Workspace number (`1..=9`).
    pub id: u8,
    /// Total windows assigned to this workspace.
    pub window_count: usize,
    /// True when this is the currently active workspace.
    pub active: bool,
    /// Normalized `(x, y, w, h)` mini-window rectangles within `0.0..=1.0` (`w == 0` marks unused slot).
    pub preview_rects: [(f32, f32, f32, f32); 4],
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
    /// Miniature workspace strip summaries shown at the top of Overview (`RUSTYOS-051`).
    pub spaces: &'a [OverviewWorkspaceSpace],
    /// Animated reveal progress (`0.0` closed -> `1.0` open) (`RUSTYOS-025`).
    pub progress: f64,
}

/// Live data for the macOS-style interactive Lock Screen (`RUSTYOS-025`/`054`).
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct LockScreenRenderState<'a> {
    /// Lock Screen date header in neutral C locale (`"Thu Sep 10"`, `"Fri Nov 13"`).
    pub date_label: &'a str,
    /// Local wall-clock time without AM/PM suffix (`"8:52"` or `"14:52"`).
    pub clock_hhmm: &'a str,
    /// Active keyboard layout badge (`"EN"`, `"RU"`, etc.).
    pub keyboard_layout: &'a str,
    /// Whether hardware battery is present.
    pub has_battery: bool,
    /// Battery charge percentage (`0..=100`).
    pub battery_percent: u8,
    /// True when charging or connected to AC power.
    pub battery_charging: bool,
    /// True when Wi-Fi / network connection is active.
    pub wifi_connected: bool,
    /// Live Wi-Fi signal strength (`0..=100`).
    pub wifi_signal_percent: Option<u8>,
    /// Optional custom PNG wallpaper path matching active desktop background.
    pub wallpaper_path: Option<&'a std::path::Path>,
    /// Human user accounts rendered at bottom-center (`RUSTYOS-054`).
    pub users: &'a [rusty_services::UserAccount],
    /// Currently highlighted/selected account index in `users`.
    pub selected_user: usize,
    /// True while the top-right power actions popover is open.
    pub power_menu_open: bool,
    /// Require PAM password verification on Lock Screen (`RUSTYOS-056`).
    pub require_password: bool,
    /// Character length of typed secret (only dots `•` rendered, never plaintext) (`RUSTYOS-056`).
    pub password_len: usize,
    /// True while background `unix_chkpwd` verification is in flight (`RUSTYOS-056`).
    pub auth_checking: bool,
    /// Last PAM rejection or error status rendered below the password pill (`RUSTYOS-056`).
    pub auth_error: Option<&'a str>,
    /// User `[ui] glass_opacity` setting (`0.35..=1.0`).
    pub glass_opacity: f32,
    /// High-contrast opaque mode (`reduce_transparency`).
    pub reduce_transparency: bool,
    /// Animated reveal progress (`0.0` unlocked -> `1.0` locked).
    pub progress: f64,
}

/// Interactive click targets on the macOS-style Lock Screen (`RUSTYOS-054`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockScreenHit {
    /// Unlock the desktop session as user `usize` (instant when password disabled).
    UnlockUser(usize),
    /// Submit the typed password via the pill submit button (`RUSTYOS-056`).
    SubmitPassword,
    /// Select another account `usize` in the multi-user stack.
    SelectUser(usize),
    /// Cycle active XKB keyboard layout (`U.S.` / `RU`).
    ToggleKeyboardLayout,
    /// Open or close the top-right power popover (`Sleep` / `Restart` / `Shut Down`).
    TogglePowerMenu,
    /// Dispatch a power lifecycle action from the power popover.
    PowerAction(rusty_services::SessionAction),
    /// Clicked outside an open power popover to close it.
    DismissPowerMenu,
    /// Non-interactive Lock Screen wallpaper/clock region.
    None,
}

/// Active native `PolicyKit` authentication modal render state (`RUSTYOS-056`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolkitPromptRenderState<'a> {
    /// Human-readable action prompt description.
    pub message: &'a str,
    /// Privilege action identifier (`org.freedesktop.policykit.exec`, etc.).
    pub action_id: &'a str,
    /// Target UNIX user display name (`"root"`, etc.).
    pub identity_user: &'a str,
    /// Active prompt label (`"Password"` / helper prompt).
    pub prompt_label: &'a str,
    /// Masked dots (`••••`) or visible echo-on text; empty if nothing typed.
    pub input_display: &'a str,
    /// True while submitted credentials are being checked.
    pub checking: bool,
    /// Optional authentication error banner shown above action buttons.
    pub error_message: Option<&'a str>,
    /// Optional PAM informational message.
    pub info_message: Option<&'a str>,
}

/// Interactive click targets inside the centered `PolicyKit` Liquid Glass modal (`RUSTYOS-056`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolkitHit {
    /// Clicked the password / verification field.
    PasswordField,
    /// Clicked `Authenticate` submit button.
    Submit,
    /// Clicked `Cancel` button.
    Cancel,
    /// Clicked elsewhere inside or outside the modal without cancelling.
    Outside,
}

/// Single monitor or window candidate row inside the XDG Portal consent modal (`RUSTYOS-057`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortalSourceRow<'a> {
    /// Primary label (`"Monitor: eDP-1"` or window title).
    pub label: &'a str,
    /// Secondary subtitle (resolution / refresh or `app_id`).
    pub detail: &'a str,
    /// `true` for a window candidate, `false` for a monitor output.
    pub is_window: bool,
}

/// Active XDG Desktop Portal source-selection / consent modal render state (`RUSTYOS-057`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortalPromptRenderState<'a> {
    /// Requesting client application identifier (`app_id`).
    pub app_id: &'a str,
    /// `true` for `ScreenCast`, `false` for `Screenshot`.
    pub is_screencast: bool,
    /// Whether the software cursor should be embedded (`ScreenCast`).
    pub include_cursor: bool,
    /// Whether the `"Include cursor"` toggle row is displayed.
    pub cursor_mode_editable: bool,
    /// Selectable monitors and/or windows.
    pub rows: &'a [PortalSourceRow<'a>],
    /// Highlighted row index inside `rows`.
    pub selected: usize,
}

/// Interactive click targets inside the Liquid Glass XDG Portal modal (`RUSTYOS-057`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortalHit {
    /// Clicked candidate row index inside `rows`.
    Row(usize),
    /// Clicked `"Include cursor"` toggle row (`ScreenCast`).
    CursorToggle,
    /// Clicked `Cancel` button.
    Cancel,
    /// Clicked `Capture` or `Share` button.
    Accept,
    /// Clicked outside interactive buttons without dismissing.
    Outside,
}

/// Interactive click target while Workspace Overview is open (`RUSTYOS-023` / `RUSTYOS-051`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverviewHit {
    /// Focus and raise the window at card index `usize`.
    Card(usize),
    /// Switch to workspace `u8` (`1..=9`) from the top Spaces strip (`RUSTYOS-051`).
    SwitchWorkspace(u8),
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
    /// Native `PolicyKit` authentication modal (`RUSTYOS-056`).
    Polkit(&'a PolkitPromptRenderState<'a>),
    /// Native XDG Desktop Portal consent modal (`RUSTYOS-057`).
    Portal(&'a PortalPromptRenderState<'a>),
    /// Non-dimming interactive `PickColor` sampling banner pill (`RUSTYOS-057`).
    PickColor(&'a str),
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

/// Active slider drag target inside the Control Center (`RUSTYOS-051`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickSettingsSliderDrag {
    /// Master Sound volume slider.
    Sound,
    /// Display brightness slider (falling back to glass opacity when no backlight exists) (`RUSTYOS-053`).
    Display,
}

/// Interactive click target inside or outside the Quick Settings popover (`RUSTYOS-021`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickSettingsHit {
    /// Toggle Wi-Fi radio via NetworkManager / `nmcli` (`RUSTYOS-053`).
    ToggleWifi,
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
    /// Set Display brightness (or fallback glass opacity) along the Display track (`0..=100`) (`RUSTYOS-053`).
    SetDisplayPercent(u8),
    /// Skip to previous track via MPRIS2 (`RUSTYOS-053`).
    MediaPrev,
    /// Toggle playback via MPRIS2 (`RUSTYOS-053`).
    MediaPlayPause,
    /// Skip to next track via MPRIS2 (`RUSTYOS-053`).
    MediaNext,
    /// Toggle workspace layout (`Floating <-> MasterStack`).
    ToggleLayout,
    /// Open Workspace Overview from Control Center (`RUSTYOS-053`).
    ToggleOverview,
    /// Toggle accessibility `reduced_motion`.
    ToggleReducedMotion,
    /// Toggle Focus (Do Not Disturb) notification suppression (`RUSTYOS-053`).
    ToggleFocus,
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

fn format_lock_kbd_label(layout: &str) -> &str {
    let trimmed = layout.trim();
    if trimmed.is_empty()
        || trimmed.eq_ignore_ascii_case("EN")
        || trimmed.eq_ignore_ascii_case("US")
    {
        "U.S."
    } else {
        trimmed
    }
}

fn default_lock_user_fallback() -> rusty_services::UserAccount {
    rusty_services::UserAccount {
        username: "user".to_owned(),
        display_name: "RustyOS User".to_owned(),
        uid: 1000,
        avatar_path: None,
    }
}

#[derive(Debug, Clone)]
struct LockScreenLayout {
    kbd_hit_rect: Rectangle<i32, smithay::utils::Physical>,
    kbd_text_x: i32,
    kbd_glyph_x: i32,
    battery_x: Option<i32>,
    wifi_x: i32,
    power_btn_rect: Rectangle<i32, smithay::utils::Physical>,
    power_menu_rect: Rectangle<i32, smithay::utils::Physical>,
    power_item_rects: [Rectangle<i32, smithay::utils::Physical>; 3],
    date_y: i32,
    date_size: f32,
    clock_y: i32,
    clock_size: f32,
    user_row_rects: Vec<Rectangle<i32, smithay::utils::Physical>>,
    avatar_x: i32,
    name_x: i32,
    unlock_hint_rect: Option<Rectangle<i32, smithay::utils::Physical>>,
    password_pill_rect: Option<Rectangle<i32, smithay::utils::Physical>>,
    password_submit_rect: Option<Rectangle<i32, smithay::utils::Physical>>,
    auth_status_rect: Option<Rectangle<i32, smithay::utils::Physical>>,
}

struct LockUserStackLayout {
    user_row_rects: Vec<Rectangle<i32, smithay::utils::Physical>>,
    avatar_x: i32,
    name_x: i32,
    unlock_hint_rect: Option<Rectangle<i32, smithay::utils::Physical>>,
    password_pill_rect: Option<Rectangle<i32, smithay::utils::Physical>>,
    password_submit_rect: Option<Rectangle<i32, smithay::utils::Physical>>,
    auth_status_rect: Option<Rectangle<i32, smithay::utils::Physical>>,
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn compute_lock_user_stack_rects(
    w: i32,
    h: i32,
    user_names: &[&str],
    slide_dy: i32,
    require_password: bool,
) -> LockUserStackLayout {
    let avatar_d = 38;
    let row_gap = 12;
    let step = avatar_d + row_gap;
    let count = user_names.len().max(1);
    let single_user = count == 1;
    let count_i32 = i32::try_from(count).unwrap_or(1);
    let users_h = count_i32 * avatar_d + (count_i32 - 1) * row_gap;
    let extra_h = if require_password {
        58
    } else if single_user {
        26
    } else {
        0
    };
    let stack_h = users_h + extra_h;
    let stack_top_y = (h - 68 - stack_h).min((f64::from(h) * 0.745).round() as i32) + slide_dy;

    let mut max_name_w = 24;
    for name in user_names {
        let nw = crate::text::measure_ui_text_width(name, 13.5, crate::text::FontWeight::Bold);
        max_name_w = max_name_w.max(nw.min(220));
    }
    let group_w = (avatar_d + 10 + max_name_w).clamp(64, 300);
    let avatar_x = (w - group_w) / 2;
    let name_x = avatar_x + avatar_d + 10;

    let mut user_row_rects = Vec::with_capacity(count);
    for idx in 0..count {
        let row_y = stack_top_y + i32::try_from(idx).unwrap_or(0) * step;
        user_row_rects.push(Rectangle::new(
            Point::from((avatar_x - 8, row_y - 4)),
            Size::from((group_w + 16, avatar_d + 8)),
        ));
    }

    let unlock_hint_rect = (!require_password && single_user).then(|| {
        let hint_w = 224.min(w - 32);
        Rectangle::new(
            Point::from(((w - hint_w) / 2, stack_top_y + users_h + 10)),
            Size::from((hint_w, 22)),
        )
    });

    let (password_pill_rect, password_submit_rect, auth_status_rect) = if require_password {
        let pill_w = 228.min(w - 32);
        let pill_h = 30;
        let pill_x = (w - pill_w) / 2;
        let pill_y = stack_top_y + users_h + 10;
        let status_w = 340.min(w - 32);
        (
            Some(Rectangle::new(
                Point::from((pill_x, pill_y)),
                Size::from((pill_w, pill_h)),
            )),
            Some(Rectangle::new(
                Point::from((pill_x + pill_w - 29, pill_y + 1)),
                Size::from((28, 28)),
            )),
            Some(Rectangle::new(
                Point::from(((w - status_w) / 2, pill_y + pill_h + 6)),
                Size::from((status_w, 18)),
            )),
        )
    } else {
        (None, None, None)
    };

    LockUserStackLayout {
        user_row_rects,
        avatar_x,
        name_x,
        unlock_hint_rect,
        password_pill_rect,
        password_submit_rect,
        auth_status_rect,
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn compute_lock_screen_layout(
    screen_size: Size<i32, smithay::utils::Physical>,
    keyboard_layout: &str,
    has_battery: bool,
    user_names: &[&str],
    slide_dy: i32,
    require_password: bool,
) -> LockScreenLayout {
    let w = screen_size.w.max(320);
    let h = screen_size.h.max(240);

    let pwr_w = 22;
    let pwr_x = w - 12 - pwr_w;
    let power_btn_rect = Rectangle::new(Point::from((pwr_x, 7)), Size::from((pwr_w, 22)));

    let wifi_w = 18;
    let wifi_x = pwr_x - 10 - wifi_w;
    let mut left_cursor = wifi_x;

    let battery_x = if has_battery {
        let bx = left_cursor - 11 - 24;
        left_cursor = bx;
        Some(bx)
    } else {
        None
    };

    let kbd_glyph_w = 18;
    let kbd_glyph_x = left_cursor - 12 - kbd_glyph_w;
    let kbd_label = format_lock_kbd_label(keyboard_layout);
    let kbd_text_w =
        crate::text::measure_ui_text_width(kbd_label, 12.0, crate::text::FontWeight::Bold)
            .clamp(18, 64);
    let kbd_text_x = kbd_glyph_x - 5 - kbd_text_w;
    let kbd_hit_rect = Rectangle::new(
        Point::from((kbd_text_x - 6, 6)),
        Size::from((kbd_text_w + 5 + kbd_glyph_w + 10, 24)),
    );

    let pm_w = 148;
    let pm_h = 104;
    let pm_x = (w - 12 - pm_w).max(8);
    let pm_y = 34;
    let power_menu_rect = Rectangle::new(Point::from((pm_x, pm_y)), Size::from((pm_w, pm_h)));
    let power_item_rects = [
        Rectangle::new(
            Point::from((pm_x + 6, pm_y + 7)),
            Size::from((pm_w - 12, 30)),
        ),
        Rectangle::new(
            Point::from((pm_x + 6, pm_y + 37)),
            Size::from((pm_w - 12, 30)),
        ),
        Rectangle::new(
            Point::from((pm_x + 6, pm_y + 67)),
            Size::from((pm_w - 12, 30)),
        ),
    ];

    let date_size = (f64::from(h) * 0.025).clamp(16.0, 26.0) as f32;
    let clock_size = (f64::from(h) * 0.136).clamp(66.0, 132.0) as f32;
    let date_y = (f64::from(h) * 0.088).round() as i32 + slide_dy;
    let clock_y = date_y + (date_size * 0.92).round() as i32;
    let stack = compute_lock_user_stack_rects(w, h, user_names, slide_dy, require_password);

    LockScreenLayout {
        kbd_hit_rect,
        kbd_text_x,
        kbd_glyph_x,
        battery_x,
        wifi_x,
        power_btn_rect,
        power_menu_rect,
        power_item_rects,
        date_y,
        date_size,
        clock_y,
        clock_size,
        user_row_rects: stack.user_row_rects,
        avatar_x: stack.avatar_x,
        name_x: stack.name_x,
        unlock_hint_rect: stack.unlock_hint_rect,
        password_pill_rect: stack.password_pill_rect,
        password_submit_rect: stack.password_submit_rect,
        auth_status_rect: stack.auth_status_rect,
    }
}

/// Hit-tests `(x, y)` on the interactive macOS-style Lock Screen (`RUSTYOS-054`).
#[must_use]
pub fn hit_test_lock_screen(
    screen_size: Size<i32, smithay::utils::Physical>,
    lock: &LockScreenRenderState<'_>,
    x: f64,
    y: f64,
) -> LockScreenHit {
    let fallback_user = default_lock_user_fallback();
    let users = if lock.users.is_empty() {
        std::slice::from_ref(&fallback_user)
    } else {
        lock.users
    };
    let names: Vec<&str> = users.iter().map(|u| u.display_name.as_str()).collect();
    let layout = compute_lock_screen_layout(
        screen_size,
        lock.keyboard_layout,
        lock.has_battery,
        &names,
        0,
        lock.require_password,
    );

    if rect_contains(layout.power_btn_rect, x, y) {
        return LockScreenHit::TogglePowerMenu;
    }

    if lock.power_menu_open {
        let actions = [
            rusty_services::SessionAction::Suspend,
            rusty_services::SessionAction::Reboot,
            rusty_services::SessionAction::PowerOff,
        ];
        for (rect, action) in layout.power_item_rects.into_iter().zip(actions) {
            if rect_contains(rect, x, y) {
                return LockScreenHit::PowerAction(action);
            }
        }
        if rect_contains(layout.power_menu_rect, x, y) {
            return LockScreenHit::None;
        }
        return LockScreenHit::DismissPowerMenu;
    }

    if rect_contains(layout.kbd_hit_rect, x, y) {
        return LockScreenHit::ToggleKeyboardLayout;
    }

    if let Some(submit_rect) = layout.password_submit_rect
        && rect_contains(submit_rect, x, y)
    {
        return LockScreenHit::SubmitPassword;
    }

    if let Some(pill_rect) = layout.password_pill_rect
        && rect_contains(pill_rect, x, y)
    {
        return LockScreenHit::None;
    }

    let selected = lock.selected_user.min(users.len().saturating_sub(1));
    for (idx, row_rect) in layout.user_row_rects.iter().enumerate() {
        if rect_contains(*row_rect, x, y) {
            return if users.len() <= 1 || idx == selected {
                LockScreenHit::UnlockUser(idx)
            } else {
                LockScreenHit::SelectUser(idx)
            };
        }
    }

    if let Some(hint_rect) = layout.unlock_hint_rect
        && rect_contains(hint_rect, x, y)
    {
        return LockScreenHit::UnlockUser(selected);
    }

    LockScreenHit::None
}

/// Hit-tests `(x, y)` against the primary Lock Screen unlock row/hint (`RUSTYOS-025`/`054`).
#[must_use]
pub fn check_lock_screen_unlock_click(
    screen_size: Size<i32, smithay::utils::Physical>,
    x: f64,
    y: f64,
) -> bool {
    let default_state = LockScreenRenderState {
        date_label: "Thu Sep 10",
        clock_hhmm: "8:52",
        keyboard_layout: "EN",
        has_battery: true,
        battery_percent: 100,
        battery_charging: true,
        wifi_connected: true,
        wifi_signal_percent: Some(90),
        wallpaper_path: None,
        users: &[],
        selected_user: 0,
        power_menu_open: false,
        require_password: false,
        password_len: 0,
        auth_checking: false,
        auth_error: None,
        glass_opacity: 0.78,
        reduce_transparency: false,
        progress: 1.0,
    };
    matches!(
        hit_test_lock_screen(screen_size, &default_state, x, y),
        LockScreenHit::UnlockUser(_)
    )
}

#[cfg(test)]
#[must_use]
pub(crate) fn lock_screen_primary_row_center(
    screen_size: Size<i32, smithay::utils::Physical>,
    user_names: &[&str],
) -> (f64, f64) {
    let layout = compute_lock_screen_layout(screen_size, "EN", true, user_names, 0, false);
    let row = layout.user_row_rects[0];
    (
        f64::from(row.loc.x + row.size.w / 2),
        f64::from(row.loc.y + row.size.h / 2),
    )
}

#[derive(Debug, Clone, Copy)]
struct PolkitModalLayout {
    modal: Rectangle<i32, smithay::utils::Physical>,
    badge: Rectangle<i32, smithay::utils::Physical>,
    input: Rectangle<i32, smithay::utils::Physical>,
    status: Rectangle<i32, smithay::utils::Physical>,
    cancel: Rectangle<i32, smithay::utils::Physical>,
    submit: Rectangle<i32, smithay::utils::Physical>,
}

fn polkit_modal_layout(screen_size: Size<i32, smithay::utils::Physical>) -> PolkitModalLayout {
    let w = screen_size.w.max(280);
    let h = screen_size.h.max(320);
    let mw = 420.min(w - 32).max(248);
    let mh = 278;
    let mx = (w - mw) / 2;
    let my = ((h - mh) / 2 - 12).max(TOP_BAR_HEIGHT + 16);
    let btn_w = ((mw - 60) / 2).clamp(88, 136);
    let btn_y = my + mh - 50;

    PolkitModalLayout {
        modal: Rectangle::new(Point::from((mx, my)), Size::from((mw, mh))),
        badge: Rectangle::new(Point::from((mx + 22, my + 20)), Size::from((38, 38))),
        input: Rectangle::new(Point::from((mx + 22, my + 126)), Size::from((mw - 44, 38))),
        status: Rectangle::new(Point::from((mx + 22, my + 174)), Size::from((mw - 44, 36))),
        cancel: Rectangle::new(Point::from((mx + 22, btn_y)), Size::from((btn_w, 32))),
        submit: Rectangle::new(
            Point::from((mx + mw - 22 - btn_w, btn_y)),
            Size::from((btn_w, 32)),
        ),
    }
}

/// Hit-tests `(x, y)` inside the centered Liquid Glass `PolicyKit` dialog (`RUSTYOS-056`).
#[must_use]
pub fn hit_test_polkit_dialog(
    screen_size: Size<i32, smithay::utils::Physical>,
    x: f64,
    y: f64,
) -> PolkitHit {
    let layout = polkit_modal_layout(screen_size);
    if rect_contains(layout.submit, x, y) {
        return PolkitHit::Submit;
    }
    if rect_contains(layout.cancel, x, y) {
        return PolkitHit::Cancel;
    }
    if rect_contains(layout.input, x, y) {
        return PolkitHit::PasswordField;
    }
    PolkitHit::Outside
}

const PORTAL_MAX_VISIBLE_ROWS: usize = 5;

#[derive(Debug, Clone, Copy)]
struct PortalModalLayout {
    modal: Rectangle<i32, smithay::utils::Physical>,
    badge: Rectangle<i32, smithay::utils::Physical>,
    row_start: usize,
    row_rects: [Rectangle<i32, smithay::utils::Physical>; PORTAL_MAX_VISIBLE_ROWS],
    visible_count: usize,
    cursor_toggle: Option<Rectangle<i32, smithay::utils::Physical>>,
    cancel: Rectangle<i32, smithay::utils::Physical>,
    accept: Rectangle<i32, smithay::utils::Physical>,
}

fn portal_visible_range(total: usize, selected: usize) -> (usize, usize) {
    if total == 0 {
        return (0, 0);
    }
    let count = total.min(PORTAL_MAX_VISIBLE_ROWS);
    let sel = selected.min(total - 1);
    let max_start = total.saturating_sub(count);
    let start = sel.saturating_sub(count / 2).min(max_start);
    (start, start + count)
}

fn portal_modal_layout(
    screen_size: Size<i32, smithay::utils::Physical>,
    total_rows: usize,
    selected: usize,
    show_cursor_toggle: bool,
) -> PortalModalLayout {
    let w = screen_size.w.max(320);
    let h = screen_size.h.max(360);
    let mw = 460.min(w - 32).max(280);
    let (row_start, row_end) = portal_visible_range(total_rows, selected);
    let visible_count = row_end.saturating_sub(row_start);
    let list_rows = i32::try_from(visible_count.max(1)).unwrap_or(1);
    let row_h = 40;
    let row_gap = 6;
    let list_h = list_rows * row_h + (list_rows - 1) * row_gap;
    let toggle_h = if show_cursor_toggle { 34 } else { 0 };
    let mh = 96 + list_h + toggle_h + 62;
    let mx = (w - mw) / 2;
    let my = ((h - mh) / 2 - 10).max(TOP_BAR_HEIGHT + 16);

    let list_y = my + 84;
    let zero = Rectangle::new(Point::from((0, 0)), Size::from((0, 0)));
    let mut row_rects = [zero; PORTAL_MAX_VISIBLE_ROWS];
    for (idx, slot) in row_rects.iter_mut().take(visible_count).enumerate() {
        let off = i32::try_from(idx).unwrap_or(0);
        *slot = Rectangle::new(
            Point::from((mx + 22, list_y + off * (row_h + row_gap))),
            Size::from((mw - 44, row_h)),
        );
    }
    let after_list_y = list_y + list_h + 10;
    let cursor_toggle = show_cursor_toggle.then(|| {
        Rectangle::new(
            Point::from((mx + 22, after_list_y)),
            Size::from((mw - 44, 26)),
        )
    });
    let btn_w = ((mw - 60) / 2).clamp(96, 146);
    let btn_y = my + mh - 50;

    PortalModalLayout {
        modal: Rectangle::new(Point::from((mx, my)), Size::from((mw, mh))),
        badge: Rectangle::new(Point::from((mx + 22, my + 20)), Size::from((38, 38))),
        row_start,
        row_rects,
        visible_count,
        cursor_toggle,
        cancel: Rectangle::new(Point::from((mx + 22, btn_y)), Size::from((btn_w, 32))),
        accept: Rectangle::new(
            Point::from((mx + mw - 22 - btn_w, btn_y)),
            Size::from((btn_w, 32)),
        ),
    }
}

fn pick_color_pill_rect(
    screen_size: Size<i32, smithay::utils::Physical>,
) -> Rectangle<i32, smithay::utils::Physical> {
    let w = screen_size.w.max(320);
    let pw = 390.min(w - 32).max(240);
    let ph = 36;
    let px = (w - pw) / 2;
    let py = TOP_BAR_HEIGHT + 12;
    Rectangle::new(Point::from((px, py)), Size::from((pw, ph)))
}

/// Hit-tests `(x, y)` inside the centered Liquid Glass XDG Portal consent dialog (`RUSTYOS-057`).
#[must_use]
pub fn hit_test_portal_dialog(
    screen_size: Size<i32, smithay::utils::Physical>,
    total_rows: usize,
    selected: usize,
    show_cursor_toggle: bool,
    x: f64,
    y: f64,
) -> PortalHit {
    let layout = portal_modal_layout(screen_size, total_rows, selected, show_cursor_toggle);
    if rect_contains(layout.accept, x, y) {
        return PortalHit::Accept;
    }
    if rect_contains(layout.cancel, x, y) {
        return PortalHit::Cancel;
    }
    if let Some(toggle_rect) = layout.cursor_toggle
        && rect_contains(toggle_rect, x, y)
    {
        return PortalHit::CursorToggle;
    }
    for (idx, rect) in layout
        .row_rects
        .iter()
        .take(layout.visible_count)
        .enumerate()
    {
        if rect_contains(*rect, x, y) {
            return PortalHit::Row(layout.row_start + idx);
        }
    }
    PortalHit::Outside
}

/// Dynamic left-side geometry for `TopBar` app name, global menus, optional workspace strip, and layout badge (`RUSTYOS-048`/`049`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TopBarLeftLayout {
    pub app_x: i32,
    pub app_w: i32,
    pub file_x: i32,
    pub file_w: i32,
    pub edit_x: i32,
    pub edit_w: i32,
    pub view_x: i32,
    pub view_w: i32,
    pub go_x: i32,
    pub go_w: i32,
    pub win_x: i32,
    pub win_w: i32,
    pub help_x: i32,
    pub help_w: i32,
    pub show_workspaces: bool,
    pub separator_x: i32,
    pub ws_start_x: i32,
    pub layout_x: i32,
    pub layout_w: i32,
}

fn measure_menu_box_width(label: &str) -> i32 {
    (crate::text::measure_ui_text_width(label, TOP_BAR_FONT_SIZE, crate::text::FontWeight::Regular)
        + 14)
        .max(26)
}

/// Computes `TopBar` left-side x coordinates (`macOS` Menu Bar rhythm: `Logo  App  File  Edit  View  Go  Window  Help`) (`RUSTYOS-048`/`049`).
#[must_use]
pub fn compute_top_bar_left_layout(active_app_name: &str) -> TopBarLeftLayout {
    compute_top_bar_left_layout_with_options(active_app_name, false)
}

/// Computes `TopBar` left-side layout with explicit workspace-strip visibility (`RUSTYOS-049`).
#[must_use]
pub fn compute_top_bar_left_layout_with_options(
    active_app_name: &str,
    show_workspaces: bool,
) -> TopBarLeftLayout {
    let app_w = (crate::text::measure_ui_text_width(
        active_app_name,
        TOP_BAR_FONT_SIZE,
        crate::text::FontWeight::Bold,
    ) + 12)
        .clamp(44, 168);
    let file_x = TOP_BAR_APP_X + app_w;
    let file_w = measure_menu_box_width("File");
    let edit_x = file_x + file_w;
    let edit_w = measure_menu_box_width("Edit");
    let view_x = edit_x + edit_w;
    let view_w = measure_menu_box_width("View");
    let go_x = view_x + view_w;
    let go_w = measure_menu_box_width("Go");
    let win_x = go_x + go_w;
    let win_w = measure_menu_box_width("Window");
    let help_x = win_x + win_w;
    let help_w = measure_menu_box_width("Help");
    let separator_x = help_x + help_w + 10;
    let ws_start_x = separator_x + 12;
    let layout_x = ws_start_x + 9 * WS_BTN_STRIDE + 8;
    TopBarLeftLayout {
        app_x: TOP_BAR_APP_X,
        app_w,
        file_x,
        file_w,
        edit_x,
        edit_w,
        view_x,
        view_w,
        go_x,
        go_w,
        win_x,
        win_w,
        help_x,
        help_w,
        show_workspaces,
        separator_x,
        ws_start_x,
        layout_x,
        layout_w: 50,
    }
}

/// Dynamic right-side geometry for `TopBar` matching macOS order (`Battery · Wi-Fi · Search · Control Center · Clock`) (`RUSTYOS-048`/`049`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TopBarRightLayout {
    pub search_x: i32,
    pub search_w: i32,
    pub wifi_x: i32,
    pub volume_x: Option<i32>,
    pub battery_x: Option<i32>,
    pub control_center_x: i32,
    pub control_center_w: i32,
    pub clock_x: i32,
    pub clock_w: i32,
    pub status_pill_x: i32,
    pub status_pill_w: i32,
    pub status_hit_x: i32,
    pub status_hit_w: i32,
    pub notif_x: i32,
    pub notif_w: i32,
    pub kbd_x: Option<i32>,
    pub kbd_w: i32,
    pub tray_anchor_x: i32,
}

/// Computes compact right-aligned `TopBar` positions for `RUSTYOS-049` defaults (`show_* = false`).
#[must_use]
pub fn compute_top_bar_right_layout(
    screen_width: i32,
    has_battery: bool,
    notification_count: u32,
    clock_text: &str,
) -> TopBarRightLayout {
    compute_top_bar_right_layout_with_options(
        screen_width,
        clock_text,
        TopBarLayoutOptions {
            has_battery,
            notification_count,
            ..TopBarLayoutOptions::default()
        },
    )
}

/// Computes right-aligned `TopBar` geometry from exact `Inter` text width and visibility options (`RUSTYOS-049`).
#[must_use]
pub fn compute_top_bar_right_layout_with_options(
    screen_width: i32,
    clock_text: &str,
    opts: TopBarLayoutOptions,
) -> TopBarRightLayout {
    let clock_w = crate::text::measure_ui_text_width(
        clock_text,
        TOP_BAR_FONT_SIZE,
        crate::text::FontWeight::Regular,
    )
    .clamp(42, 220);
    let right_edge = screen_width - 10;
    let clock_x = (right_edge - clock_w).max(screen_width - 236);

    let control_center_w = 20;
    let control_center_x = clock_x - 6 - control_center_w;

    let search_w = 18;
    let search_x = control_center_x - 10 - search_w;

    let wifi_w = 20;
    let wifi_x = search_x - 10 - wifi_w;
    let mut left_cursor = wifi_x;

    let battery_w = 26;
    let battery_x = if opts.has_battery {
        let bx = left_cursor - 10 - battery_w;
        left_cursor = bx;
        Some(bx)
    } else {
        None
    };

    let volume_w = 18;
    let volume_x = if opts.show_volume {
        let vx = left_cursor - 10 - volume_w;
        left_cursor = vx;
        Some(vx)
    } else {
        None
    };

    let status_pill_x = control_center_x - 3;
    let status_pill_w = control_center_w + 6;
    let status_hit_x = left_cursor - 4;
    let status_hit_w = (right_edge - status_hit_x).max(88);

    let (notif_x, notif_w) = if opts.notification_count > 0 {
        let nw = 38;
        let nx = left_cursor - 8 - nw;
        left_cursor = nx;
        (nx, nw)
    } else {
        (left_cursor, 0)
    };

    let kbd_w = 28;
    let kbd_x = if opts.show_input_source {
        let kx = left_cursor - 8 - kbd_w;
        left_cursor = kx;
        Some(kx)
    } else {
        None
    };

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
        status_hit_x,
        status_hit_w,
        notif_x,
        notif_w,
        kbd_x,
        kbd_w,
        tray_anchor_x: left_cursor,
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

/// Maximum number of system tray applets displayed simultaneously in `TopBar` (`RUSTYOS-032`).
pub const MAX_TOP_BAR_TRAY_ITEMS: usize = 6;

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
    let toast_w = 340.min((width - 32).max(200));
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

/// Telemetry-aware hit-test for the `TopBar` unread notification badge (`RUSTYOS-048`/`049`).
#[must_use]
pub fn check_top_bar_notification_click_for_options(
    screen_width: i32,
    clock_text: &str,
    opts: TopBarLayoutOptions,
    x: f64,
    y: f64,
) -> bool {
    if opts.notification_count == 0 || y < 0.0 || y > f64::from(TOP_BAR_HEIGHT) {
        return false;
    }
    let layout = compute_top_bar_right_layout_with_options(screen_width, clock_text, opts);
    x >= f64::from(layout.notif_x) && x < f64::from(layout.notif_x + layout.notif_w)
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
    check_top_bar_notification_click_for_options(
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

fn rect_contains(rect: Rectangle<i32, smithay::utils::Physical>, x: f64, y: f64) -> bool {
    let rx = f64::from(rect.loc.x);
    let ry = f64::from(rect.loc.y);
    let rw = f64::from(rect.size.w);
    let rh = f64::from(rect.size.h);
    x >= rx && x < rx + rw && y >= ry && y < ry + rh
}

fn quick_settings_rect(width: i32, height: i32) -> (i32, i32, i32, i32) {
    let pop_w = 316.min((width - 24).max(240));
    let pop_h = 456.min((height - TOP_BAR_HEIGHT - 20).max(240));
    let pop_x = width - pop_w - 12;
    let pop_y = TOP_BAR_HEIGHT + 10;
    (pop_x, pop_y, pop_w, pop_h)
}

#[derive(Debug, Clone, Copy)]
struct QuickSettingsLayout {
    px: i32,
    py: i32,
    pw: i32,
    ph: i32,
    wifi_pill: Rectangle<i32, smithay::utils::Physical>,
    bt_pill: Rectangle<i32, smithay::utils::Physical>,
    now_playing_card: Rectangle<i32, smithay::utils::Physical>,
    media_prev_btn: Rectangle<i32, smithay::utils::Physical>,
    media_play_btn: Rectangle<i32, smithay::utils::Physical>,
    media_next_btn: Rectangle<i32, smithay::utils::Physical>,
    power_pill: Rectangle<i32, smithay::utils::Physical>,
    layout_btn: Rectangle<i32, smithay::utils::Physical>,
    overview_btn: Rectangle<i32, smithay::utils::Physical>,
    motion_btn: Rectangle<i32, smithay::utils::Physical>,
    transparency_btn: Rectangle<i32, smithay::utils::Physical>,
    focus_pill: Rectangle<i32, smithay::utils::Physical>,
    display_card: Rectangle<i32, smithay::utils::Physical>,
    display_track: Rectangle<i32, smithay::utils::Physical>,
    sound_card: Rectangle<i32, smithay::utils::Physical>,
    sound_track: Rectangle<i32, smithay::utils::Physical>,
    sound_mute_zone: Rectangle<i32, smithay::utils::Physical>,
    sound_left_mute_zone: Rectangle<i32, smithay::utils::Physical>,
    lock_btn: Rectangle<i32, smithay::utils::Physical>,
}

fn quick_settings_layout(width: i32, height: i32, anim_dy: i32) -> QuickSettingsLayout {
    let (px, base_py, pw, ph) = quick_settings_rect(width, height);
    let py = base_py + anim_dy;
    let gap = 12;
    let col_w = (pw - gap) / 2;
    let right_w = pw - col_w - gap;
    let left_x = px;
    let right_x = px + col_w + gap;
    let btn_d = 66.min((col_w - 8) / 2);

    let y_bt = py + 64;
    let y_row3 = py + 130;
    let y_row4 = y_row3 + 78;
    let y_disp = y_row4 + 78;
    let y_snd = y_disp + 70;
    let y_lock = y_snd + 70;

    let media_btn_y = y_bt + 10;
    let media_btn_w = ((right_w - 16) / 3).max(28);
    let lock_w = 118.min(pw);

    QuickSettingsLayout {
        px,
        py,
        pw,
        ph,
        wifi_pill: Rectangle::new(Point::from((left_x, py)), Size::from((col_w, 54))),
        bt_pill: Rectangle::new(Point::from((left_x, y_bt)), Size::from((col_w, 54))),
        now_playing_card: Rectangle::new(Point::from((right_x, py)), Size::from((right_w, 118))),
        media_prev_btn: Rectangle::new(
            Point::from((right_x + 8, media_btn_y)),
            Size::from((media_btn_w, 36)),
        ),
        media_play_btn: Rectangle::new(
            Point::from((right_x + 8 + media_btn_w, media_btn_y)),
            Size::from((media_btn_w, 36)),
        ),
        media_next_btn: Rectangle::new(
            Point::from((right_x + 8 + media_btn_w * 2, media_btn_y)),
            Size::from((media_btn_w, 36)),
        ),
        power_pill: Rectangle::new(Point::from((left_x, y_row3 + 3)), Size::from((col_w, 60))),
        layout_btn: Rectangle::new(Point::from((right_x, y_row3)), Size::from((btn_d, btn_d))),
        overview_btn: Rectangle::new(
            Point::from((right_x + right_w - btn_d, y_row3)),
            Size::from((btn_d, btn_d)),
        ),
        motion_btn: Rectangle::new(Point::from((left_x, y_row4)), Size::from((btn_d, btn_d))),
        transparency_btn: Rectangle::new(
            Point::from((left_x + col_w - btn_d, y_row4)),
            Size::from((btn_d, btn_d)),
        ),
        focus_pill: Rectangle::new(
            Point::from((right_x, y_row4 + 3)),
            Size::from((right_w, 60)),
        ),
        display_card: Rectangle::new(Point::from((px, y_disp)), Size::from((pw, 58))),
        display_track: Rectangle::new(
            Point::from((px + 34, y_disp + 31)),
            Size::from(((pw - 68).max(24), 20)),
        ),
        sound_card: Rectangle::new(Point::from((px, y_snd)), Size::from((pw, 58))),
        sound_track: Rectangle::new(
            Point::from((px + 34, y_snd + 31)),
            Size::from(((pw - 106).max(24), 20)),
        ),
        sound_mute_zone: Rectangle::new(
            Point::from((px + pw - 39, y_snd + 23)),
            Size::from((30, 30)),
        ),
        sound_left_mute_zone: Rectangle::new(
            Point::from((px + 8, y_snd + 28)),
            Size::from((24, 24)),
        ),
        lock_btn: Rectangle::new(
            Point::from((px + (pw - lock_w) / 2, y_lock)),
            Size::from((lock_w, 30)),
        ),
    }
}

#[must_use]
fn dock_layout_bounds(width: i32, height: i32, window_count: usize) -> (i32, i32, i32, usize) {
    let items_count = DOCK_PINNED_COUNT + window_count + DOCK_UTILITY_COUNT;
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let count_i32 = items_count as i32;
    let divider_extra = DOCK_DIVIDER_GAP;
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
    let left_count = DOCK_PINNED_COUNT + window_count;
    let item_size = DOCK_ITEM_SIZE;
    let slot_i32 = i32::try_from(slot_idx).ok()?;
    let divider_shift = if slot_idx >= left_count {
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

#[must_use]
fn dock_tooltip_label(dock_view: &DockViewState<'_>, slot_idx: usize) -> Option<String> {
    let left_count = DOCK_PINNED_COUNT + dock_view.windows.len();
    match slot_idx {
        0 => Some("Files".to_string()),
        1 => Some("Overview".to_string()),
        2 => Some("Terminal".to_string()),
        3 => Some("Browser".to_string()),
        4 => Some("Notes".to_string()),
        5 => Some("Settings".to_string()),
        n if n < left_count => dock_view
            .windows
            .get(n - DOCK_PINNED_COUNT)
            .map(|w| truncate_ascii(&w.title, 26)),
        n if n == left_count => Some("Downloads".to_string()),
        n if n == left_count + 1 => Some("Trash".to_string()),
        _ => None,
    }
}

#[must_use]
fn dock_tooltip_rect_and_label(
    width: i32,
    height: i32,
    dock_view: &DockViewState<'_>,
) -> Option<(Rectangle<i32, smithay::utils::Physical>, String)> {
    let slot_idx = dock_view.hover_slot?;
    let slot = dock_slot_rect(width, height, dock_view.windows.len(), slot_idx)?;
    let label = dock_tooltip_label(dock_view, slot_idx)?;
    let text_w = crate::text::measure_ui_text_width(&label, 12.0, crate::text::FontWeight::Bold);
    let pill_w = (text_w + 22).clamp(48, 220);
    let pill_h = 26;
    let slot_center_x = slot.loc.x + slot.size.w / 2;
    let pill_x = (slot_center_x - pill_w / 2).clamp(12, (width - pill_w - 12).max(12));
    let pill_y = (height - DOCK_HEIGHT - DOCK_MARGIN_BOTTOM - pill_h - 14).max(TOP_BAR_HEIGHT + 8);
    Some((
        Rectangle::new(Point::from((pill_x, pill_y)), Size::from((pill_w, pill_h))),
        label,
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
    let items_count = DOCK_PINNED_COUNT + window_count + DOCK_UTILITY_COUNT;
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

/// Hit-tests a pointer click at `(x, y)` against the bottom Dock (`RUSTYOS-023` / `RUSTYOS-050`).
#[must_use]
pub fn check_dock_click(
    screen_size: Size<i32, smithay::utils::Physical>,
    window_count: usize,
    x: f64,
    y: f64,
) -> Option<DockHit> {
    let slot = check_dock_slot(screen_size, window_count, x, y)?;
    let left_count = DOCK_PINNED_COUNT + window_count;
    match slot {
        0 => Some(DockHit::PinnedFiles),
        1 => Some(DockHit::ToggleOverview),
        2 => Some(DockHit::PinnedTerminal),
        3 => Some(DockHit::PinnedBrowser),
        4 => Some(DockHit::PinnedNotes),
        5 => Some(DockHit::PinnedSettings),
        n if n < left_count => Some(DockHit::WindowSlot(n - DOCK_PINNED_COUNT)),
        n if n == left_count => Some(DockHit::PinnedDownloads),
        n if n == left_count + 1 => Some(DockHit::PinnedTrash),
        _ => None,
    }
}

/// Computes centered top Spaces miniature strip rectangles for `count` workspaces (`RUSTYOS-051`).
#[must_use]
pub fn overview_space_rects(
    screen_size: Size<i32, smithay::utils::Physical>,
    count: usize,
) -> Vec<Rectangle<i32, smithay::utils::Physical>> {
    if count == 0 {
        return Vec::new();
    }
    let visible = count.min(9);
    let width = screen_size.w.max(320);
    let count_i32 = i32::try_from(visible).unwrap_or(1);
    let gap = 10;
    let avail_w = (width - 64).max(180);
    let thumb_w = ((avail_w - gap * (count_i32 - 1)) / count_i32).clamp(52, 108);
    let thumb_h = ((thumb_w * 58) / 100).clamp(34, 62);
    let total_w = count_i32 * thumb_w + (count_i32 - 1) * gap;
    let start_x = (width - total_w) / 2;
    let start_y = TOP_BAR_HEIGHT + 16;

    (0..visible)
        .map(|idx| {
            let sx = start_x + i32::try_from(idx).unwrap_or(0) * (thumb_w + gap);
            Rectangle::new(Point::from((sx, start_y)), Size::from((thumb_w, thumb_h)))
        })
        .collect()
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
    let area_top = TOP_BAR_HEIGHT + 108;
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

/// Hit-tests `(x, y)` inside Workspace Overview, including Spaces strip and window cards (`RUSTYOS-023` / `RUSTYOS-051`).
#[must_use]
pub fn check_overview_click_with_spaces(
    screen_size: Size<i32, smithay::utils::Physical>,
    spaces: &[OverviewWorkspaceSpace],
    window_count: usize,
    x: f64,
    y: f64,
) -> OverviewHit {
    if !spaces.is_empty() {
        let space_rects = overview_space_rects(screen_size, spaces.len());
        for (space, rect) in spaces.iter().zip(space_rects) {
            let hit_rect = Rectangle::new(rect.loc, Size::from((rect.size.w, rect.size.h + 18)));
            if rect_contains(hit_rect, x, y) {
                return OverviewHit::SwitchWorkspace(space.id);
            }
        }
    }
    let cards = overview_card_rects(screen_size, window_count);
    for (idx, rect) in cards.into_iter().enumerate() {
        if rect_contains(rect, x, y) {
            return OverviewHit::Card(idx);
        }
    }
    OverviewHit::Dismiss
}

/// Hit-tests `(x, y)` against Workspace Overview window cards (`RUSTYOS-023`).
#[must_use]
pub fn check_overview_click(
    screen_size: Size<i32, smithay::utils::Physical>,
    window_count: usize,
    x: f64,
    y: f64,
) -> OverviewHit {
    check_overview_click_with_spaces(screen_size, &[], window_count, x, y)
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

const MENU_SYSTEM: &[TopBarMenuEntry] = &[
    TopBarMenuEntry::Item {
        label: "About RustyOS",
        icon: crate::surfaces::UiGlyphKind::Info,
        shortcut: None,
        action: TopBarMenuAction::AboutRustyOs,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Workspace Overview",
        icon: crate::surfaces::UiGlyphKind::Grid,
        shortcut: Some("Super+Tab"),
        action: TopBarMenuAction::OpenOverview,
    },
    TopBarMenuEntry::Item {
        label: "Control Center",
        icon: crate::surfaces::UiGlyphKind::ControlCenter,
        shortcut: Some("Alt+S"),
        action: TopBarMenuAction::OpenControlCenter,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Lock Screen",
        icon: crate::surfaces::UiGlyphKind::Lock,
        shortcut: Some("Super+L"),
        action: TopBarMenuAction::LockSession,
    },
];

const MENU_ACTIVE_APP: &[TopBarMenuEntry] = &[
    TopBarMenuEntry::Item {
        label: "New Terminal",
        icon: crate::surfaces::UiGlyphKind::TerminalPrompt,
        shortcut: None,
        action: TopBarMenuAction::NewTerminal,
    },
    TopBarMenuEntry::Item {
        label: "Open Files",
        icon: crate::surfaces::UiGlyphKind::Folder,
        shortcut: None,
        action: TopBarMenuAction::OpenFiles,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Control Center",
        icon: crate::surfaces::UiGlyphKind::ControlCenter,
        shortcut: Some("Alt+S"),
        action: TopBarMenuAction::OpenControlCenter,
    },
];

const MENU_FILE: &[TopBarMenuEntry] = &[
    TopBarMenuEntry::Item {
        label: "New Terminal",
        icon: crate::surfaces::UiGlyphKind::TerminalPrompt,
        shortcut: None,
        action: TopBarMenuAction::NewTerminal,
    },
    TopBarMenuEntry::Item {
        label: "Open Files",
        icon: crate::surfaces::UiGlyphKind::Folder,
        shortcut: None,
        action: TopBarMenuAction::OpenFiles,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Spotlight Search",
        icon: crate::surfaces::UiGlyphKind::Search,
        shortcut: Some("Super+Space"),
        action: TopBarMenuAction::OpenSpotlight,
    },
];

const MENU_EDIT: &[TopBarMenuEntry] = &[
    TopBarMenuEntry::Item {
        label: "Toggle Float / Tile",
        icon: crate::surfaces::UiGlyphKind::LayoutSplit,
        shortcut: Some("Alt+L"),
        action: TopBarMenuAction::ToggleLayout,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Toggle Reduced Motion",
        icon: crate::surfaces::UiGlyphKind::Motion,
        shortcut: Some("Alt+R"),
        action: TopBarMenuAction::ToggleReducedMotion,
    },
    TopBarMenuEntry::Item {
        label: "Toggle Opaque Panels",
        icon: crate::surfaces::UiGlyphKind::Transparency,
        shortcut: None,
        action: TopBarMenuAction::ToggleReduceTransparency,
    },
];

const MENU_VIEW: &[TopBarMenuEntry] = &[
    TopBarMenuEntry::Item {
        label: "Workspace Overview",
        icon: crate::surfaces::UiGlyphKind::Grid,
        shortcut: Some("Super+Tab"),
        action: TopBarMenuAction::OpenOverview,
    },
    TopBarMenuEntry::Item {
        label: "Control Center",
        icon: crate::surfaces::UiGlyphKind::ControlCenter,
        shortcut: Some("Alt+S"),
        action: TopBarMenuAction::OpenControlCenter,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Refresh Metrics",
        icon: crate::surfaces::UiGlyphKind::Refresh,
        shortcut: None,
        action: TopBarMenuAction::RefreshMetrics,
    },
];

const MENU_GO: &[TopBarMenuEntry] = &[
    TopBarMenuEntry::Item {
        label: "Workspace Overview",
        icon: crate::surfaces::UiGlyphKind::Grid,
        shortcut: Some("Super+Tab"),
        action: TopBarMenuAction::OpenOverview,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Next Workspace",
        icon: crate::surfaces::UiGlyphKind::ArrowNext,
        shortcut: None,
        action: TopBarMenuAction::NextWorkspace,
    },
    TopBarMenuEntry::Item {
        label: "Previous Workspace",
        icon: crate::surfaces::UiGlyphKind::ArrowPrev,
        shortcut: None,
        action: TopBarMenuAction::PrevWorkspace,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Spotlight Search",
        icon: crate::surfaces::UiGlyphKind::Search,
        shortcut: Some("Super+Space"),
        action: TopBarMenuAction::OpenSpotlight,
    },
];

const MENU_WINDOW: &[TopBarMenuEntry] = &[
    TopBarMenuEntry::Item {
        label: "Focus Next Window",
        icon: crate::surfaces::UiGlyphKind::ArrowNext,
        shortcut: Some("Alt+J"),
        action: TopBarMenuAction::FocusNextWindow,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Maximize / Restore",
        icon: crate::surfaces::UiGlyphKind::WindowMax,
        shortcut: Some("Alt+M"),
        action: TopBarMenuAction::ToggleMaximizeFocused,
    },
    TopBarMenuEntry::Item {
        label: "Minimize Window",
        icon: crate::surfaces::UiGlyphKind::WindowMin,
        shortcut: Some("Alt+H"),
        action: TopBarMenuAction::MinimizeFocused,
    },
    TopBarMenuEntry::Item {
        label: "Toggle Float / Tile",
        icon: crate::surfaces::UiGlyphKind::LayoutSplit,
        shortcut: Some("Alt+L"),
        action: TopBarMenuAction::ToggleLayout,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Close Window",
        icon: crate::surfaces::UiGlyphKind::Close,
        shortcut: Some("Alt+W"),
        action: TopBarMenuAction::CloseFocused,
    },
];

const MENU_HELP: &[TopBarMenuEntry] = &[
    TopBarMenuEntry::Item {
        label: "About RustyOS",
        icon: crate::surfaces::UiGlyphKind::Info,
        shortcut: None,
        action: TopBarMenuAction::AboutRustyOs,
    },
    TopBarMenuEntry::Item {
        label: "Keyboard Shortcuts",
        icon: crate::surfaces::UiGlyphKind::Shortcuts,
        shortcut: None,
        action: TopBarMenuAction::ShowShortcuts,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Refresh Metrics",
        icon: crate::surfaces::UiGlyphKind::Refresh,
        shortcut: None,
        action: TopBarMenuAction::RefreshMetrics,
    },
];

/// Returns menu dropdown entries with leading SDF icons, group separators, and real keybindings (`RUSTYOS-051`).
#[must_use]
pub const fn top_bar_menu_entries(menu: TopBarMenuHit) -> &'static [TopBarMenuEntry] {
    match menu {
        TopBarMenuHit::SystemLogo => MENU_SYSTEM,
        TopBarMenuHit::ActiveApp => MENU_ACTIVE_APP,
        TopBarMenuHit::File => MENU_FILE,
        TopBarMenuHit::Edit => MENU_EDIT,
        TopBarMenuHit::View => MENU_VIEW,
        TopBarMenuHit::Go => MENU_GO,
        TopBarMenuHit::Window => MENU_WINDOW,
        TopBarMenuHit::Help => MENU_HELP,
    }
}

/// Returns compact `(label, action)` menu tuples for an open `TopBar` dropdown (`RUSTYOS-048`).
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
        TopBarMenuHit::Go => &[
            ("Workspace Overview", TopBarMenuAction::OpenOverview),
            ("Next Workspace", TopBarMenuAction::NextWorkspace),
            ("Previous Workspace", TopBarMenuAction::PrevWorkspace),
            ("Spotlight Search", TopBarMenuAction::OpenSpotlight),
        ],
        TopBarMenuHit::Window => &[
            ("Focus Next Window", TopBarMenuAction::FocusNextWindow),
            (
                "Maximize / Restore",
                TopBarMenuAction::ToggleMaximizeFocused,
            ),
            ("Minimize Window", TopBarMenuAction::MinimizeFocused),
            ("Toggle Float / Tile", TopBarMenuAction::ToggleLayout),
            ("Close Window", TopBarMenuAction::CloseFocused),
        ],
        TopBarMenuHit::Help => &[
            ("About RustyOS", TopBarMenuAction::AboutRustyOs),
            ("Keyboard Shortcuts", TopBarMenuAction::ShowShortcuts),
            ("Refresh Metrics", TopBarMenuAction::RefreshMetrics),
        ],
    }
}

const MENU_POPOVER_PAD_Y: i32 = 6;
const MENU_ITEM_HEIGHT: i32 = 28;
const MENU_SEPARATOR_HEIGHT: i32 = 9;

/// Computes the glass dropdown popover rectangle anchored beneath `menu` (`RUSTYOS-048`/`051`).
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
        TopBarMenuHit::Go => left.go_x,
        TopBarMenuHit::Window => left.win_x,
        TopBarMenuHit::Help => left.help_x,
    };
    let mut content_h = 0;
    for entry in top_bar_menu_entries(menu) {
        content_h += match entry {
            TopBarMenuEntry::Item { .. } => MENU_ITEM_HEIGHT,
            TopBarMenuEntry::Separator => MENU_SEPARATOR_HEIGHT,
        };
    }
    let pop_w = 256;
    let pop_h = MENU_POPOVER_PAD_Y * 2 + content_h;
    (anchor_x, TOP_BAR_HEIGHT + 6, pop_w, pop_h)
}

fn top_bar_menu_entry_at(
    active_app_name: &str,
    menu: TopBarMenuHit,
    x: f64,
    y: f64,
) -> Option<(usize, TopBarMenuAction)> {
    let (px, py, pw, ph) = top_bar_menu_popover_rect(active_app_name, menu);
    let fx = f64::from(px);
    let fy = f64::from(py);
    if x < fx || x >= fx + f64::from(pw) || y < fy || y >= fy + f64::from(ph) {
        return None;
    }
    let mut cursor_y = py + MENU_POPOVER_PAD_Y;
    for (entry_idx, entry) in top_bar_menu_entries(menu).iter().enumerate() {
        match entry {
            TopBarMenuEntry::Item { action, .. } => {
                let top = f64::from(cursor_y);
                let bot = f64::from(cursor_y + MENU_ITEM_HEIGHT);
                if y >= top && y < bot {
                    return Some((entry_idx, *action));
                }
                cursor_y += MENU_ITEM_HEIGHT;
            }
            TopBarMenuEntry::Separator => {
                cursor_y += MENU_SEPARATOR_HEIGHT;
            }
        }
    }
    None
}

/// Hit-tests `(x, y)` against an open `TopBar` menu dropdown popover (`RUSTYOS-048`/`051`).
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
    if let Some((_, action)) = top_bar_menu_entry_at(active_app_name, menu, x, y) {
        TopBarMenuPopoverClick::Action(action)
    } else {
        TopBarMenuPopoverClick::Inside
    }
}

/// Returns the hovered entry index (`0..top_bar_menu_entries(menu).len()`) inside an open menu popover (`RUSTYOS-051`).
#[must_use]
pub fn check_top_bar_menu_popover_hover_row(
    active_app_name: &str,
    menu: TopBarMenuHit,
    x: f64,
    y: f64,
) -> Option<usize> {
    top_bar_menu_entry_at(active_app_name, menu, x, y).map(|(idx, _)| idx)
}

/// Hit-tests `(x, y)` against the `TopBar` Left Menu (`Logo`, `App`, `File`, `Edit`, `View`, `Go`, `Window`, `Help`) (`RUSTYOS-038`/`049`).
#[must_use]
pub fn check_top_bar_menu_click(x: f64, y: f64) -> Option<TopBarMenuHit> {
    check_top_bar_menu_click_for_app("RustyOS", x, y)
}

/// Active-app-width-aware hit-test for the `TopBar` Left Menu (`RUSTYOS-048`/`049`).
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
    for (hit, bx, bw) in [
        (TopBarMenuHit::ActiveApp, left.app_x, left.app_w),
        (TopBarMenuHit::File, left.file_x, left.file_w),
        (TopBarMenuHit::Edit, left.edit_x, left.edit_w),
        (TopBarMenuHit::View, left.view_x, left.view_w),
        (TopBarMenuHit::Go, left.go_x, left.go_w),
        (TopBarMenuHit::Window, left.win_x, left.win_w),
        (TopBarMenuHit::Help, left.help_x, left.help_w),
    ] {
        if x >= f64::from(bx) && x < f64::from(bx + bw) {
            return Some(hit);
        }
    }
    None
}

/// Checks if `(x, y)` clicked the `TopBar` Layout badge when visible (`Float` / `Tile`).
#[must_use]
pub fn check_top_bar_layout_click(x: f64, y: f64) -> bool {
    check_top_bar_layout_click_with_options("RustyOS", true, x, y)
}

/// Active-app-width-aware hit-test for the `TopBar` Layout badge (`RUSTYOS-048`).
#[must_use]
pub fn check_top_bar_layout_click_for_app(active_app_name: &str, x: f64, y: f64) -> bool {
    check_top_bar_layout_click_with_options(active_app_name, true, x, y)
}

/// Option-aware hit-test for the optional `TopBar` Layout badge (`RUSTYOS-049`).
#[must_use]
pub fn check_top_bar_layout_click_with_options(
    active_app_name: &str,
    show_workspaces: bool,
    x: f64,
    y: f64,
) -> bool {
    if !show_workspaces || y < 0.0 || y > f64::from(TOP_BAR_HEIGHT) {
        return false;
    }
    let left = compute_top_bar_left_layout_with_options(active_app_name, show_workspaces);
    x >= f64::from(left.layout_x) && x < f64::from(left.layout_x + left.layout_w)
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

fn rounded_rect_contains(
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

/// Evaluates pointer clicks against floating Control Center islands (`RUSTYOS-053`).
#[must_use]
pub fn check_quick_settings_click(
    screen_size: Size<i32, smithay::utils::Physical>,
    x: f64,
    y: f64,
) -> QuickSettingsHit {
    let layout = quick_settings_layout(screen_size.w, screen_size.h, 0);
    let pop_rect = Rectangle::new(
        Point::from((layout.px, layout.py)),
        Size::from((layout.pw, layout.ph)),
    );
    if !rect_contains(pop_rect, x, y) {
        return QuickSettingsHit::OutsidePopover;
    }

    if rounded_rect_contains(layout.wifi_pill, layout.wifi_pill.size.h / 2, x, y) {
        return QuickSettingsHit::ToggleWifi;
    }
    if rounded_rect_contains(layout.bt_pill, layout.bt_pill.size.h / 2, x, y) {
        return QuickSettingsHit::ToggleBluetooth;
    }
    if rect_contains(layout.media_prev_btn, x, y) {
        return QuickSettingsHit::MediaPrev;
    }
    if rect_contains(layout.media_play_btn, x, y) {
        return QuickSettingsHit::MediaPlayPause;
    }
    if rect_contains(layout.media_next_btn, x, y) {
        return QuickSettingsHit::MediaNext;
    }
    if rounded_rect_contains(layout.power_pill, layout.power_pill.size.h / 2, x, y) {
        return QuickSettingsHit::RefreshMetrics;
    }
    if rounded_rect_contains(layout.layout_btn, layout.layout_btn.size.h / 2, x, y) {
        return QuickSettingsHit::ToggleLayout;
    }
    if rounded_rect_contains(layout.overview_btn, layout.overview_btn.size.h / 2, x, y) {
        return QuickSettingsHit::ToggleOverview;
    }
    if rounded_rect_contains(layout.motion_btn, layout.motion_btn.size.h / 2, x, y) {
        return QuickSettingsHit::ToggleReducedMotion;
    }
    if rounded_rect_contains(
        layout.transparency_btn,
        layout.transparency_btn.size.h / 2,
        x,
        y,
    ) {
        return QuickSettingsHit::ToggleReduceTransparency;
    }
    if rounded_rect_contains(layout.focus_pill, layout.focus_pill.size.h / 2, x, y) {
        return QuickSettingsHit::ToggleFocus;
    }
    if rect_contains(layout.display_track, x, y) {
        return QuickSettingsHit::SetDisplayPercent(quick_settings_slider_percent(
            screen_size,
            QuickSettingsSliderDrag::Display,
            x,
        ));
    }
    if rounded_rect_contains(
        layout.sound_mute_zone,
        layout.sound_mute_zone.size.h / 2,
        x,
        y,
    ) || rect_contains(layout.sound_left_mute_zone, x, y)
    {
        return QuickSettingsHit::ToggleMute;
    }
    if rect_contains(layout.sound_track, x, y) {
        return QuickSettingsHit::SetVolumePercent(quick_settings_slider_percent(
            screen_size,
            QuickSettingsSliderDrag::Sound,
            x,
        ));
    }
    if rounded_rect_contains(layout.lock_btn, layout.lock_btn.size.h / 2, x, y) {
        return QuickSettingsHit::LockSession;
    }

    QuickSettingsHit::InsidePopover
}

/// Identifies whether `(x, y)` initiated a continuous slider drag inside Control Center (`RUSTYOS-051`).
#[must_use]
pub fn check_quick_settings_slider_drag(
    screen_size: Size<i32, smithay::utils::Physical>,
    x: f64,
    y: f64,
) -> Option<QuickSettingsSliderDrag> {
    let layout = quick_settings_layout(screen_size.w, screen_size.h, 0);
    if rect_contains(layout.display_track, x, y) {
        Some(QuickSettingsSliderDrag::Display)
    } else if rect_contains(layout.sound_track, x, y)
        && !rounded_rect_contains(
            layout.sound_mute_zone,
            layout.sound_mute_zone.size.h / 2,
            x,
            y,
        )
        && !rect_contains(layout.sound_left_mute_zone, x, y)
    {
        Some(QuickSettingsSliderDrag::Sound)
    } else {
        None
    }
}

/// Maps pointer `x` along an active Control Center slider track to `0..=100` (`RUSTYOS-053`).
#[must_use]
pub fn quick_settings_slider_percent(
    screen_size: Size<i32, smithay::utils::Physical>,
    drag: QuickSettingsSliderDrag,
    x: f64,
) -> u8 {
    let layout = quick_settings_layout(screen_size.w, screen_size.h, 0);
    let track = match drag {
        QuickSettingsSliderDrag::Sound => layout.sound_track,
        QuickSettingsSliderDrag::Display => layout.display_track,
    };
    let track_x = f64::from(track.loc.x);
    let track_w = f64::from(track.size.w.max(1));
    let norm = ((x - track_x) / track_w).clamp(0.0, 1.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    ((norm * 100.0).round() as u8).clamp(0, 100)
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
    let left_layout =
        compute_top_bar_left_layout_with_options("RustyOS", telemetry.show_workspaces);

    // 1. TopBar Background (opaque only when reduce_transparency is requested)
    if telemetry.reduce_transparency {
        let top_bar_geo = Rectangle::new(Point::from((0, 0)), Size::from((width, TOP_BAR_HEIGHT)));
        elements.push(make_solid_element(top_bar_geo, COLOR_SURFACE_ELEVATED));
        let top_bar_border =
            Rectangle::new(Point::from((0, TOP_BAR_HEIGHT - 1)), Size::from((width, 1)));
        elements.push(make_solid_element(top_bar_border, COLOR_BORDER));
    }

    // 2. Active App Menu placeholder (when no font renderer is active)
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
                Point::from((left_layout.app_x, TOP_BAR_ITEM_Y)),
                Size::from((left_layout.app_w, TOP_BAR_ITEM_H)),
            ),
            COLOR_SURFACE_OVERLAY,
        ));
    }

    // 3. Optional Workspace indicators and layout badge (`menubar_show_workspaces = true`)
    if telemetry.show_workspaces {
        render_top_bar_workspaces(
            elements,
            &left_layout,
            active_workspace,
            layout_mode,
            has_text,
        );
    }

    let ws_y = TOP_BAR_ITEM_Y;

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

fn render_top_bar_workspaces(
    elements: &mut Vec<SolidColorRenderElement>,
    left_layout: &TopBarLeftLayout,
    active_workspace: u8,
    layout_mode: &str,
    has_text: bool,
) {
    elements.push(make_solid_element(
        Rectangle::new(
            Point::from((left_layout.separator_x, 6)),
            Size::from((1, 12)),
        ),
        COLOR_BORDER,
    ));

    let ws_btn_height = TOP_BAR_ITEM_H;
    let ws_y = TOP_BAR_ITEM_Y;
    let mut start_x = left_layout.ws_start_x;

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

    let layout_bg = Rectangle::new(
        Point::from((left_layout.layout_x, ws_y)),
        Size::from((left_layout.layout_w, ws_btn_height)),
    );
    elements.push(make_solid_element(layout_bg, COLOR_SURFACE_OVERLAY));

    let layout_accent = if layout_mode == "master-stack" {
        COLOR_ACCENT
    } else {
        COLOR_SUBTLE
    };
    let layout_strip = Rectangle::new(
        Point::from((left_layout.layout_x, ws_y)),
        Size::from((3, ws_btn_height)),
    );
    elements.push(make_solid_element(layout_strip, layout_accent));
}

fn render_dock(
    elements: &mut Vec<SolidColorRenderElement>,
    width: i32,
    height: i32,
    window_count: usize,
    has_focused_window: bool,
) {
    // 5. Dock Background (floating centered pill at the bottom with outer rim)
    let (dock_x, dock_y, dock_width, dock_items_count) =
        dock_layout_bounds(width, height, window_count);
    let left_count = DOCK_PINNED_COUNT + window_count;

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
    for i in 0..dock_items_count {
        let Some(slot_rect) = dock_slot_rect(width, height, window_count, i) else {
            continue;
        };
        let is_running_window = i >= DOCK_PINNED_COUNT && i < left_count;
        let is_focused = is_running_window && has_focused_window;

        let inner_size = 40;
        let inner_offset = (slot_rect.size.w - inner_size) / 2;
        let inner_geo = Rectangle::new(
            Point::from((
                slot_rect.loc.x + inner_offset,
                slot_rect.loc.y + inner_offset - 2,
            )),
            Size::from((inner_size, inner_size)),
        );
        let inner_color = match i {
            0 => COLOR_ACCENT_MUTED, // Files
            1 => COLOR_SUBTLE,       // Overview
            2 => COLOR_ACCENT,       // Terminal
            3..=5 => COLOR_ACCENT_HOVER,
            _ if is_focused => COLOR_ACCENT_HOVER,
            _ => COLOR_MUTED,
        };
        elements.push(make_solid_element(inner_geo, inner_color));

        // Active running indicator pip under Files / running windows
        if is_running_window || i == 0 {
            let pip_geo = Rectangle::new(
                Point::from((
                    slot_rect.loc.x + (slot_rect.size.w - 4) / 2,
                    dock_y + DOCK_HEIGHT - 7,
                )),
                Size::from((4, 4)),
            );
            elements.push(make_solid_element(pip_geo, COLOR_ACCENT));
        }
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

fn render_quick_settings_slider_solid(
    elements: &mut Vec<SolidColorRenderElement>,
    card: Rectangle<i32, smithay::utils::Physical>,
    track: Rectangle<i32, smithay::utils::Physical>,
    norm: f32,
    fill_color: Color32F,
) {
    elements.push(make_solid_element(card, COLOR_SURFACE_OVERLAY));
    elements.push(make_solid_element(track, COLOR_SURFACE));
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let fill_w = ((track.size.w as f32) * norm.clamp(0.0, 1.0)).round() as i32;
    if fill_w > 0 {
        elements.push(make_solid_element(
            Rectangle::new(track.loc, Size::from((fill_w, track.size.h))),
            fill_color,
        ));
    }
}

fn render_quick_settings_popover(
    elements: &mut Vec<SolidColorRenderElement>,
    width: i32,
    height: i32,
    qs: &QuickSettingsRenderState<'_>,
) {
    let layout = quick_settings_layout(width, height, 0);
    let solid_tiles = [
        (layout.wifi_pill, qs.net_connected || qs.wifi_enabled),
        (layout.bt_pill, qs.bt_available && qs.bt_powered),
        (layout.now_playing_card, false),
        (layout.power_pill, false),
        (layout.layout_btn, qs.layout_mode == "master-stack"),
        (layout.overview_btn, false),
        (layout.motion_btn, qs.reduced_motion),
        (layout.transparency_btn, qs.reduce_transparency),
        (layout.focus_pill, qs.focus_mode),
        (layout.lock_btn, false),
    ];
    for (rect, active) in solid_tiles {
        elements.push(make_solid_element(
            rect,
            if active {
                COLOR_ACCENT_MUTED
            } else {
                COLOR_SURFACE_OVERLAY
            },
        ));
    }

    let display_norm = if qs.brightness_available {
        f32::from(qs.brightness_percent.min(100)) / 100.0
    } else {
        (crate::config::clamp_glass_opacity(qs.glass_opacity) - crate::config::MIN_GLASS_OPACITY)
            / (1.0 - crate::config::MIN_GLASS_OPACITY)
    };
    render_quick_settings_slider_solid(
        elements,
        layout.display_card,
        layout.display_track,
        display_norm,
        COLOR_ACCENT_HOVER,
    );
    render_quick_settings_slider_solid(
        elements,
        layout.sound_card,
        layout.sound_track,
        f32::from(qs.volume_percent.min(100)) / 100.0,
        if qs.audio_muted {
            COLOR_SUBTLE
        } else {
            COLOR_ACCENT
        },
    );
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
        ShellOverlayView::None
        | ShellOverlayView::Overview(_)
        | ShellOverlayView::Switcher(_)
        | ShellOverlayView::Polkit(_)
        | ShellOverlayView::Portal(_)
        | ShellOverlayView::PickColor(_) => {}
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

/// Checks if a mouse click at `(x, y)` was inside the `TopBar` workspaces area when visible and returns the target workspace (1..=9).
#[must_use]
pub fn check_top_bar_workspace_click(x: f64, y: f64) -> Option<u8> {
    check_top_bar_workspace_click_with_options("RustyOS", true, x, y)
}

/// Active-app-width-aware hit-test for the `TopBar` workspaces area (`RUSTYOS-048`).
#[must_use]
pub fn check_top_bar_workspace_click_for_app(active_app_name: &str, x: f64, y: f64) -> Option<u8> {
    check_top_bar_workspace_click_with_options(active_app_name, true, x, y)
}

/// Option-aware hit-test for the optional `TopBar` workspaces area (`RUSTYOS-049`).
#[must_use]
pub fn check_top_bar_workspace_click_with_options(
    active_app_name: &str,
    show_workspaces: bool,
    x: f64,
    y: f64,
) -> Option<u8> {
    if !show_workspaces || y < 0.0 || y > f64::from(TOP_BAR_HEIGHT) {
        return None;
    }
    let left = compute_top_bar_left_layout_with_options(active_app_name, show_workspaces);
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

                // macOS Spotlight action / category badge on the right (`RUSTYOS-039`/`051`)
                let cat_label = if is_sel {
                    "Open ↵"
                } else if item.is_custom_command {
                    "Command"
                } else {
                    "Application"
                };
                let cat_bounds = Rectangle::new(
                    Point::from((modal_x + modal_width - 104, row_y + 14)),
                    Size::from((80, 18)),
                );
                let cat_color = if is_sel {
                    [255, 252, 255, 255]
                } else {
                    text_muted
                };
                if let Some(el) = text_cache
                    .render_label_centered(renderer, cat_label, 10.5, cat_color, cat_bounds)
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
        ShellOverlayView::Polkit(pk) => {
            render_polkit_text(text_cache, renderer, screen_size, pk, &mut elements);
        }
        ShellOverlayView::Portal(portal) => {
            render_portal_text(text_cache, renderer, screen_size, portal, &mut elements);
        }
        ShellOverlayView::PickColor(app_id) => {
            render_pick_color_text(text_cache, renderer, screen_size, app_id, &mut elements);
        }
    }

    // 1b. TopBar Left Active App & Global Menus (`RUSTYOS-038`, `RUSTYOS-048`, `RUSTYOS-049`)
    let ws_btn_height = TOP_BAR_ITEM_H;
    let ws_y = TOP_BAR_ITEM_Y;

    let active_app_name = dock_view
        .windows
        .iter()
        .find(|w| w.focused)
        .map_or_else(|| "RustyOS".to_owned(), |w| extract_app_name(&w.title));
    let opts = top_telemetry.layout_options();
    let left_layout =
        compute_top_bar_left_layout_with_options(&active_app_name, top_telemetry.show_workspaces);

    let app_bounds = Rectangle::new(
        Point::from((left_layout.app_x, ws_y)),
        Size::from((left_layout.app_w, ws_btn_height)),
    );
    let pure_white = [252, 250, 255, 255];
    if let Some(el) = text_cache.render_label_centered_weighted(
        renderer,
        &active_app_name,
        crate::text::FontWeight::Bold,
        TOP_BAR_FONT_SIZE,
        pure_white,
        app_bounds,
    ) {
        elements.push(el);
    }

    let text_menu = [245, 242, 252, 245];
    for (menu_kind, name, x, w) in [
        (
            TopBarMenuHit::File,
            "File",
            left_layout.file_x,
            left_layout.file_w,
        ),
        (
            TopBarMenuHit::Edit,
            "Edit",
            left_layout.edit_x,
            left_layout.edit_w,
        ),
        (
            TopBarMenuHit::View,
            "View",
            left_layout.view_x,
            left_layout.view_w,
        ),
        (TopBarMenuHit::Go, "Go", left_layout.go_x, left_layout.go_w),
        (
            TopBarMenuHit::Window,
            "Window",
            left_layout.win_x,
            left_layout.win_w,
        ),
        (
            TopBarMenuHit::Help,
            "Help",
            left_layout.help_x,
            left_layout.help_w,
        ),
    ] {
        let color = if top_telemetry.active_menu == Some(menu_kind) {
            [255, 255, 255, 255]
        } else {
            text_menu
        };
        let bounds = Rectangle::new(Point::from((x, ws_y)), Size::from((w, ws_btn_height)));
        if let Some(el) = text_cache.render_label_centered_weighted(
            renderer,
            name,
            crate::text::FontWeight::Regular,
            TOP_BAR_FONT_SIZE,
            color,
            bounds,
        ) {
            elements.push(el);
        }
    }

    // Active TopBar left dropdown menu labels + right-aligned real shortcuts (`RUSTYOS-048`/`049`/`051`)
    if let Some(open_menu) = top_telemetry.active_menu {
        let (mx, my, mw, _) = top_bar_menu_popover_rect(&active_app_name, open_menu);
        let mut row_y = my + MENU_POPOVER_PAD_Y;
        for (entry_idx, entry) in top_bar_menu_entries(open_menu).iter().enumerate() {
            match entry {
                TopBarMenuEntry::Item {
                    label, shortcut, ..
                } => {
                    let hovered = top_telemetry.menu_hover_row == Some(entry_idx);
                    let fg = if hovered {
                        [255, 252, 255, 255]
                    } else {
                        text_bright
                    };
                    if let Some(el) = text_cache.render_label(
                        renderer,
                        label,
                        12.0,
                        fg,
                        Point::from((mx + 34, row_y + 6)),
                    ) {
                        elements.push(el);
                    }
                    if let Some(hint) = shortcut {
                        let (hint_w, _) = text_cache.measure_text(hint, 11.0);
                        let hint_fg = if hovered {
                            [248, 240, 255, 255]
                        } else {
                            text_muted
                        };
                        if let Some(el) = text_cache.render_label(
                            renderer,
                            hint,
                            11.0,
                            hint_fg,
                            Point::from((mx + mw - 14 - hint_w, row_y + 7)),
                        ) {
                            elements.push(el);
                        }
                    }
                    row_y += MENU_ITEM_HEIGHT;
                }
                TopBarMenuEntry::Separator => {
                    row_y += MENU_SEPARATOR_HEIGHT;
                }
            }
        }
    }

    // 2. Optional TopBar workspace digits & layout mode (`menubar_show_workspaces = true`)
    if top_telemetry.show_workspaces {
        let mut start_x = left_layout.ws_start_x;
        for ws in 1..=9 {
            let color = if ws == active_workspace {
                [255, 255, 255, 255]
            } else {
                [210, 202, 224, 220]
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

        let layout_label = if layout_mode == "master-stack" {
            "Tile"
        } else {
            "Float"
        };
        let layout_bounds = Rectangle::new(
            Point::from((left_layout.layout_x, ws_y)),
            Size::from((left_layout.layout_w, ws_btn_height)),
        );
        if let Some(el) = text_cache.render_label_centered(
            renderer,
            layout_label,
            11.0,
            text_bright,
            layout_bounds,
        ) {
            elements.push(el);
        }
    }

    // 4. Right status cluster live clock and optional kbd / notification badge / tray (`RUSTYOS-049`)
    let right_layout =
        compute_top_bar_right_layout_with_options(width, top_telemetry.clock_hhmm, opts);
    let clock_bounds = Rectangle::new(
        Point::from((right_layout.clock_x, ws_y)),
        Size::from((right_layout.clock_w, ws_btn_height)),
    );
    if let Some(el) = text_cache.render_label_centered_weighted(
        renderer,
        top_telemetry.clock_hhmm,
        crate::text::FontWeight::Regular,
        TOP_BAR_FONT_SIZE,
        pure_white,
        clock_bounds,
    ) {
        elements.push(el);
    }

    if let Some(kbd_x) = right_layout.kbd_x {
        let kbd_bounds = Rectangle::new(
            Point::from((kbd_x, ws_y)),
            Size::from((right_layout.kbd_w, ws_btn_height)),
        );
        if let Some(el) = text_cache.render_label_centered(
            renderer,
            top_telemetry.kbd_layout,
            10.5,
            pure_white,
            kbd_bounds,
        ) {
            elements.push(el);
        }
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
        if let Some(el) =
            text_cache.render_label_centered(renderer, &badge_label, 10.5, pure_white, badge_bounds)
        {
            elements.push(el);
        }
    }

    if top_telemetry.show_tray {
        let tray_rects = tray_slot_rects_with_options(
            width,
            top_telemetry.clock_hhmm,
            opts,
            top_telemetry.tray_items.len(),
        );
        for (item, (tx, _ty, tw, _th)) in top_telemetry.tray_items.iter().zip(tray_rects) {
            let has_tray_icon = item.icon_pixmap.is_some()
                || (!item.icon_name.is_empty()
                    && shape_cache.icons.has_named_icon(item.icon_name, 16));
            if has_tray_icon {
                continue;
            }
            let label_color = if item.attention {
                [255, 210, 214, 255]
            } else if item.passive {
                text_muted
            } else {
                pure_white
            };
            let tray_bounds =
                Rectangle::new(Point::from((tx, ws_y)), Size::from((tw, ws_btn_height)));
            if let Some(el) = text_cache.render_label_centered(
                renderer,
                item.label,
                9.5,
                label_color,
                tray_bounds,
            ) {
                elements.push(el);
            }
        }
    }

    // 4b. Floating top-right Notification HUD toast labels (`RUSTYOS-024`/`051`)
    if !matches!(
        overlay,
        ShellOverlayView::Spotlight(_)
            | ShellOverlayView::QuickSettings(_)
            | ShellOverlayView::Polkit(_)
            | ShellOverlayView::Portal(_)
    ) && !notifications.is_empty()
    {
        let rects = notification_toast_rects(screen_size, notifications.len());
        for (toast, card) in notifications.iter().zip(rects) {
            let tx = card.loc.x;
            let ty = card.loc.y;

            let title_text = truncate_ascii(toast.title, 28);
            if let Some(el) = text_cache.render_label(
                renderer,
                &title_text,
                12.5,
                text_bright,
                Point::from((tx + 62, ty + 13)),
            ) {
                elements.push(el);
            }

            let body_text = truncate_ascii(toast.body, 36);
            if let Some(el) = text_cache.render_label(
                renderer,
                &body_text,
                10.5,
                text_muted,
                Point::from((tx + 62, ty + 35)),
            ) {
                elements.push(el);
            }
        }
    }

    // 5. Dock hover tooltip label (`RUSTYOS-050`)
    if !matches!(
        overlay,
        ShellOverlayView::Spotlight(_)
            | ShellOverlayView::QuickSettings(_)
            | ShellOverlayView::Switcher(_)
            | ShellOverlayView::Polkit(_)
            | ShellOverlayView::Portal(_)
    ) && let Some((tooltip_rect, tooltip_label)) =
        dock_tooltip_rect_and_label(width, height, dock_view)
        && let Some(el) = text_cache.render_label_centered_weighted(
            renderer,
            &tooltip_label,
            crate::text::FontWeight::Bold,
            12.0,
            [252, 250, 255, 255],
            Rectangle::new(
                Point::from((tooltip_rect.loc.x, tooltip_rect.loc.y)),
                Size::from((tooltip_rect.size.w, tooltip_rect.size.h)),
            ),
        )
    {
        elements.push(el);
    }
    let _ = shape_cache;

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
    let text_muted = [188, 178, 206, 255];
    let width = screen_size.w;
    let heading_dy = anim_offset(ov.progress, 12.0);

    // Top Spaces miniature captions ("Desktop 1", "Desktop 2", ...) matching Mission Control (`RUSTYOS-051`)
    if !ov.spaces.is_empty() {
        let space_rects = overview_space_rects(screen_size, ov.spaces.len());
        for (space, rect) in ov.spaces.iter().zip(space_rects) {
            let label = format!("Desktop {}", space.id);
            let label_bounds = Rectangle::new(
                Point::from((rect.loc.x - 4, rect.loc.y + rect.size.h + 4 + heading_dy)),
                Size::from((rect.size.w + 8, 16)),
            );
            let color = if space.active {
                text_bright
            } else {
                text_muted
            };
            if let Some(el) =
                text_cache.render_label_centered(renderer, &label, 10.5, color, label_bounds)
            {
                elements.push(el);
            }
        }
    }

    if ov.windows.is_empty() {
        let empty_msg = format!("Desktop {} is empty", ov.active_workspace);
        if let Some(el) = text_cache.render_label_centered(
            renderer,
            &empty_msg,
            13.5,
            text_muted,
            Rectangle::new(
                Point::from((32, screen_size.h / 2 - 14 + heading_dy)),
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
        let label = truncate_ascii(&win.title, 26);
        let footer_bounds = Rectangle::new(
            Point::from((card.loc.x + 8, card.loc.y + card.size.h + 5 + card_dy)),
            Size::from((card.size.w - 16, 20)),
        );
        if let Some(el) = text_cache.render_label_centered(
            renderer,
            &label,
            11.5,
            if win.focused { text_bright } else { text_muted },
            footer_bounds,
        ) {
            elements.push(el);
        }
        let body_bounds = Rectangle::new(
            Point::from((card.loc.x + 12, card.loc.y + 12 + card_dy)),
            Size::from((card.size.w - 24, card.size.h - 24)),
        );
        let center_badge = format!("{}  •  {}", win.badge, win.dimensions);
        if let Some(el) = text_cache.render_label_centered(
            renderer,
            &center_badge,
            11.5,
            if win.focused { text_bright } else { text_muted },
            body_bounds,
        ) {
            elements.push(el);
        }
    }
}

const QS_TEXT_BRIGHT: [u8; 4] = [255, 253, 255, 255];
const QS_TEXT_MUTED: [u8; 4] = [226, 222, 238, 240];

fn concise_wifi_subtitle(label: &str, connected: bool, wifi_enabled: bool) -> String {
    let cleaned = label
        .trim()
        .strip_prefix("Wi-Fi:")
        .map_or_else(|| label.trim(), str::trim);
    if !wifi_enabled && !connected {
        "Off".to_owned()
    } else if !connected {
        "Not Connected".to_owned()
    } else if cleaned.eq_ignore_ascii_case("Wi-Fi Connected") || cleaned.is_empty() {
        "Connected".to_owned()
    } else {
        truncate_ascii(cleaned, 11)
    }
}

fn concise_bt_subtitle(bt_label: &str, bt_available: bool, bt_powered: bool) -> String {
    if !bt_available {
        return "Unavailable".to_owned();
    }
    if !bt_powered {
        return "Off".to_owned();
    }
    let cleaned = bt_label
        .trim()
        .strip_prefix("BT:")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("On");
    truncate_ascii(cleaned, 11)
}

fn render_control_center_capsule_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    pill: Rectangle<i32, smithay::utils::Physical>,
    title: &str,
    sub: &str,
    bright: bool,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let top_y = pill.loc.y + (pill.size.h - 29) / 2;
    if let Some(el) = text_cache.render_label_weighted(
        renderer,
        title,
        crate::text::FontWeight::Bold,
        12.0,
        if bright {
            QS_TEXT_BRIGHT
        } else {
            QS_TEXT_MUTED
        },
        Point::from((pill.loc.x + 54, top_y)),
    ) {
        elements.push(el);
    }
    if let Some(el) = text_cache.render_label(
        renderer,
        sub,
        10.5,
        QS_TEXT_MUTED,
        Point::from((pill.loc.x + 54, top_y + 16)),
    ) {
        elements.push(el);
    }
}

fn render_cc_secondary_labels(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    layout: &QuickSettingsLayout,
    qs: &QuickSettingsRenderState<'_>,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let media_label = if qs.media_active && !qs.media_title.trim().is_empty() {
        truncate_ascii(qs.media_title.trim(), 15)
    } else {
        "Not Playing".to_owned()
    };
    if let Some(el) = text_cache.render_label_weighted(
        renderer,
        &media_label,
        crate::text::FontWeight::Bold,
        12.0,
        QS_TEXT_BRIGHT,
        Point::from((
            layout.now_playing_card.loc.x + 14,
            layout.now_playing_card.loc.y + 53,
        )),
    ) {
        elements.push(el);
    }

    if qs.focus_mode {
        render_control_center_capsule_text(
            text_cache,
            renderer,
            layout.focus_pill,
            "Focus",
            "On",
            true,
            elements,
        );
    } else if let Some(el) = text_cache.render_label_weighted(
        renderer,
        "Focus",
        crate::text::FontWeight::Bold,
        12.0,
        QS_TEXT_BRIGHT,
        Point::from((
            layout.focus_pill.loc.x + 56,
            layout.focus_pill.loc.y + (layout.focus_pill.size.h - 15) / 2,
        )),
    ) {
        elements.push(el);
    }

    for (card, title) in [
        (layout.display_card, "Display"),
        (layout.sound_card, "Sound"),
    ] {
        if let Some(el) = text_cache.render_label_weighted(
            renderer,
            title,
            crate::text::FontWeight::Bold,
            12.0,
            QS_TEXT_BRIGHT,
            Point::from((card.loc.x + 14, card.loc.y + 9)),
        ) {
            elements.push(el);
        }
    }

    if let Some(el) = text_cache.render_label_centered_weighted(
        renderer,
        "Lock Screen",
        crate::text::FontWeight::Bold,
        11.0,
        QS_TEXT_BRIGHT,
        Rectangle::new(
            Point::from((layout.lock_btn.loc.x, layout.lock_btn.loc.y)),
            Size::from((layout.lock_btn.size.w, layout.lock_btn.size.h)),
        ),
    ) {
        elements.push(el);
    }
}

fn render_quick_settings_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    width: i32,
    height: i32,
    qs: &QuickSettingsRenderState<'_>,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let layout = quick_settings_layout(width, height, anim_offset(qs.progress, -16.0));
    let _ = qs.frame_p50_us;
    let _ = qs.window_count;

    let wifi_sub = concise_wifi_subtitle(qs.network_label, qs.net_connected, qs.wifi_enabled);
    render_control_center_capsule_text(
        text_cache,
        renderer,
        layout.wifi_pill,
        "Wi-Fi",
        &wifi_sub,
        true,
        elements,
    );

    let bt_sub = concise_bt_subtitle(qs.bt_label, qs.bt_available, qs.bt_powered);
    render_control_center_capsule_text(
        text_cache,
        renderer,
        layout.bt_pill,
        "Bluetooth",
        &bt_sub,
        qs.bt_available,
        elements,
    );

    let power_sub = if qs.has_battery {
        if qs.ac_online {
            format!("{}% • AC", qs.battery_percent)
        } else {
            format!("{}%", qs.battery_percent)
        }
    } else {
        "AC Power".to_owned()
    };
    render_control_center_capsule_text(
        text_cache,
        renderer,
        layout.power_pill,
        "Power",
        &power_sub,
        true,
        elements,
    );

    render_cc_secondary_labels(text_cache, renderer, &layout, qs, elements);
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
const RGBA_ACCENT_MUTED: [u8; 4] = [118, 80, 181, 255]; // #7650B5
const RGBA_BORDER: [u8; 4] = [255, 255, 255, 31];
const RGBA_BORDER_HIGHLIGHT: [u8; 4] = [255, 255, 255, 56];
const RGBA_DANGER: [u8; 4] = [229, 107, 117, 255];

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

/// Draws monochrome macOS Menu Bar glyphs (`[Volume] · [Battery] · Wi-Fi · Search · Control Center`) (`RUSTYOS-036`, `RUSTYOS-049`).
fn push_top_bar_right_glyphs(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    right: &TopBarRightLayout,
    t: &TopBarTelemetry<'_>,
) {
    use crate::surfaces::UiGlyphKind;

    let white = [252, 250, 255, 255];

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
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        UiGlyphKind::Wifi {
            connected: t.net_connected,
            signal_percent: t.wifi_signal_percent,
        },
        20,
        white,
        Point::from((right.wifi_x, TOP_BAR_ITEM_Y)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
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

#[allow(clippy::too_many_arguments)]
fn push_liquid_glass_panel(
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

fn notification_app_icon(title: &str) -> crate::icons::BuiltinAppIcon {
    let lower = title.to_ascii_lowercase();
    if lower.contains("term")
        || lower.contains("shell")
        || lower.contains("bash")
        || lower.contains("konsole")
    {
        crate::icons::BuiltinAppIcon::Terminal
    } else if lower.contains("file") || lower.contains("download") || lower.contains("dolphin") {
        crate::icons::BuiltinAppIcon::Files
    } else if lower.contains("browser")
        || lower.contains("web")
        || lower.contains("firefox")
        || lower.contains("chrom")
    {
        crate::icons::BuiltinAppIcon::Browser
    } else if lower.contains("setting") || lower.contains("config") || lower.contains("pref") {
        crate::icons::BuiltinAppIcon::Settings
    } else {
        crate::icons::BuiltinAppIcon::RustySystem
    }
}

fn osd_glyph_kind(
    label: &str,
    value_percent: Option<u8>,
    muted: bool,
) -> crate::surfaces::UiGlyphKind {
    let lower = label.to_ascii_lowercase();
    if lower.contains("bright") || lower.contains("glass") || lower.contains("display") {
        crate::surfaces::UiGlyphKind::Sun
    } else if lower.contains("vol")
        || lower.contains("audio")
        || lower.contains("sound")
        || lower.contains("mute")
        || value_percent.is_some()
    {
        crate::surfaces::UiGlyphKind::Volume {
            level_percent: value_percent.unwrap_or(50),
            muted,
        }
    } else {
        crate::surfaces::UiGlyphKind::Info
    }
}

#[allow(clippy::too_many_arguments)]
fn push_cc_capsule_icon(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    pill: Rectangle<i32, smithay::utils::Physical>,
    active: bool,
    glyph: crate::surfaces::UiGlyphKind,
    active_glyph_rgba: [u8; 4],
    inactive_glyph_rgba: [u8; 4],
) {
    let circle_d = 36;
    let circle_x = pill.loc.x + 10;
    let circle_y = pill.loc.y + (pill.size.h - circle_d) / 2;
    let glyph_size = 17;
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        glyph,
        glyph_size,
        if active {
            active_glyph_rgba
        } else {
            inactive_glyph_rgba
        },
        Point::from((
            circle_x + (circle_d - glyph_size) / 2,
            circle_y + (circle_d - glyph_size) / 2,
        )),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    let circle_fill = if active {
        [252, 252, 255, 252]
    } else {
        [255, 255, 255, 42]
    };
    let circle_border = if active {
        [255, 255, 255, 255]
    } else {
        [255, 255, 255, 62]
    };
    if let Some(el) = shape_cache.circle_element(
        renderer,
        circle_d,
        circle_fill,
        Some(circle_border),
        Point::from((circle_x, circle_y)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
}

fn push_cc_now_playing_contents(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    layout: &QuickSettingsLayout,
    qs: &QuickSettingsRenderState<'_>,
) {
    let card = layout.now_playing_card;
    let art_rect = Rectangle::new(
        Point::from((card.loc.x + 14, card.loc.y + 12)),
        Size::from((36, 36)),
    );
    let note_rgba = if qs.media_active {
        [252, 250, 255, 245]
    } else {
        [244, 240, 255, 130]
    };
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        crate::surfaces::UiGlyphKind::MusicNote,
        16,
        note_rgba,
        Point::from((art_rect.loc.x + 10, art_rect.loc.y + 10)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    let art_bg = if qs.media_active {
        [108, 90, 224, 215]
    } else {
        [255, 255, 255, 38]
    };
    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        art_rect,
        9,
        art_bg,
        Some([255, 255, 255, 44]),
    );

    let play_kind = if qs.media_playing {
        crate::surfaces::UiGlyphKind::MediaPause
    } else {
        crate::surfaces::UiGlyphKind::MediaPlay
    };
    let transport_tint = if qs.media_active {
        [255, 252, 255, 248]
    } else {
        [240, 236, 248, 200]
    };
    for (btn_rect, kind, g_size) in [
        (
            layout.media_prev_btn,
            crate::surfaces::UiGlyphKind::MediaPrev,
            17,
        ),
        (layout.media_play_btn, play_kind, 20),
        (
            layout.media_next_btn,
            crate::surfaces::UiGlyphKind::MediaNext,
            17,
        ),
    ] {
        if let Some(el) = shape_cache.ui_glyph_element(
            renderer,
            kind,
            g_size,
            transport_tint,
            Point::from((
                btn_rect.loc.x + (btn_rect.size.w - g_size) / 2,
                btn_rect.loc.y + (btn_rect.size.h - g_size) / 2,
            )),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
    }
}

fn push_cc_thin_slider(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    track: Rectangle<i32, smithay::utils::Physical>,
    norm: f32,
    active_fill: bool,
) {
    let bar_h = 6;
    let bar_y = track.loc.y + (track.size.h - bar_h) / 2;
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let fill_w = ((track.size.w as f32) * norm.clamp(0.0, 1.0))
        .round()
        .clamp(6.0, track.size.w as f32) as i32;
    let fill_rgba = if active_fill {
        [255, 253, 255, 245]
    } else {
        [190, 184, 204, 185]
    };
    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        Rectangle::new(
            Point::from((track.loc.x, bar_y)),
            Size::from((fill_w, bar_h)),
        ),
        bar_h / 2,
        fill_rgba,
        None,
    );
    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        Rectangle::new(
            Point::from((track.loc.x, bar_y)),
            Size::from((track.size.w, bar_h)),
        ),
        bar_h / 2,
        [255, 255, 255, 58],
        None,
    );
}

fn push_cc_display_slider(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    layout: &QuickSettingsLayout,
    qs: &QuickSettingsRenderState<'_>,
) {
    let white = [255, 253, 255, 245];
    let disp_cy = layout.display_track.loc.y + layout.display_track.size.h / 2;
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        crate::surfaces::UiGlyphKind::Sun,
        12,
        white,
        Point::from((layout.display_card.loc.x + 14, disp_cy - 6)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        crate::surfaces::UiGlyphKind::Sun,
        16,
        white,
        Point::from((
            layout.display_card.loc.x + layout.display_card.size.w - 28,
            disp_cy - 8,
        )),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    let disp_norm = if qs.brightness_available {
        f32::from(qs.brightness_percent.min(100)) / 100.0
    } else {
        (crate::config::clamp_glass_opacity(qs.glass_opacity) - crate::config::MIN_GLASS_OPACITY)
            / (1.0 - crate::config::MIN_GLASS_OPACITY)
    };
    push_cc_thin_slider(
        out,
        shape_cache,
        renderer,
        layout.display_track,
        disp_norm,
        true,
    );
}

fn push_cc_sound_slider(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    layout: &QuickSettingsLayout,
    qs: &QuickSettingsRenderState<'_>,
) {
    let white = [255, 253, 255, 245];
    let snd_cy = layout.sound_track.loc.y + layout.sound_track.size.h / 2;
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        crate::surfaces::UiGlyphKind::Volume {
            level_percent: 0,
            muted: false,
        },
        13,
        white,
        Point::from((layout.sound_card.loc.x + 13, snd_cy - 6)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        crate::surfaces::UiGlyphKind::Volume {
            level_percent: 90,
            muted: false,
        },
        15,
        white,
        Point::from((
            layout.sound_card.loc.x + layout.sound_card.size.w - 64,
            snd_cy - 7,
        )),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    let mute_fg = if qs.audio_muted {
        [248, 86, 86, 255]
    } else {
        white
    };
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        crate::surfaces::UiGlyphKind::Volume {
            level_percent: qs.volume_percent,
            muted: qs.audio_muted,
        },
        15,
        mute_fg,
        Point::from((
            layout.sound_mute_zone.loc.x + (layout.sound_mute_zone.size.w - 15) / 2,
            layout.sound_mute_zone.loc.y + (layout.sound_mute_zone.size.h - 15) / 2,
        )),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    let mute_circle_bg = if qs.audio_muted {
        [252, 252, 255, 245]
    } else {
        [255, 255, 255, 42]
    };
    if let Some(el) = shape_cache.circle_element(
        renderer,
        layout.sound_mute_zone.size.w,
        mute_circle_bg,
        Some([255, 255, 255, 68]),
        Point::from((layout.sound_mute_zone.loc.x, layout.sound_mute_zone.loc.y)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    let vol_norm = f32::from(qs.volume_percent.min(100)) / 100.0;
    push_cc_thin_slider(
        out,
        shape_cache,
        renderer,
        layout.sound_track,
        vol_norm,
        !qs.audio_muted,
    );
}

fn push_cc_round_toggle_buttons(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    layout: &QuickSettingsLayout,
    qs: &QuickSettingsRenderState<'_>,
) {
    let white = [255, 252, 255, 255];
    let is_tile = qs.layout_mode == "master-stack";
    for (btn_rect, active, kind) in [
        (
            layout.layout_btn,
            is_tile,
            crate::surfaces::UiGlyphKind::LayoutSplit,
        ),
        (
            layout.overview_btn,
            false,
            crate::surfaces::UiGlyphKind::OverviewSpaces,
        ),
        (
            layout.motion_btn,
            qs.reduced_motion,
            crate::surfaces::UiGlyphKind::Motion,
        ),
        (
            layout.transparency_btn,
            qs.reduce_transparency,
            crate::surfaces::UiGlyphKind::Transparency,
        ),
    ] {
        let g_size = 21;
        if let Some(el) = shape_cache.ui_glyph_element(
            renderer,
            kind,
            g_size,
            white,
            Point::from((
                btn_rect.loc.x + (btn_rect.size.w - g_size) / 2,
                btn_rect.loc.y + (btn_rect.size.h - g_size) / 2,
            )),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
        if active
            && let Some(el) = shape_cache.circle_element(
                renderer,
                btn_rect.size.w,
                [88, 134, 252, 118],
                Some([255, 255, 255, 110]),
                Point::from((btn_rect.loc.x, btn_rect.loc.y)),
            )
        {
            out.push(CompositorRenderElement::Memory(el));
        }
    }
}

fn render_quick_settings_composite(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    width: i32,
    height: i32,
    qs: &QuickSettingsRenderState<'_>,
) {
    let layout = quick_settings_layout(width, height, anim_offset(qs.progress, -16.0));
    let white = [255, 252, 255, 255];
    let blue_active = [24, 128, 250, 255];

    let wifi_active = qs.net_connected || qs.wifi_enabled;
    push_cc_capsule_icon(
        out,
        shape_cache,
        renderer,
        layout.wifi_pill,
        wifi_active,
        crate::surfaces::UiGlyphKind::Wifi {
            connected: qs.net_connected,
            signal_percent: qs.wifi_signal_percent,
        },
        blue_active,
        white,
    );

    let bt_active = qs.bt_available && qs.bt_powered;
    push_cc_capsule_icon(
        out,
        shape_cache,
        renderer,
        layout.bt_pill,
        bt_active,
        crate::surfaces::UiGlyphKind::Bluetooth,
        blue_active,
        if qs.bt_available {
            white
        } else {
            [210, 204, 224, 170]
        },
    );

    let power_healthy = qs.ac_online || qs.battery_charging || qs.battery_percent > 20;
    let power_icon_active = if qs.ac_online || qs.battery_charging {
        [22, 163, 74, 255]
    } else if power_healthy {
        blue_active
    } else {
        [239, 68, 68, 255]
    };
    push_cc_capsule_icon(
        out,
        shape_cache,
        renderer,
        layout.power_pill,
        power_healthy,
        crate::surfaces::UiGlyphKind::Battery {
            percent: qs.battery_percent,
            charging: qs.ac_online || qs.battery_charging,
        },
        power_icon_active,
        white,
    );

    push_cc_now_playing_contents(out, shape_cache, renderer, &layout, qs);
    push_cc_round_toggle_buttons(out, shape_cache, renderer, &layout, qs);

    push_cc_capsule_icon(
        out,
        shape_cache,
        renderer,
        layout.focus_pill,
        qs.focus_mode,
        crate::surfaces::UiGlyphKind::Moon,
        [106, 84, 212, 255],
        white,
    );

    push_cc_display_slider(out, shape_cache, renderer, &layout, qs);
    push_cc_sound_slider(out, shape_cache, renderer, &layout, qs);

    for (rect, radius) in [
        (layout.wifi_pill, layout.wifi_pill.size.h / 2),
        (layout.bt_pill, layout.bt_pill.size.h / 2),
        (layout.now_playing_card, 24),
        (layout.power_pill, layout.power_pill.size.h / 2),
        (layout.layout_btn, layout.layout_btn.size.h / 2),
        (layout.overview_btn, layout.overview_btn.size.h / 2),
        (layout.motion_btn, layout.motion_btn.size.h / 2),
        (layout.transparency_btn, layout.transparency_btn.size.h / 2),
        (layout.focus_pill, layout.focus_pill.size.h / 2),
        (layout.display_card, 22),
        (layout.sound_card, 22),
        (layout.lock_btn, layout.lock_btn.size.h / 2),
    ] {
        push_liquid_glass_panel(
            out,
            shape_cache,
            renderer,
            rect,
            radius,
            crate::surfaces::GlassPanelStyle::ControlModule,
            qs.glass_opacity,
            qs.reduce_transparency,
            Some((10, 26)),
        );
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

            // Search input bar plate (`macOS 27` Liquid Glass module, `RUSTYOS-039`/`051`)
            push_liquid_glass_panel(
                &mut out,
                shape_cache,
                renderer,
                Rectangle::new(Point::from((mx + 16, my + 16)), Size::from((mw - 32, 48))),
                14,
                crate::surfaces::GlassPanelStyle::ControlModule,
                glass_op,
                reduce_tr,
                None,
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

                let row_rgba = if is_sel {
                    [165, 110, 255, 198]
                } else {
                    RGBA_SURFACE
                };
                if is_sel {
                    push_rounded_glass_box(
                        &mut out,
                        shape_cache,
                        renderer,
                        Rectangle::new(Point::from((mx + 14, row_y)), Size::from((mw - 28, row_h))),
                        12,
                        row_rgba,
                        Some([215, 185, 255, 120]),
                    );
                } else {
                    push_rounded_glass_box(
                        &mut out,
                        shape_cache,
                        renderer,
                        Rectangle::new(Point::from((mx + 16, row_y)), Size::from((mw - 32, row_h))),
                        12,
                        row_rgba,
                        None,
                    );
                }
            }

            let modal_rect = Rectangle::new(Point::from((mx, my)), Size::from((mw, mh)));
            push_liquid_glass_panel(
                &mut out,
                shape_cache,
                renderer,
                modal_rect,
                WINDOW_RADIUS,
                crate::surfaces::GlassPanelStyle::Popover,
                glass_op,
                reduce_tr,
                Some((20, 58)),
            );
        }
        ShellOverlayView::QuickSettings(qs) => {
            render_quick_settings_composite(&mut out, shape_cache, renderer, width, height, qs);
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
                push_soft_shadow(&mut out, shape_cache, renderer, panel_rect, 14, 14, 52);
            }
        }
        ShellOverlayView::Polkit(pk) => {
            render_polkit_composite(
                &mut out,
                shape_cache,
                renderer,
                screen_size,
                top_telemetry,
                pk,
            );
        }
        ShellOverlayView::Portal(portal) => {
            render_portal_composite(
                &mut out,
                shape_cache,
                renderer,
                screen_size,
                top_telemetry,
                portal,
            );
        }
        ShellOverlayView::PickColor(_) => {
            render_pick_color_composite(
                &mut out,
                shape_cache,
                renderer,
                screen_size,
                top_telemetry,
            );
        }
    }

    // 2b. Floating top-right Notification HUD toasts (`RUSTYOS-024`/`051`)
    if !matches!(
        overlay,
        ShellOverlayView::Spotlight(_)
            | ShellOverlayView::QuickSettings(_)
            | ShellOverlayView::Polkit(_)
            | ShellOverlayView::Portal(_)
    ) && !notifications.is_empty()
    {
        let rects = notification_toast_rects(screen_size, notifications.len());
        for (toast, card) in notifications.iter().zip(rects) {
            let tx = card.loc.x;
            let ty = card.loc.y;
            let tw = card.size.w;

            if toast.hovered {
                if let Some(el) = shape_cache.ui_glyph_element(
                    renderer,
                    crate::surfaces::UiGlyphKind::Close,
                    10,
                    [248, 244, 255, 255],
                    Point::from((tx + tw - 23, ty + 15)),
                ) {
                    out.push(CompositorRenderElement::Memory(el));
                }
                if let Some(el) = shape_cache.circle_element(
                    renderer,
                    20,
                    [255, 255, 255, 42],
                    Some([255, 255, 255, 76]),
                    Point::from((tx + tw - 28, ty + 10)),
                ) {
                    out.push(CompositorRenderElement::Memory(el));
                }
            }

            let app_icon = notification_app_icon(toast.title);
            if let Some(icon_el) = shape_cache.icons.builtin_squircle_icon_element(
                renderer,
                app_icon,
                38,
                Point::from((tx + 14, ty + 15)),
            ) {
                out.push(CompositorRenderElement::Memory(icon_el));
            }

            push_liquid_glass_panel(
                &mut out,
                shape_cache,
                renderer,
                card,
                MENU_POPOVER_RADIUS,
                crate::surfaces::GlassPanelStyle::Popover,
                top_telemetry.glass_opacity,
                top_telemetry.reduce_transparency,
                Some((12, 46)),
            );
        }
    }

    // 3. TopBar macOS Menu Bar SDF glyphs, optional tray/workspaces, menu popover & transparent default (`RUSTYOS-048`, `RUSTYOS-049`)
    let active_app_name = dock_view
        .windows
        .iter()
        .find(|w| w.focused)
        .map_or_else(|| "RustyOS".to_owned(), |w| extract_app_name(&w.title));
    let opts = top_telemetry.layout_options();
    let left_layout =
        compute_top_bar_left_layout_with_options(&active_app_name, top_telemetry.show_workspaces);
    let right_layout =
        compute_top_bar_right_layout_with_options(width, top_telemetry.clock_hhmm, opts);

    let pure_white = [252, 250, 255, 255];
    let logo_loc = Point::from((TOP_BAR_LOGO_X + 1, TOP_BAR_ITEM_Y + 1));
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        crate::surfaces::UiGlyphKind::RustyLogo,
        18,
        pure_white,
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

    if let Some(open_menu) = top_telemetry.active_menu {
        let (anchor_x, anchor_w) = match open_menu {
            TopBarMenuHit::SystemLogo => (TOP_BAR_LOGO_X - 2, TOP_BAR_LOGO_W + 4),
            TopBarMenuHit::ActiveApp => (left_layout.app_x - 2, left_layout.app_w + 4),
            TopBarMenuHit::File => (left_layout.file_x - 2, left_layout.file_w + 4),
            TopBarMenuHit::Edit => (left_layout.edit_x - 2, left_layout.edit_w + 4),
            TopBarMenuHit::View => (left_layout.view_x - 2, left_layout.view_w + 4),
            TopBarMenuHit::Go => (left_layout.go_x - 2, left_layout.go_w + 4),
            TopBarMenuHit::Window => (left_layout.win_x - 2, left_layout.win_w + 4),
            TopBarMenuHit::Help => (left_layout.help_x - 2, left_layout.help_w + 4),
        };
        push_rounded_glass_box(
            &mut out,
            shape_cache,
            renderer,
            Rectangle::new(
                Point::from((anchor_x, TOP_BAR_ITEM_Y)),
                Size::from((anchor_w, TOP_BAR_ITEM_H)),
            ),
            5,
            [255, 255, 255, 42],
            None,
        );
        let (mx, my, mw, mh) = top_bar_menu_popover_rect(&active_app_name, open_menu);
        let menu_rect = Rectangle::new(Point::from((mx, my)), Size::from((mw, mh)));
        let mut row_y = my + MENU_POPOVER_PAD_Y;
        for (entry_idx, entry) in top_bar_menu_entries(open_menu).iter().enumerate() {
            match entry {
                TopBarMenuEntry::Item { icon, .. } => {
                    let hovered = top_telemetry.menu_hover_row == Some(entry_idx);
                    let icon_tint = if hovered {
                        [255, 252, 255, 255]
                    } else {
                        [218, 208, 238, 240]
                    };
                    if let Some(el) = shape_cache.ui_glyph_element(
                        renderer,
                        *icon,
                        14,
                        icon_tint,
                        Point::from((mx + 14, row_y + 7)),
                    ) {
                        out.push(CompositorRenderElement::Memory(el));
                    }
                    if hovered {
                        push_rounded_glass_box(
                            &mut out,
                            shape_cache,
                            renderer,
                            Rectangle::new(
                                Point::from((mx + 6, row_y + 1)),
                                Size::from((mw - 12, MENU_ITEM_HEIGHT - 2)),
                            ),
                            8,
                            [165, 110, 255, 200],
                            Some([215, 185, 255, 110]),
                        );
                    }
                    row_y += MENU_ITEM_HEIGHT;
                }
                TopBarMenuEntry::Separator => {
                    out.push(CompositorRenderElement::Solid(make_solid_element(
                        Rectangle::new(Point::from((mx + 12, row_y + 4)), Size::from((mw - 24, 1))),
                        COLOR_BORDER,
                    )));
                    row_y += MENU_SEPARATOR_HEIGHT;
                }
            }
        }
        push_liquid_glass_panel(
            &mut out,
            shape_cache,
            renderer,
            menu_rect,
            MENU_POPOVER_RADIUS,
            crate::surfaces::GlassPanelStyle::Popover,
            top_telemetry.glass_opacity,
            top_telemetry.reduce_transparency,
            Some((16, 52)),
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
                Point::from((right_layout.control_center_x - 5, TOP_BAR_ITEM_Y)),
                Size::from((right_layout.control_center_w + 10, TOP_BAR_ITEM_H)),
            ),
            6,
            [255, 255, 255, 58],
            Some([255, 255, 255, 42]),
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
    }

    // Fully transparent TopBar by default (`reduce_transparency == false`);
    // render solid high-contrast bar only when `reduce_transparency` is enabled (`RUSTYOS-049`).
    if top_telemetry.reduce_transparency {
        out.push(CompositorRenderElement::Solid(make_solid_element(
            Rectangle::new(Point::from((0, TOP_BAR_HEIGHT - 1)), Size::from((width, 1))),
            COLOR_BORDER,
        )));
        out.push(CompositorRenderElement::Solid(make_solid_element(
            Rectangle::new(Point::from((0, 0)), Size::from((width, TOP_BAR_HEIGHT))),
            rgba_to_premul_color32f([20, 15, 32, 245]),
        )));
    }

    // 4. Dock macOS glass container, 4x4 SSAA icons, soft drop shadows, hover magnification, tooltip & divider (`RUSTYOS-050`)
    if !matches!(
        overlay,
        ShellOverlayView::Spotlight(_)
            | ShellOverlayView::QuickSettings(_)
            | ShellOverlayView::Switcher(_)
            | ShellOverlayView::Polkit(_)
            | ShellOverlayView::Portal(_)
    ) && let Some((tooltip_rect, _)) = dock_tooltip_rect_and_label(width, height, dock_view)
    {
        push_rounded_glass_box(
            &mut out,
            shape_cache,
            renderer,
            tooltip_rect,
            9,
            scale_glass_rgba(
                [42, 34, 62, 224],
                top_telemetry.glass_opacity,
                top_telemetry.reduce_transparency,
            ),
            Some(RGBA_BORDER_HIGHLIGHT),
        );
        push_soft_shadow(&mut out, shape_cache, renderer, tooltip_rect, 9, 8, 34);
    }

    let (dock_x, dock_y, dock_width, dock_items_count) =
        dock_layout_bounds(width, height, dock_view.windows.len());
    let left_count = DOCK_PINNED_COUNT + dock_view.windows.len();
    let _ = dock_view.pinned_icons;

    let mut dock_icons = Vec::with_capacity(dock_items_count);
    let mut dock_dots_and_divider = Vec::new();

    if let Some(last_app_slot) =
        dock_slot_rect(width, height, dock_view.windows.len(), left_count - 1)
    {
        let sep_x = last_app_slot.loc.x
            + last_app_slot.size.w
            + i32::midpoint(DOCK_ITEM_GAP, DOCK_DIVIDER_GAP);
        dock_dots_and_divider.push(CompositorRenderElement::Solid(make_solid_element(
            Rectangle::new(Point::from((sep_x, dock_y + 15)), Size::from((1, 38))),
            rgba_to_premul_color32f([255, 255, 255, 64]),
        )));
    }

    for i in 0..dock_items_count {
        let Some(slot_rect) = dock_slot_rect(width, height, dock_view.windows.len(), i) else {
            continue;
        };
        let item_x = slot_rect.loc.x;
        let item_size = slot_rect.size.w;
        let hovered = dock_view.hover_slot == Some(i);
        let hover_factor = if hovered {
            if dock_view.reduced_motion {
                0.25
            } else {
                dock_view.hover_progress.clamp(0.35, 1.0)
            }
        } else {
            0.0
        };
        #[allow(clippy::cast_possible_truncation)]
        let lift = if hovered {
            if dock_view.reduced_motion {
                2
            } else {
                (6.0 * hover_factor).round() as i32
            }
        } else {
            0
        };
        #[allow(clippy::cast_possible_truncation)]
        let extra_half = if hovered {
            ((4.0 * hover_factor).round() as i32).clamp(0, 4)
        } else {
            0
        };
        let draw_size = DOCK_ICON_SIZE + extra_half * 2;
        let icon_x = item_x + (item_size - draw_size) / 2;
        let icon_y = (dock_y + 7) - extra_half - lift;
        let icon_loc = Point::from((icon_x, icon_y));

        let win_entry = (i >= DOCK_PINNED_COUNT && i < left_count)
            .then(|| dock_view.windows.get(i - DOCK_PINNED_COUNT))
            .flatten();
        let is_minimized = win_entry.is_some_and(|w| w.minimized);
        let is_active = match i {
            0 => true,
            1 => dock_view.overview_active,
            _ => win_entry.is_some_and(|w| w.focused && !w.minimized),
        };
        let has_running_dot =
            win_entry.is_some() || i == 0 || (i == 1 && dock_view.overview_active);
        if has_running_dot {
            let pip_color = if is_minimized {
                [232, 196, 119, 235]
            } else if is_active {
                [248, 246, 255, 240]
            } else {
                [230, 224, 245, 165]
            };
            if let Some(dot) = shape_cache.circle_element(
                renderer,
                5,
                pip_color,
                None,
                Point::from((item_x + (item_size - 5) / 2, dock_y + DOCK_HEIGHT - 8)),
            ) {
                dock_dots_and_divider.push(CompositorRenderElement::Memory(dot));
            }
        }

        let builtin_for_slot = match i {
            0 => Some(crate::icons::BuiltinAppIcon::Files),
            1 => Some(crate::icons::BuiltinAppIcon::Overview),
            2 => Some(crate::icons::BuiltinAppIcon::Terminal),
            3 => Some(crate::icons::BuiltinAppIcon::Browser),
            4 => Some(crate::icons::BuiltinAppIcon::Notes),
            5 => Some(crate::icons::BuiltinAppIcon::Settings),
            n if n == left_count => Some(crate::icons::BuiltinAppIcon::Downloads),
            n if n == left_count + 1 => Some(crate::icons::BuiltinAppIcon::Trash),
            _ => None,
        };
        let slot_icon_el = if let Some(kind) = builtin_for_slot {
            shape_cache
                .icons
                .builtin_squircle_icon_element(renderer, kind, draw_size, icon_loc)
                .map(|el| crate::icons::ResolvedIconElement::Memory(Box::new(el)))
        } else {
            win_entry.and_then(|w| {
                let key = format!("win-{}", w.id.0);
                shape_cache
                    .icons
                    .window_icon_element(
                        renderer,
                        &key,
                        w.toplevel_icon.as_ref(),
                        w.icon_name.as_deref(),
                        draw_size,
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
                                draw_size,
                                icon_loc,
                            )
                            .map(|el| crate::icons::ResolvedIconElement::Memory(Box::new(el)))
                    })
            })
        };
        if let Some(el) = slot_icon_el {
            dock_icons.push(resolved_icon_to_element(el));
        }
    }

    out.extend(dock_icons);
    out.extend(dock_dots_and_divider);

    if let Some(dock_glass) = shape_cache.dock_glass_panel_element(
        renderer,
        dock_width,
        DOCK_HEIGHT,
        DOCK_RADIUS,
        top_telemetry.glass_opacity,
        top_telemetry.reduce_transparency,
        Point::from((dock_x, dock_y)),
    ) {
        out.push(CompositorRenderElement::Memory(dock_glass));
    }

    // 5. Workspace Overview Spaces strip, floating cards & light scrim (`RUSTYOS-023`/`025`/`051`)
    if let ShellOverlayView::Overview(ov) = overlay {
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
                        [224, 212, 248, 185],
                        Some([255, 255, 255, 90]),
                    );
                }
                let thumb_border = if space.active {
                    [185, 138, 255, 255]
                } else {
                    RGBA_BORDER
                };
                let thumb_bg = if space.active {
                    [72, 42, 124, 228]
                } else if space.window_count > 0 {
                    [50, 32, 86, 214]
                } else {
                    [36, 24, 62, 196]
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
                push_soft_shadow(&mut out, shape_cache, renderer, thumb, 8, 8, 32);
            }
            out.push(CompositorRenderElement::Solid(make_solid_element(
                Rectangle::new(Point::from((0, TOP_BAR_HEIGHT)), Size::from((width, 98))),
                premul(0.08, 0.05, 0.14, 0.28),
            )));
        }

        let cards = overview_card_rects(screen_size, ov.windows.len());
        for (win, base_card) in ov.windows.iter().zip(cards) {
            let card = Rectangle::new(
                Point::from((base_card.loc.x, base_card.loc.y + card_dy)),
                base_card.size,
            );
            push_liquid_glass_panel(
                &mut out,
                shape_cache,
                renderer,
                card,
                14,
                crate::surfaces::GlassPanelStyle::Popover,
                top_telemetry.glass_opacity,
                top_telemetry.reduce_transparency,
                None,
            );
            if win.focused {
                push_rounded_glass_box(
                    &mut out,
                    shape_cache,
                    renderer,
                    Rectangle::new(
                        Point::from((card.loc.x - 2, card.loc.y - 2)),
                        Size::from((card.size.w + 4, card.size.h + 4)),
                    ),
                    15,
                    [165, 110, 255, 68],
                    Some([198, 156, 255, 245]),
                );
            }
            push_soft_shadow(&mut out, shape_cache, renderer, card, 14, 16, 50);
        }
        #[allow(clippy::cast_possible_truncation)]
        let scrim_a = 0.36 * (ov.progress.clamp(0.0, 1.0) as f32);
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

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn render_lock_power_menu(
    out: &mut Vec<CompositorRenderElement>,
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    layout: &LockScreenLayout,
    lock: &LockScreenRenderState<'_>,
    fade: f32,
) {
    if !lock.power_menu_open || fade <= 0.25 {
        return;
    }
    let text_alpha = (244.0 * fade).clamp(0.0, 255.0).round() as u8;
    let text_rgba = [246, 248, 252, text_alpha];
    let labels = ["Sleep", "Restart...", "Shut Down..."];
    for (idx, (rect, label)) in layout.power_item_rects.iter().zip(labels).enumerate() {
        if let Some(el) = text_cache.render_label_weighted(
            renderer,
            label,
            crate::text::FontWeight::Regular,
            12.5,
            text_rgba,
            Point::from((rect.loc.x + 12, rect.loc.y + 7)),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
        if idx < 2 {
            out.push(CompositorRenderElement::Solid(make_solid_element(
                Rectangle::new(
                    Point::from((rect.loc.x + 8, rect.loc.y + rect.size.h - 1)),
                    Size::from((rect.size.w - 16, 1)),
                ),
                premul(1.0, 1.0, 1.0, 0.14 * fade),
            )));
        }
    }
    push_liquid_glass_panel(
        out,
        shape_cache,
        renderer,
        layout.power_menu_rect,
        12,
        crate::surfaces::GlassPanelStyle::Popover,
        lock.glass_opacity,
        lock.reduce_transparency,
        Some((14, (44.0 * fade).round() as u8)),
    );
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn render_lock_top_right_status(
    out: &mut Vec<CompositorRenderElement>,
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    layout: &LockScreenLayout,
    lock: &LockScreenRenderState<'_>,
    fade: f32,
) {
    if fade <= 0.15 {
        return;
    }
    let white_a = (242.0 * fade).clamp(0.0, 255.0).round() as u8;
    let shadow_a = (72.0 * fade).clamp(0.0, 255.0).round() as u8;
    let white = [255, 255, 255, white_a];
    let kbd_label = format_lock_kbd_label(lock.keyboard_layout);

    if text_cache.has_font() {
        if let Some(el) = text_cache.render_label_weighted(
            renderer,
            kbd_label,
            crate::text::FontWeight::Bold,
            12.0,
            white,
            Point::from((layout.kbd_text_x, 10)),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
        if let Some(el) = text_cache.render_label_weighted(
            renderer,
            kbd_label,
            crate::text::FontWeight::Bold,
            12.0,
            [0, 0, 0, shadow_a],
            Point::from((layout.kbd_text_x, 11)),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
    }

    if let Some(el) = shape_cache.ui_glyph_rect_element(
        renderer,
        crate::surfaces::UiGlyphKind::Keyboard,
        18,
        14,
        white,
        Point::from((layout.kbd_glyph_x, 11)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }

    if let Some(bx) = layout.battery_x
        && let Some(el) = shape_cache.ui_glyph_rect_element(
            renderer,
            crate::surfaces::UiGlyphKind::Battery {
                percent: lock.battery_percent,
                charging: lock.battery_charging,
            },
            24,
            18,
            white,
            Point::from((bx, 9)),
        )
    {
        out.push(CompositorRenderElement::Memory(el));
    }

    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        crate::surfaces::UiGlyphKind::Wifi {
            connected: lock.wifi_connected,
            signal_percent: lock.wifi_signal_percent,
        },
        18,
        white,
        Point::from((layout.wifi_x, 9)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }

    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        crate::surfaces::UiGlyphKind::PowerSymbol,
        16,
        white,
        Point::from((
            layout.power_btn_rect.loc.x + 3,
            layout.power_btn_rect.loc.y + 3,
        )),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    if lock.power_menu_open
        && let Some(el) = shape_cache.circle_element(
            renderer,
            layout.power_btn_rect.size.w,
            [255, 255, 255, (56.0 * fade).round() as u8],
            Some([255, 255, 255, (92.0 * fade).round() as u8]),
            Point::from((layout.power_btn_rect.loc.x, layout.power_btn_rect.loc.y)),
        )
    {
        out.push(CompositorRenderElement::Memory(el));
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn render_lock_clock_header(
    out: &mut Vec<CompositorRenderElement>,
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    screen_width: i32,
    layout: &LockScreenLayout,
    lock: &LockScreenRenderState<'_>,
    fade: f32,
) {
    if fade <= 0.15 || !text_cache.has_font() {
        return;
    }
    let date_bounds = Rectangle::new(
        Point::from((0, layout.date_y)),
        Size::from((screen_width, (layout.date_size * 1.4).round() as i32)),
    );
    if let Some(el) = text_cache.render_label_centered_weighted(
        renderer,
        lock.date_label,
        crate::text::FontWeight::Bold,
        layout.date_size,
        [248, 249, 252, (238.0 * fade).round() as u8],
        date_bounds,
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    let date_shadow_bounds = Rectangle::new(Point::from((0, layout.date_y + 1)), date_bounds.size);
    if let Some(el) = text_cache.render_label_centered_weighted(
        renderer,
        lock.date_label,
        crate::text::FontWeight::Bold,
        layout.date_size,
        [0, 0, 0, (48.0 * fade).round() as u8],
        date_shadow_bounds,
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }

    let clock_bounds = Rectangle::new(
        Point::from((0, layout.clock_y)),
        Size::from((screen_width, (layout.clock_size * 1.15).round() as i32)),
    );
    // macOS Liquid Glass lock clock: clean translucent numerals letting wallpaper glow through.
    if let Some(el) = text_cache.render_label_centered_weighted(
        renderer,
        lock.clock_hhmm,
        crate::text::FontWeight::Bold,
        layout.clock_size,
        [255, 255, 255, (186.0 * fade).round() as u8],
        clock_bounds,
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn render_lock_users_section(
    out: &mut Vec<CompositorRenderElement>,
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    layout: &LockScreenLayout,
    lock: &LockScreenRenderState<'_>,
    fade: f32,
) {
    if fade <= 0.18 {
        return;
    }
    let fallback_user = default_lock_user_fallback();
    let users = if lock.users.is_empty() {
        std::slice::from_ref(&fallback_user)
    } else {
        lock.users
    };
    let selected = lock.selected_user.min(users.len().saturating_sub(1));
    let multi_user = users.len() > 1;
    let name_alpha = (246.0 * fade).round() as u8;
    let shadow_alpha = (64.0 * fade).round() as u8;

    for (idx, (user, row_rect)) in users.iter().zip(&layout.user_row_rects).enumerate() {
        let row_y = row_rect.loc.y + 4;
        let is_selected = idx == selected;
        if text_cache.has_font() {
            let text_rgba = if is_selected || !multi_user {
                [255, 255, 255, name_alpha]
            } else {
                [232, 236, 242, (206.0 * fade).round() as u8]
            };
            if let Some(el) = text_cache.render_label_weighted(
                renderer,
                &user.display_name,
                crate::text::FontWeight::Bold,
                13.5,
                text_rgba,
                Point::from((layout.name_x, row_y + 10)),
            ) {
                out.push(CompositorRenderElement::Memory(el));
            }
            if let Some(el) = text_cache.render_label_weighted(
                renderer,
                &user.display_name,
                crate::text::FontWeight::Bold,
                13.5,
                [0, 0, 0, shadow_alpha],
                Point::from((layout.name_x, row_y + 11)),
            ) {
                out.push(CompositorRenderElement::Memory(el));
            }
        }
        if let Some(el) = shape_cache.user_avatar_element(
            renderer,
            38,
            user.avatar_path.as_deref().map(std::path::Path::new),
            user.uid,
            is_selected,
            Point::from((layout.avatar_x, row_y)),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
    }

    if let Some(hint_rect) = layout.unlock_hint_rect
        && text_cache.has_font()
    {
        let hint_logical = Rectangle::new(
            Point::from((hint_rect.loc.x, hint_rect.loc.y)),
            Size::from((hint_rect.size.w, hint_rect.size.h)),
        );
        if let Some(el) = text_cache.render_label_centered_weighted(
            renderer,
            "Click or press Return to unlock",
            crate::text::FontWeight::Regular,
            11.0,
            [244, 247, 252, (170.0 * fade).round() as u8],
            hint_logical,
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
        let shadow_logical = Rectangle::new(
            Point::from((hint_rect.loc.x, hint_rect.loc.y + 1)),
            hint_logical.size,
        );
        if let Some(el) = text_cache.render_label_centered_weighted(
            renderer,
            "Click or press Return to unlock",
            crate::text::FontWeight::Regular,
            11.0,
            [0, 0, 0, shadow_alpha],
            shadow_logical,
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
    }

    if lock.require_password {
        render_lock_password_pill(out, text_cache, shape_cache, renderer, layout, lock, fade);
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn render_lock_password_pill(
    out: &mut Vec<CompositorRenderElement>,
    text_cache: &mut crate::text::TextCache,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    layout: &LockScreenLayout,
    lock: &LockScreenRenderState<'_>,
    fade: f32,
) {
    let Some(pill_rect) = layout.password_pill_rect else {
        return;
    };
    let dot_count = lock.password_len.min(36);
    let dots = "•".repeat(dot_count);
    if text_cache.has_font() {
        let (pill_label, pill_color) = if lock.auth_checking {
            (
                "Verifying...",
                [232, 236, 252, (215.0 * fade).round() as u8],
            )
        } else if dot_count == 0 {
            (
                "Enter Password",
                [240, 244, 252, (165.0 * fade).round() as u8],
            )
        } else {
            (dots.as_str(), [255, 255, 255, (245.0 * fade).round() as u8])
        };
        let text_bounds = Rectangle::new(
            Point::from((pill_rect.loc.x + 12, pill_rect.loc.y)),
            Size::from(((pill_rect.size.w - 42).max(24), pill_rect.size.h)),
        );
        if let Some(el) = text_cache.render_label_centered_weighted(
            renderer,
            pill_label,
            crate::text::FontWeight::Regular,
            12.5,
            pill_color,
            text_bounds,
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
    }

    if let Some(submit_rect) = layout.password_submit_rect {
        if let Some(arrow) = shape_cache.ui_glyph_element(
            renderer,
            crate::surfaces::UiGlyphKind::ArrowNext,
            12,
            [250, 250, 255, (240.0 * fade).round() as u8],
            Point::from((submit_rect.loc.x + 8, submit_rect.loc.y + 8)),
        ) {
            out.push(CompositorRenderElement::Memory(arrow));
        }
        let btn_alpha = if dot_count > 0 { 68.0 } else { 36.0 };
        if let Some(circle) = shape_cache.circle_element(
            renderer,
            22,
            [255, 255, 255, (btn_alpha * fade).round() as u8],
            Some([255, 255, 255, (92.0 * fade).round() as u8]),
            Point::from((submit_rect.loc.x + 3, submit_rect.loc.y + 3)),
        ) {
            out.push(CompositorRenderElement::Memory(circle));
        }
    }

    let border_rgba = if lock.auth_error.is_some() {
        [255, 132, 138, (195.0 * fade).round() as u8]
    } else {
        [255, 255, 255, (112.0 * fade).round() as u8]
    };
    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        pill_rect,
        15,
        [255, 255, 255, (42.0 * fade).round() as u8],
        Some(border_rgba),
    );

    render_lock_auth_status_label(out, text_cache, renderer, layout, lock, fade);
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn render_lock_auth_status_label(
    out: &mut Vec<CompositorRenderElement>,
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    layout: &LockScreenLayout,
    lock: &LockScreenRenderState<'_>,
    fade: f32,
) {
    let shadow_alpha = (64.0 * fade).round() as u8;
    if let Some(status_rect) = layout.auth_status_rect
        && text_cache.has_font()
    {
        let status_owned;
        let (status_label, status_color) = if lock.auth_checking {
            (
                "Checking credentials...",
                [236, 240, 252, (205.0 * fade).round() as u8],
            )
        } else if let Some(err) = lock.auth_error {
            status_owned = truncate_ascii(err, 48);
            (
                status_owned.as_str(),
                [255, 152, 152, (245.0 * fade).round() as u8],
            )
        } else {
            (
                "Enter password and press Return",
                [244, 247, 252, (176.0 * fade).round() as u8],
            )
        };
        let status_bounds = Rectangle::new(
            Point::from((status_rect.loc.x, status_rect.loc.y)),
            Size::from((status_rect.size.w, status_rect.size.h)),
        );
        if let Some(el) = text_cache.render_label_centered_weighted(
            renderer,
            status_label,
            crate::text::FontWeight::Regular,
            11.0,
            status_color,
            status_bounds,
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
        let shadow_bounds = Rectangle::new(
            Point::from((status_rect.loc.x, status_rect.loc.y + 1)),
            Size::from((status_rect.size.w, status_rect.size.h)),
        );
        if let Some(el) = text_cache.render_label_centered_weighted(
            renderer,
            status_label,
            crate::text::FontWeight::Regular,
            11.0,
            [0, 0, 0, shadow_alpha],
            shadow_bounds,
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
    }
}

/// Generates topmost front-to-back macOS-style Lock Screen backdrop, frosted clock,
/// minimal top-right controls, and bottom user selector (`RUSTYOS-025`/`054`).
#[allow(clippy::cast_possible_truncation)]
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

    let fallback_user = default_lock_user_fallback();
    let users = if lock.users.is_empty() {
        std::slice::from_ref(&fallback_user)
    } else {
        lock.users
    };
    let names: Vec<&str> = users.iter().map(|u| u.display_name.as_str()).collect();
    let slide_dy = anim_offset(p, 16.0);
    let layout = compute_lock_screen_layout(
        screen_size,
        lock.keyboard_layout,
        lock.has_battery,
        &names,
        slide_dy,
        lock.require_password,
    );
    let fade = p as f32;

    render_lock_power_menu(
        &mut out,
        text_cache,
        shape_cache,
        renderer,
        &layout,
        lock,
        fade,
    );
    render_lock_top_right_status(
        &mut out,
        text_cache,
        shape_cache,
        renderer,
        &layout,
        lock,
        fade,
    );
    render_lock_clock_header(
        &mut out,
        text_cache,
        renderer,
        screen_size.w,
        &layout,
        lock,
        fade,
    );
    render_lock_users_section(
        &mut out,
        text_cache,
        shape_cache,
        renderer,
        &layout,
        lock,
        fade,
    );

    if let Some(bg) = shape_cache.wallpaper_element_with_alpha(
        renderer,
        Size::from((screen_size.w, screen_size.h)),
        lock.wallpaper_path,
        fade,
    ) {
        out.push(CompositorRenderElement::Memory(bg));
    } else {
        out.push(CompositorRenderElement::Solid(make_solid_element(
            Rectangle::new(Point::from((0, 0)), screen_size),
            premul(0.08, 0.09, 0.13, fade),
        )));
    }

    out
}

fn wrap_polkit_two_lines(message: &str, max_chars: usize) -> (String, Option<String>) {
    let mut line1 = String::new();
    let mut line2 = String::new();
    let mut on_second = false;
    for word in message.split_whitespace() {
        if !on_second {
            let needed = if line1.is_empty() {
                word.chars().count()
            } else {
                line1.chars().count() + 1 + word.chars().count()
            };
            if needed <= max_chars || line1.is_empty() {
                if !line1.is_empty() {
                    line1.push(' ');
                }
                line1.push_str(word);
                continue;
            }
            on_second = true;
        }
        let needed = if line2.is_empty() {
            word.chars().count()
        } else {
            line2.chars().count() + 1 + word.chars().count()
        };
        if needed <= max_chars {
            if !line2.is_empty() {
                line2.push(' ');
            }
            line2.push_str(word);
        } else {
            if !line2.ends_with("...") && line2.len() + 3 <= max_chars + 3 {
                line2.push_str("...");
            }
            break;
        }
    }
    (
        truncate_ascii(&line1, max_chars),
        (!line2.is_empty()).then(|| truncate_ascii(&line2, max_chars)),
    )
}

fn render_polkit_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    pk: &PolkitPromptRenderState<'_>,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let layout = polkit_modal_layout(screen_size);
    render_polkit_header_text(text_cache, renderer, layout.modal, pk, elements);
    render_polkit_controls_text(text_cache, renderer, layout, pk, elements);
}

fn render_polkit_header_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    modal: Rectangle<i32, smithay::utils::Physical>,
    pk: &PolkitPromptRenderState<'_>,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let mx = modal.loc.x;
    let my = modal.loc.y;
    if let Some(el) = text_cache.render_label_weighted(
        renderer,
        "Authentication Required",
        crate::text::FontWeight::Bold,
        14.5,
        [248, 245, 255, 255],
        Point::from((mx + 70, my + 20)),
    ) {
        elements.push(el);
    }
    let user_line = truncate_ascii(&format!("Authenticating as {}", pk.identity_user), 38);
    if let Some(el) = text_cache.render_label(
        renderer,
        &user_line,
        11.5,
        [188, 178, 212, 255],
        Point::from((mx + 70, my + 40)),
    ) {
        elements.push(el);
    }

    let (line1, line2) = wrap_polkit_two_lines(pk.message, 46);
    if !line1.is_empty()
        && let Some(el) = text_cache.render_label(
            renderer,
            &line1,
            12.0,
            [236, 230, 248, 255],
            Point::from((mx + 22, my + 72)),
        )
    {
        elements.push(el);
    }
    if let Some(ref l2) = line2
        && let Some(el) = text_cache.render_label(
            renderer,
            l2,
            12.0,
            [236, 230, 248, 255],
            Point::from((mx + 22, my + 92)),
        )
    {
        elements.push(el);
    }
}

fn render_polkit_controls_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    layout: PolkitModalLayout,
    pk: &PolkitPromptRenderState<'_>,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let input = layout.input;
    if pk.input_display.is_empty() {
        let placeholder = truncate_ascii(pk.prompt_label.trim_end_matches(':').trim(), 36);
        let label = if placeholder.is_empty() {
            "Password"
        } else {
            placeholder.as_str()
        };
        if let Some(el) = text_cache.render_label(
            renderer,
            label,
            13.0,
            [172, 162, 194, 230],
            Point::from((input.loc.x + 12, input.loc.y + 10)),
        ) {
            elements.push(el);
        }
    } else if let Some(el) = text_cache.render_label(
        renderer,
        pk.input_display,
        13.5,
        [250, 248, 255, 255],
        Point::from((input.loc.x + 12, input.loc.y + 10)),
    ) {
        elements.push(el);
    }

    let (status_text, status_color) = if pk.checking {
        ("Verifying credentials...".to_owned(), [210, 185, 255, 255])
    } else if let Some(err) = pk.error_message {
        (truncate_ascii(err, 48), [255, 138, 142, 255])
    } else if let Some(info) = pk.info_message {
        (truncate_ascii(info, 48), [212, 206, 232, 240])
    } else {
        (truncate_ascii(pk.action_id, 48), [166, 155, 188, 210])
    };
    if !status_text.is_empty()
        && let Some(el) = text_cache.render_label(
            renderer,
            &status_text,
            11.0,
            status_color,
            Point::from((layout.status.loc.x, layout.status.loc.y + 4)),
        )
    {
        elements.push(el);
    }

    let cancel_bounds = Rectangle::new(
        Point::from((layout.cancel.loc.x, layout.cancel.loc.y)),
        Size::from((layout.cancel.size.w, layout.cancel.size.h)),
    );
    if let Some(el) = text_cache.render_label_centered_weighted(
        renderer,
        "Cancel",
        crate::text::FontWeight::Bold,
        12.0,
        [240, 236, 248, 255],
        cancel_bounds,
    ) {
        elements.push(el);
    }
    let submit_label = if pk.checking {
        "Verifying..."
    } else {
        "Authenticate"
    };
    let submit_bounds = Rectangle::new(
        Point::from((layout.submit.loc.x, layout.submit.loc.y)),
        Size::from((layout.submit.size.w, layout.submit.size.h)),
    );
    if let Some(el) = text_cache.render_label_centered_weighted(
        renderer,
        submit_label,
        crate::text::FontWeight::Bold,
        12.0,
        [252, 250, 255, 255],
        submit_bounds,
    ) {
        elements.push(el);
    }
}

fn render_polkit_composite(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    top_telemetry: &TopBarTelemetry<'_>,
    pk: &PolkitPromptRenderState<'_>,
) {
    let layout = polkit_modal_layout(screen_size);
    let badge = layout.badge;
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        crate::surfaces::UiGlyphKind::Lock,
        18,
        [248, 244, 255, 255],
        Point::from((badge.loc.x + 10, badge.loc.y + 10)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }

    let input = layout.input;
    let typed_w = crate::text::measure_ui_text_width(
        pk.input_display,
        13.5,
        crate::text::FontWeight::Regular,
    );
    let caret_x = (input.loc.x + 13 + typed_w).min(input.loc.x + input.size.w - 10);
    out.push(CompositorRenderElement::Solid(make_solid_element(
        Rectangle::new(Point::from((caret_x, input.loc.y + 8)), Size::from((2, 22))),
        COLOR_ACCENT,
    )));

    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        badge,
        11,
        [165, 110, 255, 205],
        Some([215, 185, 255, 155]),
    );

    let input_border = if pk.error_message.is_some() {
        [255, 130, 136, 220]
    } else {
        [192, 148, 255, 225]
    };
    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        input,
        11,
        scale_glass_rgba(
            [24, 18, 38, 198],
            top_telemetry.glass_opacity,
            top_telemetry.reduce_transparency,
        ),
        Some(input_border),
    );

    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        layout.cancel,
        10,
        RGBA_SURFACE,
        Some(RGBA_BORDER),
    );
    let submit_fill = if pk.checking {
        [124, 92, 184, 170]
    } else {
        [165, 110, 255, 224]
    };
    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        layout.submit,
        10,
        submit_fill,
        Some([215, 185, 255, 185]),
    );

    push_liquid_glass_panel(
        out,
        shape_cache,
        renderer,
        layout.modal,
        WINDOW_RADIUS,
        crate::surfaces::GlassPanelStyle::Popover,
        top_telemetry.glass_opacity,
        top_telemetry.reduce_transparency,
        Some((22, 62)),
    );

    out.push(CompositorRenderElement::Solid(make_solid_element(
        Rectangle::new(Point::from((0, 0)), screen_size),
        premul(0.059, 0.039, 0.098, 0.44),
    )));
}

fn phys_to_logical_rect(
    rect: Rectangle<i32, smithay::utils::Physical>,
) -> Rectangle<i32, smithay::utils::Logical> {
    Rectangle::new(
        Point::from((rect.loc.x, rect.loc.y)),
        Size::from((rect.size.w, rect.size.h)),
    )
}

fn render_portal_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    portal: &PortalPromptRenderState<'_>,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let layout = portal_modal_layout(
        screen_size,
        portal.rows.len(),
        portal.selected,
        portal.cursor_mode_editable,
    );
    let mx = layout.modal.loc.x;
    let my = layout.modal.loc.y;
    let title = if portal.is_screencast {
        "Share Screen or Window"
    } else {
        "Screenshot Request"
    };
    if let Some(el) = text_cache.render_label_weighted(
        renderer,
        title,
        crate::text::FontWeight::Bold,
        14.5,
        [248, 245, 255, 255],
        Point::from((mx + 70, my + 20)),
    ) {
        elements.push(el);
    }
    let subtitle = truncate_ascii(
        &format!(
            "Requested by {}",
            if portal.app_id.is_empty() {
                "Application"
            } else {
                portal.app_id
            }
        ),
        40,
    );
    if let Some(el) = text_cache.render_label(
        renderer,
        &subtitle,
        11.5,
        [188, 178, 212, 255],
        Point::from((mx + 70, my + 40)),
    ) {
        elements.push(el);
    }

    for (vis_idx, row_rect) in layout
        .row_rects
        .iter()
        .take(layout.visible_count)
        .enumerate()
    {
        let row_idx = layout.row_start + vis_idx;
        let Some(row) = portal.rows.get(row_idx) else {
            continue;
        };
        let label = truncate_ascii(row.label, 38);
        if let Some(el) = text_cache.render_label_weighted(
            renderer,
            &label,
            crate::text::FontWeight::Bold,
            12.0,
            [248, 244, 255, 255],
            Point::from((row_rect.loc.x + 36, row_rect.loc.y + 5)),
        ) {
            elements.push(el);
        }
        let detail = truncate_ascii(row.detail, 44);
        if let Some(el) = text_cache.render_label(
            renderer,
            &detail,
            10.0,
            [176, 166, 200, 255],
            Point::from((row_rect.loc.x + 36, row_rect.loc.y + 22)),
        ) {
            elements.push(el);
        }
    }

    render_portal_footer_text(text_cache, renderer, screen_size, portal, elements);
}

fn render_portal_footer_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    portal: &PortalPromptRenderState<'_>,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let layout = portal_modal_layout(
        screen_size,
        portal.rows.len(),
        portal.selected,
        portal.cursor_mode_editable,
    );

    if let Some(toggle_rect) = layout.cursor_toggle {
        let mark = if portal.include_cursor {
            "[x] Include cursor in stream"
        } else {
            "[ ] Include cursor in stream"
        };
        if let Some(el) = text_cache.render_label(
            renderer,
            mark,
            11.5,
            [232, 224, 248, 255],
            Point::from((toggle_rect.loc.x + 6, toggle_rect.loc.y + 5)),
        ) {
            elements.push(el);
        }
    }

    if let Some(el) = text_cache.render_label_centered_weighted(
        renderer,
        "Cancel",
        crate::text::FontWeight::Regular,
        12.5,
        [240, 236, 248, 255],
        phys_to_logical_rect(layout.cancel),
    ) {
        elements.push(el);
    }
    let accept_label = if portal.is_screencast {
        "Share"
    } else {
        "Capture"
    };
    if let Some(el) = text_cache.render_label_centered_weighted(
        renderer,
        accept_label,
        crate::text::FontWeight::Bold,
        12.5,
        [252, 249, 255, 255],
        phys_to_logical_rect(layout.accept),
    ) {
        elements.push(el);
    }
}

fn render_portal_composite(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    top_telemetry: &TopBarTelemetry<'_>,
    portal: &PortalPromptRenderState<'_>,
) {
    let layout = portal_modal_layout(
        screen_size,
        portal.rows.len(),
        portal.selected,
        portal.cursor_mode_editable,
    );
    let badge = layout.badge;
    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        crate::surfaces::UiGlyphKind::Grid,
        18,
        [248, 244, 255, 255],
        Point::from((badge.loc.x + 10, badge.loc.y + 10)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }
    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        badge,
        11,
        [165, 110, 255, 205],
        Some([215, 185, 255, 155]),
    );

    for (vis_idx, row_rect) in layout
        .row_rects
        .iter()
        .take(layout.visible_count)
        .enumerate()
    {
        let row_idx = layout.row_start + vis_idx;
        let is_selected = row_idx == portal.selected;
        let glyph = if portal.rows.get(row_idx).is_some_and(|r| r.is_window) {
            crate::surfaces::UiGlyphKind::WindowMax
        } else {
            crate::surfaces::UiGlyphKind::Grid
        };
        if let Some(el) = shape_cache.ui_glyph_element(
            renderer,
            glyph,
            14,
            [244, 238, 255, 255],
            Point::from((row_rect.loc.x + 12, row_rect.loc.y + 13)),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }
        let (fill, border) = if is_selected {
            ([165, 110, 255, 138], [215, 185, 255, 225])
        } else {
            (RGBA_SURFACE, RGBA_BORDER)
        };
        push_rounded_glass_box(
            out,
            shape_cache,
            renderer,
            *row_rect,
            10,
            fill,
            Some(border),
        );
    }

    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        layout.cancel,
        10,
        RGBA_SURFACE,
        Some(RGBA_BORDER),
    );
    push_rounded_glass_box(
        out,
        shape_cache,
        renderer,
        layout.accept,
        10,
        [165, 110, 255, 224],
        Some([215, 185, 255, 185]),
    );

    push_liquid_glass_panel(
        out,
        shape_cache,
        renderer,
        layout.modal,
        WINDOW_RADIUS,
        crate::surfaces::GlassPanelStyle::Popover,
        top_telemetry.glass_opacity,
        top_telemetry.reduce_transparency,
        Some((22, 62)),
    );

    out.push(CompositorRenderElement::Solid(make_solid_element(
        Rectangle::new(Point::from((0, 0)), screen_size),
        premul(0.059, 0.039, 0.098, 0.44),
    )));
}

fn render_pick_color_text(
    text_cache: &mut crate::text::TextCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    app_id: &str,
    elements: &mut Vec<MemoryRenderBufferRenderElement<GlesRenderer>>,
) {
    let pill = pick_color_pill_rect(screen_size);
    let msg = truncate_ascii(
        &format!(
            "Pick Color ({}) · Click pixel · Esc cancel",
            if app_id.is_empty() { "App" } else { app_id }
        ),
        48,
    );
    if let Some(el) = text_cache.render_label_centered_weighted(
        renderer,
        &msg,
        crate::text::FontWeight::Bold,
        12.0,
        [248, 244, 255, 255],
        phys_to_logical_rect(pill),
    ) {
        elements.push(el);
    }
}

fn render_pick_color_composite(
    out: &mut Vec<CompositorRenderElement>,
    shape_cache: &mut crate::surfaces::UiShapeCache,
    renderer: &mut GlesRenderer,
    screen_size: Size<i32, smithay::utils::Physical>,
    top_telemetry: &TopBarTelemetry<'_>,
) {
    let pill = pick_color_pill_rect(screen_size);
    push_liquid_glass_panel(
        out,
        shape_cache,
        renderer,
        pill,
        12,
        crate::surfaces::GlassPanelStyle::Popover,
        top_telemetry.glass_opacity,
        top_telemetry.reduce_transparency,
        Some((12, 42)),
    );
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

/// Generates Liquid Glass OSD indicator elements (`Volume`, `Brightness`, `Media`, `Screenshot`) (`RUSTYOS-031`/`051`).
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

    let text_bright = [248, 244, 255, 255];
    let text_muted = [182, 172, 204, 255];

    if let Some(el) = shape_cache.ui_glyph_element(
        renderer,
        osd_glyph_kind(osd.label, osd.value_percent, osd.muted),
        22,
        [250, 246, 255, 255],
        Point::from((px + 14, py + (pill.size.h - 22) / 2)),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }

    if let Some(pct) = osd.value_percent {
        if let Some(el) = text_cache.render_label(
            renderer,
            osd.label,
            12.0,
            text_bright,
            Point::from((px + 44, py + 11)),
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
            Point::from((px + pw - 14 - vw, py + 11)),
        ) {
            out.push(CompositorRenderElement::Memory(el));
        }

        let bar_x = px + 44;
        let bar_y = py + 34;
        let bar_w = (pw - 58).max(24);
        let bar_h = 8;
        let fill_w = (bar_w * i32::from(pct.min(100))) / 100;
        if fill_w > 0 {
            let fill_rgba = if osd.muted {
                [152, 140, 174, 180]
            } else {
                [246, 240, 255, 236]
            };
            push_rounded_glass_box(
                &mut out,
                shape_cache,
                renderer,
                Rectangle::new(Point::from((bar_x, bar_y)), Size::from((fill_w, bar_h))),
                fill_w.min(4),
                fill_rgba,
                None,
            );
        }
        push_rounded_glass_box(
            &mut out,
            shape_cache,
            renderer,
            Rectangle::new(Point::from((bar_x, bar_y)), Size::from((bar_w, bar_h))),
            4,
            [255, 255, 255, 30],
            Some([255, 255, 255, 48]),
        );
    } else if let Some(el) = text_cache.render_label_centered(
        renderer,
        osd.label,
        12.5,
        text_bright,
        Rectangle::new(
            Point::from((px + 38, py)),
            Size::from((pw - 48, pill.size.h)),
        ),
    ) {
        out.push(CompositorRenderElement::Memory(el));
    }

    push_liquid_glass_panel(
        &mut out,
        shape_cache,
        renderer,
        pill,
        18,
        crate::surfaces::GlassPanelStyle::Popover,
        crate::config::DEFAULT_GLASS_OPACITY,
        false,
        Some((14, 46)),
    );
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
            wifi_enabled: true,
            wifi_signal_percent: Some(80),
            bt_available: true,
            bt_powered: true,
            bt_label: "BT: On",
            battery_percent: 88,
            has_battery: true,
            ac_online: true,
            battery_charging: true,
            brightness_available: false,
            brightness_percent: 100,
            volume_percent: 75,
            audio_muted: false,
            media_active: false,
            media_playing: false,
            media_title: "",
            focus_mode: false,
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
    fn top_bar_hit_testing_works_accurately() {
        assert!(check_top_bar_status_click(1280, 1200.0, 12.0));
        assert!(!check_top_bar_status_click(1280, 100.0, 12.0));

        let left_ws = compute_top_bar_left_layout_with_options("RustyOS", true);
        let layout_mid = f64::from(left_ws.layout_x + left_ws.layout_w / 2);
        assert!(check_top_bar_layout_click(layout_mid, 12.0));
        assert!(!check_top_bar_layout_click_with_options(
            "RustyOS", false, layout_mid, 12.0
        ));
        assert!(!check_top_bar_layout_click(50.0, 12.0));
        assert!(!check_top_bar_layout_click(300.0, 12.0));

        // Test TopBar Left Menu hits (`RUSTYOS-038`, `RUSTYOS-049`: macOS File/Edit/View/Go/Window/Help)
        let left = compute_top_bar_left_layout("RustyOS");
        assert_eq!(
            check_top_bar_menu_click(f64::from(TOP_BAR_LOGO_X + TOP_BAR_LOGO_W / 2), 12.0),
            Some(TopBarMenuHit::SystemLogo)
        );
        for (hit, bx, bw) in [
            (TopBarMenuHit::ActiveApp, left.app_x, left.app_w),
            (TopBarMenuHit::File, left.file_x, left.file_w),
            (TopBarMenuHit::Edit, left.edit_x, left.edit_w),
            (TopBarMenuHit::View, left.view_x, left.view_w),
            (TopBarMenuHit::Go, left.go_x, left.go_w),
            (TopBarMenuHit::Window, left.win_x, left.win_w),
            (TopBarMenuHit::Help, left.help_x, left.help_w),
        ] {
            assert_eq!(
                check_top_bar_menu_click(f64::from(bx + bw / 2), 12.0),
                Some(hit)
            );
        }
        assert_eq!(
            check_top_bar_menu_click(f64::from(left.help_x + left.help_w + 15), 12.0),
            None
        );
        assert_eq!(
            check_top_bar_menu_click(15.0, f64::from(TOP_BAR_HEIGHT + 5)),
            None
        );
    }

    #[test]
    fn quick_settings_hit_testing_works_accurately() {
        let size = Size::from((1280, 800));
        let layout = quick_settings_layout(size.w, size.h, 0);
        let cases = [
            (100, 200, QuickSettingsHit::OutsidePopover),
            (
                layout.px + 2,
                layout.py + 2,
                QuickSettingsHit::InsidePopover,
            ),
            (
                layout.wifi_pill.loc.x + 60,
                layout.wifi_pill.loc.y + 27,
                QuickSettingsHit::ToggleWifi,
            ),
            (
                layout.bt_pill.loc.x + 60,
                layout.bt_pill.loc.y + 27,
                QuickSettingsHit::ToggleBluetooth,
            ),
            (
                layout.media_prev_btn.loc.x + 12,
                layout.media_prev_btn.loc.y + 12,
                QuickSettingsHit::MediaPrev,
            ),
            (
                layout.media_play_btn.loc.x + 12,
                layout.media_play_btn.loc.y + 12,
                QuickSettingsHit::MediaPlayPause,
            ),
            (
                layout.media_next_btn.loc.x + 12,
                layout.media_next_btn.loc.y + 12,
                QuickSettingsHit::MediaNext,
            ),
            (
                layout.power_pill.loc.x + 60,
                layout.power_pill.loc.y + 30,
                QuickSettingsHit::RefreshMetrics,
            ),
            (
                layout.layout_btn.loc.x + 33,
                layout.layout_btn.loc.y + 33,
                QuickSettingsHit::ToggleLayout,
            ),
            (
                layout.overview_btn.loc.x + 33,
                layout.overview_btn.loc.y + 33,
                QuickSettingsHit::ToggleOverview,
            ),
            (
                layout.motion_btn.loc.x + 33,
                layout.motion_btn.loc.y + 33,
                QuickSettingsHit::ToggleReducedMotion,
            ),
            (
                layout.transparency_btn.loc.x + 33,
                layout.transparency_btn.loc.y + 33,
                QuickSettingsHit::ToggleReduceTransparency,
            ),
            (
                layout.focus_pill.loc.x + 60,
                layout.focus_pill.loc.y + 30,
                QuickSettingsHit::ToggleFocus,
            ),
            (
                layout.sound_mute_zone.loc.x + 15,
                layout.sound_mute_zone.loc.y + 15,
                QuickSettingsHit::ToggleMute,
            ),
            (
                layout.lock_btn.loc.x + 50,
                layout.lock_btn.loc.y + 15,
                QuickSettingsHit::LockSession,
            ),
        ];
        for (px, py, expected) in cases {
            assert_eq!(
                check_quick_settings_click(size, f64::from(px), f64::from(py)),
                expected
            );
        }
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
        let left = compute_top_bar_left_layout_with_options("RustyOS", true);
        for ws in 1u8..=9u8 {
            let x = f64::from(left.ws_start_x) + f64::from(i32::from(ws - 1) * WS_BTN_STRIDE) + 5.0;
            let detected = check_top_bar_workspace_click(x, 12.0);
            assert_eq!(detected, Some(ws));
            assert_eq!(
                check_top_bar_workspace_click_with_options("RustyOS", false, x, 12.0),
                None
            );
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
            (0, DockHit::PinnedFiles),
            (1, DockHit::ToggleOverview),
            (2, DockHit::PinnedTerminal),
            (3, DockHit::PinnedBrowser),
            (4, DockHit::PinnedNotes),
            (5, DockHit::PinnedSettings),
            (6, DockHit::WindowSlot(0)),
            (7, DockHit::WindowSlot(1)),
            (8, DockHit::PinnedDownloads),
            (9, DockHit::PinnedTrash),
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
        let single_users = [rusty_services::UserAccount {
            username: "anc".to_owned(),
            display_name: "anc".to_owned(),
            uid: 1000,
            avatar_path: None,
        }];
        let single_layout = compute_lock_screen_layout(size, "EN", true, &["anc"], 0, false);
        let single_state = LockScreenRenderState {
            date_label: "Thu Sep 10",
            clock_hhmm: "8:52",
            keyboard_layout: "EN",
            has_battery: true,
            battery_percent: 100,
            battery_charging: true,
            wifi_connected: true,
            wifi_signal_percent: Some(90),
            wallpaper_path: None,
            users: &single_users,
            selected_user: 0,
            power_menu_open: false,
            require_password: false,
            password_len: 0,
            auth_checking: false,
            auth_error: None,
            glass_opacity: 0.78,
            reduce_transparency: false,
            progress: 1.0,
        };
        let row0 = single_layout.user_row_rects[0];
        assert_eq!(
            hit_test_lock_screen(
                size,
                &single_state,
                f64::from(row0.loc.x + row0.size.w / 2),
                f64::from(row0.loc.y + row0.size.h / 2)
            ),
            LockScreenHit::UnlockUser(0)
        );
        assert!(check_lock_screen_unlock_click(
            size,
            f64::from(row0.loc.x + row0.size.w / 2),
            f64::from(row0.loc.y + row0.size.h / 2)
        ));
        assert!(!check_lock_screen_unlock_click(size, 40.0, 40.0));
        assert_eq!(
            hit_test_lock_screen(
                size,
                &single_state,
                f64::from(single_layout.kbd_hit_rect.loc.x + 6),
                f64::from(single_layout.kbd_hit_rect.loc.y + 8)
            ),
            LockScreenHit::ToggleKeyboardLayout
        );
        assert_eq!(
            hit_test_lock_screen(
                size,
                &single_state,
                f64::from(single_layout.power_btn_rect.loc.x + 8),
                f64::from(single_layout.power_btn_rect.loc.y + 8)
            ),
            LockScreenHit::TogglePowerMenu
        );
        assert_eq!(anim_offset(1.0, -22.0), 0);
        assert_eq!(anim_offset(0.0, -22.0), -22);
    }

    #[test]
    fn lock_screen_power_menu_and_multi_user_hit_testing() {
        let size = Size::from((1280, 800));
        let multi_users = [
            rusty_services::UserAccount {
                username: "anc".to_owned(),
                display_name: "anc".to_owned(),
                uid: 1000,
                avatar_path: None,
            },
            rusty_services::UserAccount {
                username: "stephen".to_owned(),
                display_name: "Stephen Hackett".to_owned(),
                uid: 1001,
                avatar_path: None,
            },
        ];
        let multi_layout =
            compute_lock_screen_layout(size, "RU", true, &["anc", "Stephen Hackett"], 0, false);
        let mut multi_state = LockScreenRenderState {
            date_label: "Thu Sep 10",
            clock_hhmm: "8:52",
            keyboard_layout: "RU",
            has_battery: true,
            battery_percent: 100,
            battery_charging: true,
            wifi_connected: true,
            wifi_signal_percent: Some(90),
            wallpaper_path: None,
            users: &multi_users,
            selected_user: 0,
            power_menu_open: false,
            require_password: false,
            password_len: 0,
            auth_checking: false,
            auth_error: None,
            glass_opacity: 0.78,
            reduce_transparency: false,
            progress: 1.0,
        };
        let row1 = multi_layout.user_row_rects[1];
        assert_eq!(
            hit_test_lock_screen(
                size,
                &multi_state,
                f64::from(row1.loc.x + 12),
                f64::from(row1.loc.y + 12)
            ),
            LockScreenHit::SelectUser(1)
        );
        multi_state.selected_user = 1;
        assert_eq!(
            hit_test_lock_screen(
                size,
                &multi_state,
                f64::from(row1.loc.x + 12),
                f64::from(row1.loc.y + 12)
            ),
            LockScreenHit::UnlockUser(1)
        );

        multi_state.power_menu_open = true;
        let sleep_item = multi_layout.power_item_rects[0];
        let shutdown_item = multi_layout.power_item_rects[2];
        assert_eq!(
            hit_test_lock_screen(
                size,
                &multi_state,
                f64::from(sleep_item.loc.x + 20),
                f64::from(sleep_item.loc.y + 10)
            ),
            LockScreenHit::PowerAction(rusty_services::SessionAction::Suspend)
        );
        assert_eq!(
            hit_test_lock_screen(
                size,
                &multi_state,
                f64::from(shutdown_item.loc.x + 20),
                f64::from(shutdown_item.loc.y + 10)
            ),
            LockScreenHit::PowerAction(rusty_services::SessionAction::PowerOff)
        );
        assert_eq!(
            hit_test_lock_screen(size, &multi_state, 40.0, 40.0),
            LockScreenHit::DismissPowerMenu
        );

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
        let layout = quick_settings_layout(size.w, size.h, 0);
        let sound_x = f64::from(layout.sound_track.loc.x);
        let sound_w = f64::from(layout.sound_track.size.w);
        let sound_y = f64::from(layout.sound_track.loc.y + layout.sound_track.size.h / 2);
        let disp_x = f64::from(layout.display_track.loc.x);
        let disp_w = f64::from(layout.display_track.size.w);
        let disp_y = f64::from(layout.display_track.loc.y + layout.display_track.size.h / 2);

        assert_eq!(
            check_quick_settings_click(size, sound_x + sound_w * 0.5, sound_y),
            QuickSettingsHit::SetVolumePercent(50)
        );
        assert_eq!(
            check_quick_settings_slider_drag(size, sound_x + sound_w * 0.5, sound_y),
            Some(QuickSettingsSliderDrag::Sound)
        );
        assert_eq!(
            quick_settings_slider_percent(size, QuickSettingsSliderDrag::Sound, sound_x),
            0
        );
        assert_eq!(
            check_quick_settings_click(
                size,
                f64::from(layout.transparency_btn.loc.x + 24),
                f64::from(layout.transparency_btn.loc.y + 16)
            ),
            QuickSettingsHit::ToggleReduceTransparency
        );
        assert_eq!(
            check_quick_settings_click(size, disp_x + 1.0, disp_y),
            QuickSettingsHit::SetDisplayPercent(0)
        );
        assert_eq!(
            check_quick_settings_slider_drag(size, disp_x + 40.0, disp_y),
            Some(QuickSettingsSliderDrag::Display)
        );
        assert_eq!(
            check_quick_settings_click(size, disp_x + disp_w - 1.0, disp_y),
            QuickSettingsHit::SetDisplayPercent(100)
        );
        assert_eq!(
            quick_settings_slider_percent(
                size,
                QuickSettingsSliderDrag::Display,
                disp_x + disp_w * 0.5
            ),
            50
        );

        let tinted = scale_glass_rgba([20, 14, 34, 200], 0.50, false);
        assert!(tinted[3] < 200 && tinted[3] >= 40);
        let opaque = scale_glass_rgba([20, 14, 34, 180], 0.40, true);
        assert!(opaque[3] >= 246);
    }

    #[test]
    fn macos27_top_bar_dynamic_left_right_and_menu_popover_hit_testing() {
        let without_bat = compute_top_bar_right_layout(1280, false, 0, "Wed Apr 1  9:41 AM");
        let with_bat = compute_top_bar_right_layout(1280, true, 0, "Wed Apr 1  9:41 AM");
        assert!(without_bat.battery_x.is_none());
        assert!(without_bat.volume_x.is_none());
        assert!(without_bat.kbd_x.is_none());
        assert!(with_bat.battery_x.is_some());
        let bat_x = with_bat.battery_x.unwrap_or(i32::MAX);
        // Exact macOS right order: Battery · Wi-Fi · Search · Control Center · Clock
        assert!(bat_x < with_bat.wifi_x);
        assert!(with_bat.wifi_x < with_bat.search_x);
        assert!(with_bat.search_x < with_bat.control_center_x);
        assert!(with_bat.control_center_x < with_bat.clock_x);

        let all_opts = compute_top_bar_right_layout_with_options(
            1280,
            "Wed Apr 1  9:41 AM",
            TopBarLayoutOptions {
                has_battery: true,
                notification_count: 0,
                show_volume: true,
                show_input_source: true,
                show_workspaces: true,
                show_tray: true,
            },
        );
        assert!(all_opts.volume_x.is_some());
        assert!(all_opts.kbd_x.is_some());
        assert!(all_opts.kbd_x.unwrap_or(0) < all_opts.volume_x.unwrap_or(0));
        assert!(all_opts.volume_x.unwrap_or(0) < all_opts.battery_x.unwrap_or(0));

        assert!(check_top_bar_search_click_for_options(
            1280,
            "Wed Apr 1  9:41 AM",
            TopBarLayoutOptions::default(),
            f64::from(without_bat.search_x + without_bat.search_w / 2),
            12.0,
        ));

        let short_left = compute_top_bar_left_layout_with_options("RustyOS", true);
        let wide_left = compute_top_bar_left_layout_with_options("Visual Studio Code", true);
        assert!(wide_left.app_w > short_left.app_w);
        assert!(wide_left.help_x > short_left.help_x);
        assert!(wide_left.ws_start_x > short_left.ws_start_x);
        assert_eq!(
            check_top_bar_workspace_click_with_options(
                "Visual Studio Code",
                true,
                f64::from(wide_left.ws_start_x + 4),
                12.0
            ),
            Some(1)
        );
        assert!(check_top_bar_layout_click_for_app(
            "Visual Studio Code",
            f64::from(wide_left.layout_x + 10),
            12.0
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
        let (gx, gy, _gw, _gh) = top_bar_menu_popover_rect("RustyOS", TopBarMenuHit::Go);
        assert_eq!(
            check_top_bar_menu_popover_click(
                "RustyOS",
                TopBarMenuHit::Go,
                f64::from(gx + 24),
                f64::from(gy + 16),
            ),
            TopBarMenuPopoverClick::Action(TopBarMenuAction::OpenOverview)
        );
        assert_eq!(
            check_top_bar_menu_popover_click(
                "RustyOS",
                TopBarMenuHit::Go,
                f64::from(gx + 24),
                f64::from(gy + 44),
            ),
            TopBarMenuPopoverClick::Action(TopBarMenuAction::NextWorkspace)
        );
        assert_eq!(
            check_top_bar_menu_popover_click("RustyOS", TopBarMenuHit::File, 800.0, 400.0),
            TopBarMenuPopoverClick::Outside
        );
    }

    #[test]
    fn macos_dock_divider_gap_tooltip_and_radii_constants() {
        assert_eq!(DOCK_HEIGHT, 68);
        assert_eq!(DOCK_RADIUS, 22);
        assert_eq!(DOCK_ICON_SIZE, 50);
        assert_eq!(DOCK_ITEM_GAP, 6);
        assert_eq!(WINDOW_RADIUS, 20);
        let last_app_slot_opt = dock_slot_rect(1280, 800, 2, 7);
        let downloads_slot_opt = dock_slot_rect(1280, 800, 2, 8);
        assert!(last_app_slot_opt.is_some());
        assert!(downloads_slot_opt.is_some());
        let last_app_slot =
            last_app_slot_opt.unwrap_or_else(|| Rectangle::from_size(Size::from((0, 0))));
        let downloads_slot =
            downloads_slot_opt.unwrap_or_else(|| Rectangle::from_size(Size::from((0, 0))));
        assert_eq!(
            downloads_slot.loc.x - (last_app_slot.loc.x + last_app_slot.size.w),
            DOCK_ITEM_GAP + DOCK_DIVIDER_GAP
        );

        let dock_view = DockViewState {
            pinned_icons: [None, None],
            windows: &[],
            hover_slot: Some(0),
            overview_active: false,
            hover_progress: 1.0,
            reduced_motion: false,
        };
        let tooltip_opt = dock_tooltip_rect_and_label(1280, 800, &dock_view);
        assert!(tooltip_opt.is_some());
        if let Some((pill_rect, label)) = tooltip_opt {
            assert_eq!(label, "Files");
            assert!(pill_rect.loc.y + pill_rect.size.h < 800 - DOCK_HEIGHT - DOCK_MARGIN_BOTTOM);
        }
    }

    #[test]
    fn macos27_menu_entries_and_overview_spaces_hit_testing() {
        // 1. Rich menu entries have separators, leading SDF icons, shortcuts & accurate hover rows
        let win_entries = top_bar_menu_entries(TopBarMenuHit::Window);
        assert!(
            win_entries
                .iter()
                .any(|e| matches!(e, TopBarMenuEntry::Separator))
        );
        let (wx, wy, _mw, _mh) = top_bar_menu_popover_rect("RustyOS", TopBarMenuHit::Window);
        assert_eq!(
            check_top_bar_menu_popover_hover_row(
                "RustyOS",
                TopBarMenuHit::Window,
                f64::from(wx + 24),
                f64::from(wy + MENU_POPOVER_PAD_Y + 10)
            ),
            Some(0)
        );
        // Separator gap does not trigger hover highlight
        assert_eq!(
            check_top_bar_menu_popover_hover_row(
                "RustyOS",
                TopBarMenuHit::Window,
                f64::from(wx + 24),
                f64::from(wy + MENU_POPOVER_PAD_Y + MENU_ITEM_HEIGHT + 4)
            ),
            None
        );

        // 2. Overview Spaces strip hit-testing switches workspace cleanly
        let size = Size::from((1280, 800));
        let spaces = [
            OverviewWorkspaceSpace {
                id: 1,
                window_count: 1,
                active: true,
                preview_rects: [
                    (0.1, 0.1, 0.4, 0.4),
                    (0.0, 0.0, 0.0, 0.0),
                    (0.0, 0.0, 0.0, 0.0),
                    (0.0, 0.0, 0.0, 0.0),
                ],
            },
            OverviewWorkspaceSpace {
                id: 2,
                window_count: 2,
                active: false,
                preview_rects: [
                    (0.1, 0.1, 0.35, 0.7),
                    (0.55, 0.1, 0.35, 0.7),
                    (0.0, 0.0, 0.0, 0.0),
                    (0.0, 0.0, 0.0, 0.0),
                ],
            },
        ];
        let space_rects = overview_space_rects(size, spaces.len());
        assert_eq!(space_rects.len(), 2);
        let s2 = space_rects[1];
        assert_eq!(
            check_overview_click_with_spaces(
                size,
                &spaces,
                1,
                f64::from(s2.loc.x + s2.size.w / 2),
                f64::from(s2.loc.y + s2.size.h / 2)
            ),
            OverviewHit::SwitchWorkspace(2)
        );
    }

    #[test]
    fn macos27_traffic_hover_notifications_and_osd_routing() {
        let size = Size::from((1280, 800));

        // 3. SSD traffic-light hover helper respects stacking & CSD windows
        let ssd_win = SsdHitWindow {
            id: rusty_wm::WindowId(10),
            rect: rusty_wm::Rect::new(100, 80, 500, 360),
            has_ssd: true,
        };
        let csd_win = SsdHitWindow {
            id: rusty_wm::WindowId(11),
            rect: rusty_wm::Rect::new(200, 120, 400, 300),
            has_ssd: false,
        };
        assert_eq!(
            check_window_traffic_lights_hover(&[ssd_win], 116.0, 92.0),
            Some(rusty_wm::WindowId(10))
        );
        assert_eq!(
            check_window_traffic_lights_hover(&[ssd_win], 300.0, 92.0),
            None
        );
        assert_eq!(
            check_window_traffic_lights_hover(&[csd_win], 216.0, 132.0),
            None
        );

        // 4. Notifications (340 px, squircle icon choice) & OSD SDF glyph routing
        let notif_rects = notification_toast_rects(size, 1);
        assert_eq!(notif_rects[0].size.w, 340);
        assert_eq!(
            notification_app_icon("Foot Terminal"),
            crate::icons::BuiltinAppIcon::Terminal
        );
        assert_eq!(
            notification_app_icon("System Update"),
            crate::icons::BuiltinAppIcon::RustySystem
        );
        assert_eq!(
            osd_glyph_kind("Brightness", Some(70), false),
            crate::surfaces::UiGlyphKind::Sun
        );
        assert_eq!(
            osd_glyph_kind("Volume", Some(45), false),
            crate::surfaces::UiGlyphKind::Volume {
                level_percent: 45,
                muted: false
            }
        );
        assert_eq!(
            osd_glyph_kind("Screenshot Saved", None, false),
            crate::surfaces::UiGlyphKind::Info
        );
    }

    #[test]
    fn lock_screen_password_pill_and_polkit_modal_hit_testing() {
        let size = Size::from((1280, 800));
        let layout = compute_lock_screen_layout(size, "EN", true, &["anc"], 0, true);
        assert!(layout.password_submit_rect.is_some());
        assert!(layout.password_pill_rect.is_some());
        if let (Some(submit), Some(pill)) = (layout.password_submit_rect, layout.password_pill_rect)
        {
            let lock_state = LockScreenRenderState {
                date_label: "Thu Sep 10",
                clock_hhmm: "8:52",
                keyboard_layout: "EN",
                has_battery: true,
                battery_percent: 100,
                battery_charging: false,
                wifi_connected: true,
                wifi_signal_percent: Some(88),
                wallpaper_path: None,
                users: &[],
                selected_user: 0,
                power_menu_open: false,
                require_password: true,
                password_len: 5,
                auth_checking: false,
                auth_error: None,
                glass_opacity: 0.78,
                reduce_transparency: false,
                progress: 1.0,
            };
            assert_eq!(
                hit_test_lock_screen(
                    size,
                    &lock_state,
                    f64::from(submit.loc.x + submit.size.w / 2),
                    f64::from(submit.loc.y + submit.size.h / 2)
                ),
                LockScreenHit::SubmitPassword
            );
            assert_eq!(
                hit_test_lock_screen(
                    size,
                    &lock_state,
                    f64::from(pill.loc.x + 24),
                    f64::from(pill.loc.y + 12)
                ),
                LockScreenHit::None
            );
        }

        let pk_layout = polkit_modal_layout(size);
        assert_eq!(
            hit_test_polkit_dialog(
                size,
                f64::from(pk_layout.submit.loc.x + 20),
                f64::from(pk_layout.submit.loc.y + 14)
            ),
            PolkitHit::Submit
        );
        assert_eq!(
            hit_test_polkit_dialog(
                size,
                f64::from(pk_layout.cancel.loc.x + 20),
                f64::from(pk_layout.cancel.loc.y + 14)
            ),
            PolkitHit::Cancel
        );
        assert_eq!(
            hit_test_polkit_dialog(
                size,
                f64::from(pk_layout.input.loc.x + 30),
                f64::from(pk_layout.input.loc.y + 14)
            ),
            PolkitHit::PasswordField
        );
        assert_eq!(hit_test_polkit_dialog(size, 20.0, 20.0), PolkitHit::Outside);
    }

    #[test]
    fn portal_modal_hit_testing() {
        let size = Size::from((1280, 800));
        let portal_layout = portal_modal_layout(size, 3, 1, true);
        assert_eq!(
            hit_test_portal_dialog(
                size,
                3,
                1,
                true,
                f64::from(portal_layout.row_rects[1].loc.x + 20),
                f64::from(portal_layout.row_rects[1].loc.y + 14),
            ),
            PortalHit::Row(1)
        );
        if let Some(toggle) = portal_layout.cursor_toggle {
            assert_eq!(
                hit_test_portal_dialog(
                    size,
                    3,
                    1,
                    true,
                    f64::from(toggle.loc.x + 20),
                    f64::from(toggle.loc.y + 10),
                ),
                PortalHit::CursorToggle
            );
        }
        assert_eq!(
            hit_test_portal_dialog(
                size,
                3,
                1,
                true,
                f64::from(portal_layout.accept.loc.x + 20),
                f64::from(portal_layout.accept.loc.y + 14),
            ),
            PortalHit::Accept
        );
        assert_eq!(
            hit_test_portal_dialog(
                size,
                3,
                1,
                true,
                f64::from(portal_layout.cancel.loc.x + 20),
                f64::from(portal_layout.cancel.loc.y + 14),
            ),
            PortalHit::Cancel
        );
        assert_eq!(
            hit_test_portal_dialog(size, 3, 1, true, 20.0, 20.0),
            PortalHit::Outside
        );
    }
}
