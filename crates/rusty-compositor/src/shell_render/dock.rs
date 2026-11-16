//! Floating Liquid Glass Dock layout, local icon hover, hit-testing, tooltips, and solid fallback rendering (`RUSTYOS-023` / `RUSTYOS-050` / `RUSTYOS-073`).

use smithay::backend::renderer::element::solid::SolidColorRenderElement;
use smithay::utils::{Point, Rectangle, Size};

use crate::config::{DEFAULT_DOCK_ICON_SIZE, DockPosition, clamp_dock_icon_size};

use super::tokens::{
    COLOR_ACCENT, COLOR_ACCENT_HOVER, COLOR_ACCENT_MUTED, COLOR_BORDER_HIGHLIGHT, COLOR_MUTED,
    COLOR_SUBTLE, COLOR_SURFACE_ELEVATED, DOCK_DIVIDER_GAP, DOCK_HEIGHT, DOCK_ITEM_GAP,
    DOCK_MARGIN_BOTTOM, DOCK_PADDING, DOCK_PINNED_COUNT, DOCK_UTILITY_COUNT, TOP_BAR_HEIGHT,
    make_solid_element, truncate_ascii,
};

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

/// Visual section inside the macOS 27 Dock strip (`RUSTYOS-073`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockSection {
    /// Pinned applications (`[dock].pinned_apps`).
    Pinned,
    /// Unpinned running applications and recently closed unpinned applications.
    UnpinnedOrRecent,
    /// Right-hand minimized window thumbnails (when `minimize_into_app == false`).
    MinimizedWindow,
    /// Right-hand utility icons (`Downloads`, `Trash`).
    Utility,
}

/// Interactive Dock slot descriptor built per frame for rendering and hit-testing (`RUSTYOS-073`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockAppTile {
    /// Logical section inside the Dock capsule.
    pub section: DockSection,
    /// Canonical lowercase application key (`"files"`, `"overview"`, `"firefox"`, ...).
    pub app_key: String,
    /// Tooltip / accessibility label.
    pub label: String,
    /// Compact click/context-menu target.
    pub hit: DockHit,
    /// Built-in SSAA squircle icon when the tile maps to a native RustyOS icon.
    pub builtin_icon: Option<crate::icons::BuiltinAppIcon>,
    /// Primary managed window associated with this tile (if any).
    pub representative_win: Option<rusty_wm::WindowId>,
    /// Total managed windows matching this app key.
    pub window_count: usize,
    /// Whether at least one matching window is currently minimized.
    pub has_minimized: bool,
    /// Whether this app/window currently owns focus or active overlay highlight.
    pub is_active: bool,
    /// Whether an open-application dot indicator should be shown (when enabled).
    pub has_indicator: bool,
    /// Optional XDG icon name/path resolved from client metadata.
    pub icon_name: Option<String>,
    /// Optional `xdg_toplevel_icon_v1` client icon.
    pub toplevel_icon: Option<crate::icons::ShmToplevelIcon>,
    /// Deterministic hue seed title fallback for unthemed windows.
    pub fallback_title: String,
}

/// Active Dock pinned-icon drag preview state passed to layout (`RUSTYOS-073`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DockDragPreview<'a> {
    /// Canonical app key currently dragged.
    pub app_key: &'a str,
    /// Original index inside `[dock].pinned_apps`.
    pub source_pinned_idx: usize,
    /// Target insertion index inside `[dock].pinned_apps`.
    pub target_pinned_idx: usize,
    /// Pointer coordinates relative to the primary output (`Logical` / `1x` px).
    pub pointer_xy: (f64, f64),
    /// `true` once dragged far enough outside the Dock to trigger `Remove`.
    pub pulled_outside: bool,
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

const CANONICAL_DOCK_ALIASES: &[(&str, &[&str])] = &[
    (
        "files",
        &[
            "rusty-files",
            "org.rustyos.files",
            "org.gnome.nautilus",
            "nautilus",
            "dolphin",
            "thunar",
        ],
    ),
    (
        "updater",
        &["rusty-updater", "org.rustyos.updater", "updater"],
    ),
    (
        "terminal",
        &[
            "foot",
            "kitty",
            "alacritty",
            "wezterm",
            "ghostty",
            "weston-terminal",
            "gnome-terminal",
            "konsole",
            "xfce4-terminal",
        ],
    ),
    (
        "google-chrome",
        &[
            "google-chrome",
            "google-chrome-stable",
            "com.google.chrome",
            "google chrome",
        ],
    ),
    (
        "chromium",
        &["chromium", "chromium-browser", "org.chromium.chromium"],
    ),
    (
        "firefox",
        &[
            "firefox",
            "org.mozilla.firefox",
            "firefox-esr",
            "firefox-nightly",
            "firefox-developer-edition",
            "mozilla firefox",
        ],
    ),
    ("brave-browser", &["brave-browser", "com.brave.browser"]),
    ("browser", &["browser", "safari", "org.rustyos.browser"]),
    (
        "notes",
        &[
            "gnome-text-editor",
            "gedit",
            "kate",
            "mousepad",
            "featherpad",
            "obsidian",
        ],
    ),
    (
        "settings",
        &["rusty-settings", "gnome-control-center", "systemsettings"],
    ),
];

/// Normalizes a Wayland `app_id` / `WM_CLASS` + window title into a canonical Dock grouping key (`RUSTYOS-073`).
#[must_use]
pub fn canonical_dock_app_key(
    app_id: Option<&str>,
    title: &str,
    win_id: rusty_wm::WindowId,
) -> String {
    let id_lower = app_id.unwrap_or_default().trim().to_ascii_lowercase();
    let title_lower = title.trim().to_ascii_lowercase();
    if !id_lower.is_empty() {
        for &(canonical, aliases) in CANONICAL_DOCK_ALIASES {
            if aliases
                .iter()
                .any(|a| id_lower == *a || id_lower.contains(a))
            {
                return canonical.to_owned();
            }
        }
        return id_lower.trim_end_matches(".desktop").to_owned();
    }

    for &(canonical, aliases) in CANONICAL_DOCK_ALIASES {
        if aliases.iter().any(|a| title_lower.contains(a)) {
            return canonical.to_owned();
        }
    }
    let clean_title = title.trim();
    if !clean_title.is_empty() && !is_synthetic_wm_title(clean_title) {
        return format!(
            "title:{}",
            extract_app_name(clean_title).to_ascii_lowercase()
        );
    }
    format!("win-{}", win_id.0)
}

fn is_synthetic_wm_title(title: &str) -> bool {
    title
        .strip_prefix("Window ")
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
}

/// Returns the built-in squircle icon mapped to `app_key`, if any (`RUSTYOS-073`).
#[must_use]
pub fn dock_builtin_for_key(app_key: &str) -> Option<crate::icons::BuiltinAppIcon> {
    match app_key.trim().to_ascii_lowercase().as_str() {
        "files" | "rusty-files" => Some(crate::icons::BuiltinAppIcon::Files),
        "updater" | "rusty-updater" => Some(crate::icons::BuiltinAppIcon::Updater),
        "overview" | "apps" => Some(crate::icons::BuiltinAppIcon::Overview),
        "terminal" => Some(crate::icons::BuiltinAppIcon::Terminal),
        "browser" => Some(crate::icons::BuiltinAppIcon::Browser),
        "notes" => Some(crate::icons::BuiltinAppIcon::Notes),
        "settings" => Some(crate::icons::BuiltinAppIcon::Settings),
        _ => None,
    }
}

/// Human-readable Dock tooltip label for a canonical or custom app key (`RUSTYOS-073`).
#[must_use]
pub fn dock_label_for_key(app_key: &str) -> String {
    match app_key.trim().to_ascii_lowercase().as_str() {
        "files" | "rusty-files" => "Files".to_owned(),
        "updater" | "rusty-updater" => "Software Update".to_owned(),
        "overview" | "apps" => "Overview".to_owned(),
        "terminal" => "Terminal".to_owned(),
        "browser" => "Browser".to_owned(),
        "google-chrome" => "Google Chrome".to_owned(),
        "chromium" => "Chromium".to_owned(),
        "firefox" => "Firefox".to_owned(),
        "brave-browser" => "Brave".to_owned(),
        "notes" => "Notes".to_owned(),
        "settings" => "Settings".to_owned(),
        other => {
            let short = other
                .trim_start_matches("title:")
                .rsplit('.')
                .next()
                .unwrap_or(other);
            let mut chars = short.chars();
            match chars.next() {
                Some(first) => {
                    let mut titled = first.to_ascii_uppercase().to_string();
                    titled.push_str(chars.as_str());
                    truncate_ascii(&titled, 22)
                }
                None => "App".to_owned(),
            }
        }
    }
}

/// Built-in Finder (`Files`) and `Overview` (`Apps`) pins stay anchored and cannot be dragged to `Remove`.
#[must_use]
pub fn is_dock_pin_removable(app_key: &str) -> bool {
    !matches!(
        app_key.trim().to_ascii_lowercase().as_str(),
        "files" | "overview"
    )
}

/// Maps built-in keys to legacy [`DockHit`] variants or `PinnedApp(idx)` for custom apps (`RUSTYOS-073`).
#[must_use]
pub fn dock_hit_for_pinned_key(app_key: &str, pinned_idx: usize) -> DockHit {
    match app_key.trim().to_ascii_lowercase().as_str() {
        "files" | "rusty-files" => DockHit::PinnedFiles,
        "overview" | "apps" => DockHit::ToggleOverview,
        "terminal" => DockHit::PinnedTerminal,
        "browser" => DockHit::PinnedBrowser,
        "notes" => DockHit::PinnedNotes,
        "settings" => DockHit::PinnedSettings,
        _ => DockHit::PinnedApp(pinned_idx),
    }
}

/// Dock view state describing tiles, local icon hover, auto-hide reveal, and drag preview (`RUSTYOS-023` / `RUSTYOS-073`).
#[derive(Debug, Clone, Copy)]
pub struct DockViewState<'a> {
    /// Legacy XDG icon hints preserved for struct literal compatibility.
    pub pinned_icons: [Option<&'a str>; 2],
    /// Open visible windows on the active workspace in stable slot order.
    pub windows: &'a [DockWindowEntry],
    /// Pre-grouped macOS 27 Dock tiles (`Pinned`, `UnpinnedOrRecent`, `MinimizedWindow`, `Utility`).
    pub tiles: &'a [DockAppTile],
    /// Currently hovered Dock slot index inside computed tiles, if any.
    pub hover_slot: Option<usize>,
    /// Whether Workspace Overview is active (highlights Dock Overview squircle tile).
    pub overview_active: bool,
    /// Smooth 120 ms hover progress factor (`0.0..=1.0`) (`RUSTYOS-048`).
    pub hover_progress: f64,
    /// Prefer instant subtle lift without animated hover scaling (`RUSTYOS-048`).
    pub reduced_motion: bool,
    /// Base Dock icon edge size (`36..=68`, default `50`) (`RUSTYOS-073`).
    pub icon_size: i32,
    /// Screen edge anchoring the Dock (`Bottom`, `Left`, `Right`) (`RUSTYOS-073`).
    pub position: DockPosition,
    /// Auto-hide reveal progress (`1.0` = visible, `0.0` = slid offscreen) (`RUSTYOS-073`).
    pub auto_hide_reveal: f64,
    /// Render running-application dot indicators (`RUSTYOS-073`).
    pub show_indicators: bool,
    /// Active opening-app bounce offsets `(app_key, jump_px)` (`RUSTYOS-073`).
    pub bounces: &'a [(&'a str, f64)],
    /// Active pinned-app drag-to-reorder / drag-to-remove preview (`RUSTYOS-073`).
    pub drag_preview: Option<DockDragPreview<'a>>,
}

impl Default for DockViewState<'_> {
    fn default() -> Self {
        Self {
            pinned_icons: [None, None],
            windows: &[],
            tiles: &[],
            hover_slot: None,
            overview_active: false,
            hover_progress: 0.0,
            reduced_motion: false,
            icon_size: DEFAULT_DOCK_ICON_SIZE,
            position: DockPosition::Bottom,
            auto_hide_reveal: 1.0,
            show_indicators: true,
            bounces: &[],
            drag_preview: None,
        }
    }
}

/// Interactive click target inside the floating Dock (`RUSTYOS-023` / `RUSTYOS-050` / `RUSTYOS-073`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockHit {
    /// Pinned Files launcher (`Files`).
    PinnedFiles,
    /// Workspace Overview / Apps toggle (`Overview`).
    ToggleOverview,
    /// Pinned Terminal launcher (`Terminal`).
    PinnedTerminal,
    /// Pinned Browser launcher (`Browser`).
    PinnedBrowser,
    /// Pinned Notes launcher (`Notes`).
    PinnedNotes,
    /// Pinned System Settings launcher (`Settings`).
    PinnedSettings,
    /// Custom user-pinned application at `index` inside `[dock].pinned_apps` (`RUSTYOS-073`).
    PinnedApp(usize),
    /// Unpinned running app / window tile at `index`.
    WindowSlot(usize),
    /// Recently closed unpinned app at `index` inside `recent_unpinned_apps` (`RUSTYOS-073`).
    RecentApp(usize),
    /// Minimized window thumbnail in right utility section (`RUSTYOS-073`).
    MinimizedWindow(rusty_wm::WindowId),
    /// Utility folder / Stack shortcut (`Downloads`).
    PinnedDownloads,
    /// Utility wastebasket shortcut (`Trash`).
    PinnedTrash,
}

/// Concrete screen-space geometry for a single Dock slot (`RUSTYOS-073`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComputedDockSlot {
    /// Index in rendered slot order (`0..slots.len()`).
    pub slot_idx: usize,
    /// Source descriptor (metadata + hit target + icons).
    pub tile: DockAppTile,
    /// Resting/expanded slot box used for hit-testing and target anchors.
    pub hit_rect: Rectangle<i32, smithay::utils::Physical>,
    /// Final icon draw box including local hover scaling, bounce offset, and cross-axis lift.
    pub icon_rect: Rectangle<i32, smithay::utils::Physical>,
    /// Open-application indicator dot center box (`5×5` px).
    pub dot_rect: Rectangle<i32, smithay::utils::Physical>,
}

/// Full computed Dock layout for rendering, hit-testing, tooltips, and minimize targets (`RUSTYOS-073`).
#[derive(Debug, Clone)]
pub struct ComputedDockLayout {
    /// Glass Dock background capsule bounds in screen space.
    pub capsule_rect: Rectangle<i32, smithay::utils::Physical>,
    /// Optional hairline section dividers (`Pinned | Unpinned/Recent` and `Left | Utility`).
    pub dividers: Vec<Rectangle<i32, smithay::utils::Physical>>,
    /// Per-slot computed bounds and descriptors.
    pub slots: Vec<ComputedDockSlot>,
    /// Floating `"Remove"` pill badge shown when a pinned icon is dragged off the Dock.
    pub drag_remove_badge: Option<Rectangle<i32, smithay::utils::Physical>>,
    /// Floating dragged icon bounds centered on the pointer (when dragging a pinned icon).
    pub dragged_icon: Option<(DockAppTile, Rectangle<i32, smithay::utils::Physical>)>,
}

/// Builds legacy fallback tiles from `[Files..Settings] + windows + [Downloads, Trash]`
/// when callers supply only `DockWindowEntry` slices (used by standalone unit tests).
#[must_use]
pub(super) fn build_default_dock_tiles(
    windows: &[DockWindowEntry],
    overview_active: bool,
) -> Vec<DockAppTile> {
    let mut tiles = Vec::with_capacity(DOCK_PINNED_COUNT + windows.len() + DOCK_UTILITY_COUNT);
    for (idx, key) in [
        "files", "overview", "terminal", "browser", "notes", "settings",
    ]
    .into_iter()
    .enumerate()
    {
        let hit = dock_hit_for_pinned_key(key, idx);
        let active = (key == "overview" && overview_active) || key == "files";
        tiles.push(DockAppTile {
            section: DockSection::Pinned,
            app_key: key.to_owned(),
            label: dock_label_for_key(key),
            hit,
            builtin_icon: dock_builtin_for_key(key),
            representative_win: None,
            window_count: 0,
            has_minimized: false,
            is_active: active,
            has_indicator: active,
            icon_name: None,
            toplevel_icon: None,
            fallback_title: dock_label_for_key(key),
        });
    }
    for (idx, w) in windows.iter().enumerate() {
        tiles.push(DockAppTile {
            section: DockSection::UnpinnedOrRecent,
            app_key: format!("win-{}", w.id.0),
            label: truncate_ascii(&w.title, 26),
            hit: DockHit::WindowSlot(idx),
            builtin_icon: None,
            representative_win: Some(w.id),
            window_count: 1,
            has_minimized: w.minimized,
            is_active: w.focused && !w.minimized,
            has_indicator: true,
            icon_name: w.icon_name.clone(),
            toplevel_icon: w.toplevel_icon.clone(),
            fallback_title: w.title.clone(),
        });
    }
    append_utility_dock_tiles(&mut tiles);
    tiles
}

/// Appends the right-side utility slots (`Downloads`, `Trash`) to `tiles` (`RUSTYOS-073`).
pub fn append_utility_dock_tiles(tiles: &mut Vec<DockAppTile>) {
    tiles.push(DockAppTile {
        section: DockSection::Utility,
        app_key: "downloads".to_owned(),
        label: "Downloads".to_owned(),
        hit: DockHit::PinnedDownloads,
        builtin_icon: Some(crate::icons::BuiltinAppIcon::Downloads),
        representative_win: None,
        window_count: 0,
        has_minimized: false,
        is_active: false,
        has_indicator: false,
        icon_name: None,
        toplevel_icon: None,
        fallback_title: "Downloads".to_owned(),
    });
    tiles.push(DockAppTile {
        section: DockSection::Utility,
        app_key: "trash".to_owned(),
        label: "Trash".to_owned(),
        hit: DockHit::PinnedTrash,
        builtin_icon: Some(crate::icons::BuiltinAppIcon::Trash),
        representative_win: None,
        window_count: 0,
        has_minimized: false,
        is_active: false,
        has_indicator: false,
        icon_name: None,
        toplevel_icon: None,
        fallback_title: "Trash".to_owned(),
    });
}

fn reorder_pinned_tiles_for_drag(
    raw_tiles: &[DockAppTile],
    drag: Option<DockDragPreview<'_>>,
) -> (Vec<DockAppTile>, Option<DockAppTile>) {
    let mut dragged_tile = None;
    let Some(drag) = drag else {
        return (raw_tiles.to_vec(), None);
    };
    let mut pinned: Vec<DockAppTile> = Vec::new();
    let mut tail: Vec<DockAppTile> = Vec::new();
    for tile in raw_tiles {
        if tile.section == DockSection::Pinned {
            if tile.app_key == drag.app_key && dragged_tile.is_none() {
                dragged_tile = Some(tile.clone());
            } else {
                pinned.push(tile.clone());
            }
        } else {
            tail.push(tile.clone());
        }
    }
    if !drag.pulled_outside
        && let Some(ref tile) = dragged_tile
    {
        let insert_idx = drag.target_pinned_idx.min(pinned.len());
        pinned.insert(insert_idx, tile.clone());
    }
    pinned.extend(tail);
    (pinned, dragged_tile)
}

const fn has_divider_between(prev: DockSection, cur: DockSection) -> bool {
    matches!(
        (prev, cur),
        (DockSection::Pinned, DockSection::UnpinnedOrRecent)
            | (
                DockSection::Pinned | DockSection::UnpinnedOrRecent,
                DockSection::MinimizedWindow | DockSection::Utility
            )
    )
}

fn dock_cross_origin(
    screen_size: Size<i32, smithay::utils::Physical>,
    pos: DockPosition,
    thickness: i32,
    reveal: f64,
) -> i32 {
    let r = reveal.clamp(0.0, 1.0);
    let (rest, hidden) = match pos {
        DockPosition::Bottom => (
            f64::from(screen_size.h - thickness - DOCK_MARGIN_BOTTOM),
            f64::from(screen_size.h + 8),
        ),
        DockPosition::Left => (f64::from(DOCK_MARGIN_BOTTOM), f64::from(-thickness - 8)),
        DockPosition::Right => (
            f64::from(screen_size.w - thickness - DOCK_MARGIN_BOTTOM),
            f64::from(screen_size.w + 8),
        ),
    };
    #[allow(clippy::cast_possible_truncation)]
    let out = (hidden + (rest - hidden) * r).round() as i32;
    out
}

fn map_axis_rect(
    pos: DockPosition,
    axis_pos: i32,
    cross_pos: i32,
    axis_len: i32,
    cross_len: i32,
) -> Rectangle<i32, smithay::utils::Physical> {
    match pos {
        DockPosition::Bottom => Rectangle::new(
            Point::from((axis_pos, cross_pos)),
            Size::from((axis_len, cross_len)),
        ),
        DockPosition::Left | DockPosition::Right => Rectangle::new(
            Point::from((cross_pos, axis_pos)),
            Size::from((cross_len, axis_len)),
        ),
    }
}

struct AxisGeometry<'a> {
    pos: DockPosition,
    base_icon: i32,
    base_slot: i32,
    thickness: i32,
    cross_origin: i32,
    capsule_axis_start: i32,
    scale_fit: f64,
    divider_before: &'a [bool],
}

#[allow(clippy::cast_possible_truncation)]
fn build_dock_slots_along_axis(
    tiles: &[DockAppTile],
    geom: &AxisGeometry<'_>,
    dock_view: &DockViewState<'_>,
) -> (
    Vec<Rectangle<i32, smithay::utils::Physical>>,
    Vec<ComputedDockSlot>,
) {
    let mut dividers = Vec::new();
    let mut slots = Vec::with_capacity(tiles.len());
    let mut cur_axis =
        f64::from(geom.capsule_axis_start) + f64::from(DOCK_PADDING) * geom.scale_fit;
    let slot_span_f = f64::from(geom.base_slot) * geom.scale_fit;
    let slot_span = slot_span_f.round().max(16.0) as i32;
    let base_draw = (f64::from(geom.base_icon) * geom.scale_fit)
        .round()
        .max(16.0) as i32;

    for (i, tile) in tiles.iter().enumerate() {
        if i > 0 {
            cur_axis += f64::from(DOCK_ITEM_GAP) * geom.scale_fit;
        }
        if geom.divider_before[i] {
            let div_offset = f64::from(DOCK_DIVIDER_GAP - DOCK_ITEM_GAP) * 0.5 * geom.scale_fit;
            let div_axis = (cur_axis + div_offset).round() as i32;
            dividers.push(map_axis_rect(
                geom.pos,
                div_axis,
                geom.cross_origin + 15,
                1,
                (geom.thickness - 30).max(16),
            ));
            cur_axis += f64::from(DOCK_DIVIDER_GAP) * geom.scale_fit;
        }

        let slot_axis = cur_axis.round() as i32;
        cur_axis += slot_span_f;

        let bounce_px = dock_view
            .bounces
            .iter()
            .find(|(k, _)| *k == tile.app_key)
            .map_or(0.0, |(_, px)| *px);
        let hovered = dock_view.hover_slot == Some(i);
        let (hover_scale_px, hover_lift_px) = if hovered {
            if dock_view.reduced_motion {
                (0.0, 2.0)
            } else {
                let hp = dock_view.hover_progress.clamp(0.35, 1.0);
                (8.0 * hp, 6.0 * hp)
            }
        } else {
            (0.0, 0.0)
        };
        let lift_f = hover_lift_px + bounce_px;

        let draw_size = ((f64::from(geom.base_icon) + hover_scale_px) * geom.scale_fit)
            .round()
            .max(16.0) as i32;
        let icon_axis = slot_axis + (slot_span - draw_size) / 2;
        let base_cross_icon = geom.cross_origin + DOCK_PADDING - 1;
        let pop_dir = match geom.pos {
            DockPosition::Bottom | DockPosition::Right => -1,
            DockPosition::Left => 1,
        };
        let extra_cross = if matches!(geom.pos, DockPosition::Bottom | DockPosition::Right) {
            draw_size - base_draw
        } else {
            0
        };
        let icon_cross = base_cross_icon - extra_cross + pop_dir * (lift_f.round() as i32);

        let hit_rect = map_axis_rect(
            geom.pos,
            slot_axis,
            geom.cross_origin + DOCK_PADDING,
            slot_span,
            geom.base_slot,
        );
        let icon_rect = map_axis_rect(geom.pos, icon_axis, icon_cross, draw_size, draw_size);
        let dot_cross = match geom.pos {
            DockPosition::Left => geom.cross_origin + 3,
            DockPosition::Bottom | DockPosition::Right => geom.cross_origin + geom.thickness - 8,
        };
        let dot_rect = map_axis_rect(geom.pos, slot_axis + (slot_span - 5) / 2, dot_cross, 5, 5);

        slots.push(ComputedDockSlot {
            slot_idx: i,
            tile: tile.clone(),
            hit_rect,
            icon_rect,
            dot_rect,
        });
    }
    (dividers, slots)
}

/// Computes single-source-of-truth Dock geometry across `Bottom`/`Left`/`Right`, local icon hover,
/// auto-hide offset, bounces, and drag preview (`RUSTYOS-073`).
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn compute_dock_layout(
    screen_size: Size<i32, smithay::utils::Physical>,
    dock_view: &DockViewState<'_>,
) -> ComputedDockLayout {
    let owned_default;
    let raw_tiles = if dock_view.tiles.is_empty() {
        owned_default = build_default_dock_tiles(dock_view.windows, dock_view.overview_active);
        owned_default.as_slice()
    } else {
        dock_view.tiles
    };

    let (tiles, dragged_tile) = reorder_pinned_tiles_for_drag(raw_tiles, dock_view.drag_preview);
    let base_icon = clamp_dock_icon_size(dock_view.icon_size);
    let base_slot = base_icon + 2;
    let thickness = base_icon + DOCK_PADDING * 2 + 2;
    let pos = dock_view.position;
    let (screen_axis, axis_origin_offset) = match pos {
        DockPosition::Bottom => (screen_size.w.max(200), 0),
        DockPosition::Left | DockPosition::Right => {
            ((screen_size.h - TOP_BAR_HEIGHT).max(200), TOP_BAR_HEIGHT)
        }
    };

    let mut divider_before = vec![false; tiles.len()];
    let mut div_count = 0_i32;
    for i in 1..tiles.len() {
        if has_divider_between(tiles[i - 1].section, tiles[i].section) {
            divider_before[i] = true;
            div_count += 1;
        }
    }

    let tile_count_i32 = i32::try_from(tiles.len()).unwrap_or(0);
    let gap_count_i32 = i32::try_from(tiles.len().saturating_sub(1)).unwrap_or(0);
    let raw_len = tile_count_i32 * base_slot
        + gap_count_i32 * DOCK_ITEM_GAP
        + DOCK_PADDING * 2
        + div_count * DOCK_DIVIDER_GAP;
    let max_len = (screen_axis - 36).max(160);
    let capsule_len = raw_len.clamp(120, max_len);
    let capsule_axis_start = axis_origin_offset + (screen_axis - capsule_len) / 2;
    let cross_origin = dock_cross_origin(screen_size, pos, thickness, dock_view.auto_hide_reveal);
    let scale_fit = if raw_len > max_len && raw_len > 1 {
        f64::from(max_len) / f64::from(raw_len)
    } else {
        1.0
    };

    let geom = AxisGeometry {
        pos,
        base_icon,
        base_slot,
        thickness,
        cross_origin,
        capsule_axis_start,
        scale_fit,
        divider_before: &divider_before,
    };
    let (dividers, slots) = build_dock_slots_along_axis(&tiles, &geom, dock_view);
    let capsule_rect = map_axis_rect(
        pos,
        capsule_axis_start,
        cross_origin,
        capsule_len,
        thickness,
    );
    let (dragged_icon, drag_remove_badge) =
        build_drag_overlay(&tiles, dragged_tile, dock_view, base_icon);

    ComputedDockLayout {
        capsule_rect,
        dividers,
        slots,
        drag_remove_badge,
        dragged_icon,
    }
}

type DragOverlayOutput = (
    Option<(DockAppTile, Rectangle<i32, smithay::utils::Physical>)>,
    Option<Rectangle<i32, smithay::utils::Physical>>,
);

#[allow(clippy::cast_possible_truncation)]
fn build_drag_overlay(
    tiles: &[DockAppTile],
    dragged_tile: Option<DockAppTile>,
    dock_view: &DockViewState<'_>,
    base_icon: i32,
) -> DragOverlayOutput {
    let Some(drag) = dock_view.drag_preview else {
        return (None, None);
    };
    let Some(tile) = dragged_tile.or_else(|| {
        tiles
            .iter()
            .find(|t| t.section == DockSection::Pinned && t.app_key == drag.app_key)
            .cloned()
    }) else {
        return (None, None);
    };
    let sz = (base_icon + 4).clamp(40, 76);
    let px = drag.pointer_xy.0.round() as i32 - sz / 2;
    let py = drag.pointer_xy.1.round() as i32 - sz / 2;
    let icon_rect = Rectangle::new(Point::from((px, py)), Size::from((sz, sz)));
    let badge = drag.pulled_outside.then(|| {
        Rectangle::new(
            Point::from((px + sz / 2 - 36, py - 30)),
            Size::from((72, 22)),
        )
    });
    (Some((tile, icon_rect)), badge)
}

/// Computes floating Dock layout bounds `(x, y, width, items_count)` using default settings (`RUSTYOS-023`).
#[must_use]
pub(super) fn dock_layout_bounds(
    width: i32,
    height: i32,
    window_count: usize,
) -> (i32, i32, i32, usize) {
    let dummy_wins = vec![
        DockWindowEntry {
            id: rusty_wm::WindowId(0),
            title: String::new(),
            badge: String::new(),
            icon_name: None,
            toplevel_icon: None,
            dimensions: String::new(),
            focused: false,
            minimized: false,
        };
        window_count
    ];
    let numbered_wins: Vec<DockWindowEntry> = dummy_wins
        .into_iter()
        .enumerate()
        .map(|(idx, mut w)| {
            w.id = rusty_wm::WindowId((idx as u64) + 1);
            w.title = format!("Window {}", idx + 1);
            w
        })
        .collect();
    let view = DockViewState {
        windows: &numbered_wins,
        ..DockViewState::default()
    };
    let layout = compute_dock_layout(Size::from((width, height)), &view);
    (
        layout.capsule_rect.loc.x,
        layout.capsule_rect.loc.y,
        layout.capsule_rect.size.w,
        layout.slots.len(),
    )
}

/// Computes resting hit rectangle of slot `slot_idx` under default Dock settings (`RUSTYOS-023`).
#[must_use]
pub fn dock_slot_rect(
    width: i32,
    height: i32,
    window_count: usize,
    slot_idx: usize,
) -> Option<Rectangle<i32, smithay::utils::Physical>> {
    let numbered_wins: Vec<DockWindowEntry> = (0..window_count)
        .map(|idx| DockWindowEntry {
            id: rusty_wm::WindowId((idx as u64) + 1),
            title: format!("Window {}", idx + 1),
            badge: "Wn".to_owned(),
            icon_name: None,
            toplevel_icon: None,
            dimensions: "800x600".to_owned(),
            focused: false,
            minimized: false,
        })
        .collect();
    let view = DockViewState {
        windows: &numbered_wins,
        ..DockViewState::default()
    };
    let layout = compute_dock_layout(Size::from((width, height)), &view);
    layout.slots.get(slot_idx).map(|s| s.hit_rect)
}

/// Computes tooltip bubble rectangle + label for hovered Dock slot across all Dock positions (`RUSTYOS-050` / `RUSTYOS-073`).
#[must_use]
pub(super) fn dock_tooltip_rect_and_label(
    width: i32,
    height: i32,
    dock_view: &DockViewState<'_>,
) -> Option<(Rectangle<i32, smithay::utils::Physical>, String)> {
    if dock_view.auto_hide_reveal < 0.35 || dock_view.drag_preview.is_some() {
        return None;
    }
    let slot_idx = dock_view.hover_slot?;
    let layout = compute_dock_layout(Size::from((width, height)), dock_view);
    let slot = layout.slots.get(slot_idx)?;
    let label = slot.tile.label.clone();
    let text_w = crate::text::measure_ui_text_width(&label, 12.0, crate::text::FontWeight::Bold);
    let pill_w = (text_w + 22).clamp(48, 220);
    let pill_h = 26;
    let rect = match dock_view.position {
        DockPosition::Bottom => {
            let cx = slot.icon_rect.loc.x + slot.icon_rect.size.w / 2;
            let pill_x = (cx - pill_w / 2).clamp(12, (width - pill_w - 12).max(12));
            let pill_y = (layout.capsule_rect.loc.y - pill_h - 14).max(TOP_BAR_HEIGHT + 8);
            Rectangle::new(Point::from((pill_x, pill_y)), Size::from((pill_w, pill_h)))
        }
        DockPosition::Left => {
            let cy = slot.icon_rect.loc.y + slot.icon_rect.size.h / 2;
            let pill_x = (layout.capsule_rect.loc.x + layout.capsule_rect.size.w + 14)
                .clamp(12, (width - pill_w - 12).max(12));
            let pill_y = (cy - pill_h / 2).clamp(
                TOP_BAR_HEIGHT + 8,
                (height - pill_h - 12).max(TOP_BAR_HEIGHT + 8),
            );
            Rectangle::new(Point::from((pill_x, pill_y)), Size::from((pill_w, pill_h)))
        }
        DockPosition::Right => {
            let cy = slot.icon_rect.loc.y + slot.icon_rect.size.h / 2;
            let pill_x = (layout.capsule_rect.loc.x - pill_w - 14).max(12);
            let pill_y = (cy - pill_h / 2).clamp(
                TOP_BAR_HEIGHT + 8,
                (height - pill_h - 12).max(TOP_BAR_HEIGHT + 8),
            );
            Rectangle::new(Point::from((pill_x, pill_y)), Size::from((pill_w, pill_h)))
        }
    };
    Some((rect, label))
}

/// Hit-tests `(x, y)` against computed Dock layout slots (`RUSTYOS-073`).
#[must_use]
pub fn check_dock_layout_slot(layout: &ComputedDockLayout, x: f64, y: f64) -> Option<usize> {
    for slot in &layout.slots {
        let r = slot.hit_rect;
        let rx = f64::from(r.loc.x);
        let ry = f64::from(r.loc.y);
        let rw = f64::from(r.size.w);
        let rh = f64::from(r.size.h);
        if x >= rx && x < rx + rw && y >= ry && y < ry + rh {
            return Some(slot.slot_idx);
        }
    }
    None
}

/// Returns `true` if pointer `(px, py)` should keep an auto-hidden Dock revealed (`RUSTYOS-073`).
#[must_use]
pub fn is_pointer_in_dock_auto_hide_zone(
    screen_size: Size<i32, smithay::utils::Physical>,
    dock_view: &DockViewState<'_>,
    currently_revealed: bool,
    px: f64,
    py: f64,
) -> bool {
    let screen_w = f64::from(screen_size.w.max(1));
    let screen_h = f64::from(screen_size.h.max(1));
    if py < f64::from(TOP_BAR_HEIGHT) {
        return false;
    }
    let edge_band = match dock_view.position {
        DockPosition::Bottom => py >= screen_h - 6.0,
        DockPosition::Left => px <= 6.0,
        DockPosition::Right => px >= screen_w - 6.0,
    };
    if edge_band {
        return true;
    }
    if !currently_revealed {
        return false;
    }
    let layout = compute_dock_layout(screen_size, dock_view);
    let pad = 20.0;
    let capsule = layout.capsule_rect;
    px >= f64::from(capsule.loc.x) - pad
        && px <= f64::from(capsule.loc.x + capsule.size.w) + pad
        && py >= f64::from(capsule.loc.y) - pad
        && py <= f64::from(capsule.loc.y + capsule.size.h) + pad
}

/// Hit-tests `(x, y)` against Dock slots using default settings (`RUSTYOS-023`).
#[must_use]
pub fn check_dock_slot(
    screen_size: Size<i32, smithay::utils::Physical>,
    window_count: usize,
    x: f64,
    y: f64,
) -> Option<usize> {
    let numbered_wins: Vec<DockWindowEntry> = (0..window_count)
        .map(|idx| DockWindowEntry {
            id: rusty_wm::WindowId((idx as u64) + 1),
            title: format!("Window {}", idx + 1),
            badge: "Wn".to_owned(),
            icon_name: None,
            toplevel_icon: None,
            dimensions: "800x600".to_owned(),
            focused: false,
            minimized: false,
        })
        .collect();
    let view = DockViewState {
        windows: &numbered_wins,
        ..DockViewState::default()
    };
    let layout = compute_dock_layout(screen_size, &view);
    check_dock_layout_slot(&layout, x, y)
}

/// Hit-tests a pointer click at `(x, y)` against default bottom Dock (`RUSTYOS-023` / `RUSTYOS-050`).
#[must_use]
pub fn check_dock_click(
    screen_size: Size<i32, smithay::utils::Physical>,
    window_count: usize,
    x: f64,
    y: f64,
) -> Option<DockHit> {
    let numbered_wins: Vec<DockWindowEntry> = (0..window_count)
        .map(|idx| DockWindowEntry {
            id: rusty_wm::WindowId((idx as u64) + 1),
            title: format!("Window {}", idx + 1),
            badge: "Wn".to_owned(),
            icon_name: None,
            toplevel_icon: None,
            dimensions: "800x600".to_owned(),
            focused: false,
            minimized: false,
        })
        .collect();
    let view = DockViewState {
        windows: &numbered_wins,
        ..DockViewState::default()
    };
    let layout = compute_dock_layout(screen_size, &view);
    let idx = check_dock_layout_slot(&layout, x, y)?;
    layout.slots.get(idx).map(|s| s.tile.hit)
}

pub(super) fn render_dock(
    elements: &mut Vec<SolidColorRenderElement>,
    width: i32,
    height: i32,
    window_count: usize,
    has_focused_window: bool,
) {
    let (dock_x, dock_y, dock_width, dock_items_count) =
        dock_layout_bounds(width, height, window_count);
    let dock_rim_geo = Rectangle::new(
        Point::from((dock_x - 1, dock_y - 1)),
        Size::from((dock_width + 2, DOCK_HEIGHT + 2)),
    );
    elements.push(make_solid_element(dock_rim_geo, COLOR_BORDER_HIGHLIGHT));
    let dock_geo = Rectangle::new(
        Point::from((dock_x, dock_y)),
        Size::from((dock_width, DOCK_HEIGHT)),
    );
    elements.push(make_solid_element(dock_geo, COLOR_SURFACE_ELEVATED));

    for i in 0..dock_items_count {
        let Some(slot_rect) = dock_slot_rect(width, height, window_count, i) else {
            continue;
        };
        let is_running_window = i >= DOCK_PINNED_COUNT && i < DOCK_PINNED_COUNT + window_count;
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
            0 => COLOR_ACCENT_MUTED,
            1 => COLOR_SUBTLE,
            2 => COLOR_ACCENT,
            3..=5 => COLOR_ACCENT_HOVER,
            _ if is_focused => COLOR_ACCENT_HOVER,
            _ => COLOR_MUTED,
        };
        elements.push(make_solid_element(inner_geo, inner_color));
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
