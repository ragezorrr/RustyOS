//! `TopBar` left-side application and global menus, items, popovers, and click handlers (`RUSTYOS-090`).

use crate::shell_render::tokens::{TOP_BAR_HEIGHT, TOP_BAR_LOGO_W, TOP_BAR_LOGO_X};
use crate::shell_render::top_bar_menus::{
    AppShortcutAction, ResolvedTopBarMenuItem, TopBarAppProfile, resolve_top_bar_menu_entries,
};

use super::panel::compute_top_bar_left_layout_for_profile;

/// Corner radius for concentric Menu Bar and context menu popovers (`DESIGN.md` §12.3, ADR-0058, ADR-0069).
pub const MENU_POPOVER_RADIUS: i32 = rusty_ui::menu::MENU_RADIUS;

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
    /// Live D-Bus `AppMenu` section index (`RUSTYOS-145`).
    Dynamic(usize),
    /// Collapsed overflow menus (`>>`) when left menus exceed available horizontal space.
    Overflow,
}

/// Action triggered from an open `TopBar` menu dropdown popover (`RUSTYOS-048`/`049`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopBarMenuAction {
    /// Show concise RustyOS notification card.
    AboutRustyOs,
    /// Open built-in System Settings overlay (`Super+,`) (`RUSTYOS-061`).
    OpenSettings,
    /// Open Software Updates popover and trigger an async check (`RUSTYOS-076`).
    CheckForUpdates,
    /// Reload `~/.config/rustyos/config.toml` immediately (`Super+Shift+R`) (`RUSTYOS-061`).
    ReloadConfig,
    /// Show concise keyboard shortcuts reference card (`RUSTYOS-049`).
    ShowShortcuts,
    /// Activate a live `com.canonical.dbusmenu` item by protocol id (`RUSTYOS-145`).
    DbusMenuItem(i32),
    /// Toggle Workspace Overview.
    OpenOverview,
    /// Toggle Control Center (`QuickSettings`).
    OpenControlCenter,
    /// Toggle Spotlight Search.
    OpenSpotlight,
    /// Open interactive Screenshot HUD (`Region / Window / Full Screen`, `RUSTYOS-133`).
    OpenScreenshotHud,
    /// Launch preferred terminal emulator.
    NewTerminal,
    /// Launch preferred file manager (`rusty-files`).
    OpenFiles,
    /// Create a new folder on `~/Desktop` (`RUSTYOS-067`).
    NewDesktopFolder,
    /// Open `rusty-files --recents` (`RUSTYOS-067`).
    GoRecents,
    /// Open `rusty-files` in `~/Desktop` (`RUSTYOS-067`).
    GoDesktop,
    /// Open `rusty-files` in `~/Documents` (`RUSTYOS-067`).
    GoDocuments,
    /// Open `rusty-files` in `~/Downloads` (`RUSTYOS-067`).
    GoDownloads,
    /// Open `rusty-files` in `~` (`Home`) (`RUSTYOS-067`).
    GoHome,
    /// Open `rusty-files --trash` (`RUSTYOS-067`).
    GoTrash,
    /// Empty the Freedesktop Trash directory asynchronously (`RUSTYOS-067`).
    EmptyTrash,
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
    /// Suspend system to RAM via systemd-logind (`RUSTYOS-080`).
    Sleep,
    /// Reboot the system via systemd-logind (`RUSTYOS-080`).
    Restart,
    /// Shut down the system via systemd-logind (`RUSTYOS-080`).
    ShutDown,
    /// Terminate compositor session loop and log out (`RUSTYOS-080`).
    LogOut,
    /// Cycle focus to next window.
    FocusNextWindow,
    /// Maximize / restore focused window.
    ToggleMaximizeFocused,
    /// Minimize focused window.
    MinimizeFocused,
    /// Tile focused window to the left half of the screen (`RUSTYOS-132`).
    TileLeftFocused,
    /// Tile focused window to the right half of the screen (`RUSTYOS-132`).
    TileRightFocused,
    /// Move focused window to the next workspace (`RUSTYOS-132`).
    MoveToNextWorkspace,
    /// Move focused window to the previous workspace (`RUSTYOS-132`).
    MoveToPrevWorkspace,
    /// Move focused window to the next display output (`RUSTYOS-132`).
    MoveToNextDisplay,
    /// Hide active application window (`RUSTYOS-127`).
    HideActiveApp,
    /// Hide all other windows in the active workspace (`RUSTYOS-127`).
    HideOtherApps,
    /// Show "About <ActiveApp>" notification/banner (`RUSTYOS-068`).
    AboutActiveApp,
    /// Toggle fullscreen on the focused window (`RUSTYOS-068`).
    ToggleFullscreenFocused,
    /// Gracefully quit the focused application (`RUSTYOS-068`).
    QuitActiveApp,
    /// Terminate the focused application process (`RUSTYOS-068`).
    ForceQuitActiveApp,
    /// Dispatch a synthetic shortcut to the focused application (`RUSTYOS-068`).
    AppShortcut(AppShortcutAction),
    /// Close focused window.
    CloseFocused,
}

/// Rich menu item entry with optional leading SDF glyph, optional shortcut label, and separator (`RUSTYOS-051`, `RUSTYOS-107`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopBarMenuEntry {
    /// Clickable action row.
    Item {
        /// Visible label.
        label: &'static str,
        /// Optional leading `4x4` SSAA SDF glyph (`None` aligns text directly on left inset).
        icon: Option<crate::surfaces::UiGlyphKind>,
        /// Optional right-aligned real keybinding hint.
        shortcut: Option<&'static str>,
        /// Target compositor / WM action.
        action: TopBarMenuAction,
        /// Whether the item is interactive (enabled) or greyed out (disabled) (`RUSTYOS-127`).
        enabled: bool,
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

pub(crate) const MENU_SYSTEM: &[TopBarMenuEntry] = &[
    TopBarMenuEntry::Item {
        label: "About RustyOS",
        icon: Some(crate::surfaces::UiGlyphKind::Info),
        shortcut: None,
        action: TopBarMenuAction::AboutRustyOs,
        enabled: true,
    },
    TopBarMenuEntry::Item {
        label: "System Settings...",
        icon: Some(crate::surfaces::UiGlyphKind::ControlCenter),
        shortcut: None,
        action: TopBarMenuAction::OpenSettings,
        enabled: true,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Suspend",
        icon: None,
        shortcut: None,
        action: TopBarMenuAction::Sleep,
        enabled: true,
    },
    TopBarMenuEntry::Item {
        label: "Restart...",
        icon: None,
        shortcut: None,
        action: TopBarMenuAction::Restart,
        enabled: true,
    },
    TopBarMenuEntry::Item {
        label: "Shut Down...",
        icon: None,
        shortcut: None,
        action: TopBarMenuAction::ShutDown,
        enabled: true,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Lock Screen",
        icon: None,
        shortcut: None,
        action: TopBarMenuAction::LockSession,
        enabled: true,
    },
    TopBarMenuEntry::Item {
        label: "Log Out...",
        icon: None,
        shortcut: None,
        action: TopBarMenuAction::LogOut,
        enabled: true,
    },
];

pub(crate) const MENU_ACTIVE_APP: &[TopBarMenuEntry] = &[
    TopBarMenuEntry::Item {
        label: "Hide Files",
        icon: None,
        shortcut: None,
        action: TopBarMenuAction::HideActiveApp,
        enabled: true,
    },
    TopBarMenuEntry::Item {
        label: "Hide Others",
        icon: None,
        shortcut: None,
        action: TopBarMenuAction::HideOtherApps,
        enabled: true,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Quit",
        icon: None,
        shortcut: None,
        action: TopBarMenuAction::QuitActiveApp,
        enabled: true,
    },
];

pub(crate) const MENU_FILE: &[TopBarMenuEntry] = &[
    TopBarMenuEntry::Item {
        label: "New Files Window",
        icon: Some(crate::surfaces::UiGlyphKind::Folder),
        shortcut: Some("⌘N"),
        action: TopBarMenuAction::OpenFiles,
        enabled: true,
    },
    TopBarMenuEntry::Item {
        label: "New Desktop Folder",
        icon: Some(crate::surfaces::UiGlyphKind::Folder),
        shortcut: Some("⇧⌘N"),
        action: TopBarMenuAction::NewDesktopFolder,
        enabled: true,
    },
    TopBarMenuEntry::Item {
        label: "New Terminal",
        icon: Some(crate::surfaces::UiGlyphKind::TerminalPrompt),
        shortcut: Some("⌥⏎"),
        action: TopBarMenuAction::NewTerminal,
        enabled: true,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Spotlight Search",
        icon: Some(crate::surfaces::UiGlyphKind::Search),
        shortcut: Some("⌘Space"),
        action: TopBarMenuAction::OpenSpotlight,
        enabled: true,
    },
    TopBarMenuEntry::Item {
        label: "Screenshot...",
        icon: None,
        shortcut: Some("⇧⌘4"),
        action: TopBarMenuAction::OpenScreenshotHud,
        enabled: true,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Close Window",
        icon: None,
        shortcut: Some("⌥W"),
        action: TopBarMenuAction::CloseFocused,
        enabled: true,
    },
];

pub(crate) const MENU_EDIT: &[TopBarMenuEntry] = &[
    TopBarMenuEntry::Item {
        label: "Toggle Float / Tile",
        icon: Some(crate::surfaces::UiGlyphKind::LayoutSplit),
        shortcut: Some("⌥L"),
        action: TopBarMenuAction::ToggleLayout,
        enabled: true,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Toggle Reduced Motion",
        icon: None,
        shortcut: Some("⌥R"),
        action: TopBarMenuAction::ToggleReducedMotion,
        enabled: true,
    },
    TopBarMenuEntry::Item {
        label: "Toggle Opaque Panels",
        icon: None,
        shortcut: None,
        action: TopBarMenuAction::ToggleReduceTransparency,
        enabled: true,
    },
];

pub(crate) const MENU_VIEW: &[TopBarMenuEntry] = &[
    TopBarMenuEntry::Item {
        label: "Workspace Overview",
        icon: None,
        shortcut: Some("⌘⇥"),
        action: TopBarMenuAction::OpenOverview,
        enabled: true,
    },
    TopBarMenuEntry::Item {
        label: "Control Center",
        icon: None,
        shortcut: Some("⌥S"),
        action: TopBarMenuAction::OpenControlCenter,
        enabled: true,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Refresh Metrics",
        icon: None,
        shortcut: None,
        action: TopBarMenuAction::RefreshMetrics,
        enabled: true,
    },
];

pub(crate) const MENU_GO: &[TopBarMenuEntry] = &[
    TopBarMenuEntry::Item {
        label: "Recents",
        icon: Some(crate::surfaces::UiGlyphKind::Search),
        shortcut: Some("⇧⌘F"),
        action: TopBarMenuAction::GoRecents,
        enabled: true,
    },
    TopBarMenuEntry::Item {
        label: "Desktop",
        icon: Some(crate::surfaces::UiGlyphKind::Folder),
        shortcut: Some("⇧⌘D"),
        action: TopBarMenuAction::GoDesktop,
        enabled: true,
    },
    TopBarMenuEntry::Item {
        label: "Documents",
        icon: Some(crate::surfaces::UiGlyphKind::Folder),
        shortcut: Some("⇧⌘O"),
        action: TopBarMenuAction::GoDocuments,
        enabled: true,
    },
    TopBarMenuEntry::Item {
        label: "Downloads",
        icon: Some(crate::surfaces::UiGlyphKind::Folder),
        shortcut: Some("⌥⌘L"),
        action: TopBarMenuAction::GoDownloads,
        enabled: true,
    },
    TopBarMenuEntry::Item {
        label: "Home",
        icon: Some(crate::surfaces::UiGlyphKind::Folder),
        shortcut: Some("⇧⌘H"),
        action: TopBarMenuAction::GoHome,
        enabled: true,
    },
    TopBarMenuEntry::Item {
        label: "Trash",
        icon: Some(crate::surfaces::UiGlyphKind::Trash),
        shortcut: Some("⇧⌘⌫"),
        action: TopBarMenuAction::GoTrash,
        enabled: true,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Workspace Overview",
        icon: None,
        shortcut: Some("⌘▲"),
        action: TopBarMenuAction::OpenOverview,
        enabled: true,
    },
    TopBarMenuEntry::Item {
        label: "Next Workspace",
        icon: None,
        shortcut: Some("⌘]"),
        action: TopBarMenuAction::NextWorkspace,
        enabled: true,
    },
    TopBarMenuEntry::Item {
        label: "Previous Workspace",
        icon: None,
        shortcut: Some("⌘["),
        action: TopBarMenuAction::PrevWorkspace,
        enabled: true,
    },
];

pub(crate) const MENU_WINDOW: &[TopBarMenuEntry] = &[
    TopBarMenuEntry::Item {
        label: "Focus Next Window",
        icon: None,
        shortcut: Some("⌥J"),
        action: TopBarMenuAction::FocusNextWindow,
        enabled: true,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Maximize / Restore",
        icon: None,
        shortcut: Some("⌥M"),
        action: TopBarMenuAction::ToggleMaximizeFocused,
        enabled: true,
    },
    TopBarMenuEntry::Item {
        label: "Minimize Window",
        icon: None,
        shortcut: Some("⌥H"),
        action: TopBarMenuAction::MinimizeFocused,
        enabled: true,
    },
    TopBarMenuEntry::Item {
        label: "Tile Window to Left",
        icon: None,
        shortcut: Some("⌘←"),
        action: TopBarMenuAction::TileLeftFocused,
        enabled: true,
    },
    TopBarMenuEntry::Item {
        label: "Tile Window to Right",
        icon: None,
        shortcut: Some("⌘→"),
        action: TopBarMenuAction::TileRightFocused,
        enabled: true,
    },
    TopBarMenuEntry::Item {
        label: "Toggle Float / Tile",
        icon: None,
        shortcut: Some("⌥L"),
        action: TopBarMenuAction::ToggleLayout,
        enabled: true,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Move to Next Workspace",
        icon: None,
        shortcut: None,
        action: TopBarMenuAction::MoveToNextWorkspace,
        enabled: true,
    },
    TopBarMenuEntry::Item {
        label: "Move to Previous Workspace",
        icon: None,
        shortcut: None,
        action: TopBarMenuAction::MoveToPrevWorkspace,
        enabled: true,
    },
    TopBarMenuEntry::Item {
        label: "Move to Next Display",
        icon: None,
        shortcut: None,
        action: TopBarMenuAction::MoveToNextDisplay,
        enabled: true,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Close Window",
        icon: None,
        shortcut: Some("⌥W"),
        action: TopBarMenuAction::CloseFocused,
        enabled: true,
    },
];

pub(crate) const MENU_HELP: &[TopBarMenuEntry] = &[
    TopBarMenuEntry::Item {
        label: "About RustyOS",
        icon: Some(crate::surfaces::UiGlyphKind::Info),
        shortcut: None,
        action: TopBarMenuAction::AboutRustyOs,
        enabled: true,
    },
    TopBarMenuEntry::Item {
        label: "Keyboard Shortcuts",
        icon: Some(crate::surfaces::UiGlyphKind::Shortcuts),
        shortcut: None,
        action: TopBarMenuAction::ShowShortcuts,
        enabled: true,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Refresh Metrics",
        icon: None,
        shortcut: None,
        action: TopBarMenuAction::RefreshMetrics,
        enabled: true,
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
        TopBarMenuHit::Dynamic(_) | TopBarMenuHit::Overflow => &[],
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
            ("System Settings...", TopBarMenuAction::OpenSettings),
            ("Suspend", TopBarMenuAction::Sleep),
            ("Restart...", TopBarMenuAction::Restart),
            ("Shut Down...", TopBarMenuAction::ShutDown),
            ("Lock Screen", TopBarMenuAction::LockSession),
            ("Log Out...", TopBarMenuAction::LogOut),
        ],
        TopBarMenuHit::ActiveApp => &[
            ("Hide", TopBarMenuAction::HideActiveApp),
            ("Hide Others", TopBarMenuAction::HideOtherApps),
            ("Quit", TopBarMenuAction::QuitActiveApp),
        ],
        TopBarMenuHit::File => &[
            ("New Files Window", TopBarMenuAction::OpenFiles),
            ("New Desktop Folder", TopBarMenuAction::NewDesktopFolder),
            ("New Terminal", TopBarMenuAction::NewTerminal),
            ("Spotlight Search", TopBarMenuAction::OpenSpotlight),
            ("Screenshot...", TopBarMenuAction::OpenScreenshotHud),
            ("Close Window", TopBarMenuAction::CloseFocused),
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
            ("Recents", TopBarMenuAction::GoRecents),
            ("Desktop", TopBarMenuAction::GoDesktop),
            ("Documents", TopBarMenuAction::GoDocuments),
            ("Downloads", TopBarMenuAction::GoDownloads),
            ("Home", TopBarMenuAction::GoHome),
            ("Trash", TopBarMenuAction::GoTrash),
            ("Workspace Overview", TopBarMenuAction::OpenOverview),
            ("Next Workspace", TopBarMenuAction::NextWorkspace),
            ("Previous Workspace", TopBarMenuAction::PrevWorkspace),
        ],
        TopBarMenuHit::Window => &[
            ("Focus Next Window", TopBarMenuAction::FocusNextWindow),
            (
                "Maximize / Restore",
                TopBarMenuAction::ToggleMaximizeFocused,
            ),
            ("Minimize Window", TopBarMenuAction::MinimizeFocused),
            ("Tile Window to Left", TopBarMenuAction::TileLeftFocused),
            ("Tile Window to Right", TopBarMenuAction::TileRightFocused),
            ("Toggle Float / Tile", TopBarMenuAction::ToggleLayout),
            (
                "Move to Next Workspace",
                TopBarMenuAction::MoveToNextWorkspace,
            ),
            (
                "Move to Previous Workspace",
                TopBarMenuAction::MoveToPrevWorkspace,
            ),
            ("Move to Next Display", TopBarMenuAction::MoveToNextDisplay),
            ("Close Window", TopBarMenuAction::CloseFocused),
        ],
        TopBarMenuHit::Help => &[
            ("About RustyOS", TopBarMenuAction::AboutRustyOs),
            ("Keyboard Shortcuts", TopBarMenuAction::ShowShortcuts),
            ("Refresh Metrics", TopBarMenuAction::RefreshMetrics),
        ],
        TopBarMenuHit::Dynamic(_) | TopBarMenuHit::Overflow => &[],
    }
}

pub(crate) const MENU_POPOVER_PAD_Y: i32 = rusty_ui::menu::VERTICAL_CONTENT_MARGIN;
pub(crate) const MENU_ITEM_HEIGHT: i32 = rusty_ui::menu::ROW_HEIGHT;
pub(crate) const MENU_SEPARATOR_HEIGHT: i32 = rusty_ui::menu::SEPARATOR_HEIGHT;

/// Computes the glass dropdown popover rectangle anchored beneath `menu` (`RUSTYOS-048`/`051`).
#[must_use]
pub fn top_bar_menu_popover_rect(
    active_app_name: &str,
    menu: TopBarMenuHit,
) -> (i32, i32, i32, i32) {
    let profile = if active_app_name.eq_ignore_ascii_case("desktop") {
        TopBarAppProfile::Desktop
    } else {
        TopBarAppProfile::Files
    };
    top_bar_menu_popover_rect_for_profile(profile, active_app_name, menu)
}

/// Computes the glass dropdown popover rectangle adapted to `profile` and `active_app_name` (`RUSTYOS-068`).
#[must_use]
pub fn top_bar_menu_popover_rect_for_profile(
    profile: TopBarAppProfile,
    active_app_name: &str,
    menu: TopBarMenuHit,
) -> (i32, i32, i32, i32) {
    let left = compute_top_bar_left_layout_for_profile(profile, active_app_name, false);
    let anchor_x = match menu {
        TopBarMenuHit::SystemLogo => TOP_BAR_LOGO_X,
        TopBarMenuHit::ActiveApp => left.app_x,
        TopBarMenuHit::File | TopBarMenuHit::Dynamic(_) => left.file_x,
        TopBarMenuHit::Edit => left.edit_x,
        TopBarMenuHit::View => left.view_x,
        TopBarMenuHit::Go => left.go_x,
        TopBarMenuHit::Window => left.win_x,
        TopBarMenuHit::Help | TopBarMenuHit::Overflow => left.help_x,
    };
    let entries = resolve_top_bar_menu_entries(profile, active_app_name, menu);
    let mut content_h = 0;
    let mut pop_w = 228;
    for entry in &entries {
        match entry {
            ResolvedTopBarMenuItem::Item {
                label,
                icon,
                shortcut,
                ..
            } => {
                content_h += MENU_ITEM_HEIGHT;
                let left_x = if icon.is_some() { 35 } else { 14 };
                let label_w = crate::text::measure_ui_text_width(
                    label,
                    13.0,
                    crate::text::FontWeight::Regular,
                );
                let shortcut_w = shortcut.as_deref().map_or(0, |s| {
                    crate::text::measure_ui_text_width(s, 12.0, crate::text::FontWeight::Regular)
                        + 18
                });
                pop_w = pop_w.max(left_x + label_w + shortcut_w + 18);
            }
            ResolvedTopBarMenuItem::Separator => {
                content_h += MENU_SEPARATOR_HEIGHT;
            }
        }
    }
    let pop_h = MENU_POPOVER_PAD_Y * 2 + content_h;
    (anchor_x, TOP_BAR_HEIGHT + 6, pop_w, pop_h)
}

pub(crate) fn top_bar_menu_entry_at_for_profile(
    profile: TopBarAppProfile,
    active_app_name: &str,
    menu: TopBarMenuHit,
    x: f64,
    y: f64,
) -> Option<(usize, TopBarMenuAction)> {
    let entries = resolve_top_bar_menu_entries(profile, active_app_name, menu);
    if entries.is_empty() {
        return None;
    }
    let (px, py, pw, ph) = top_bar_menu_popover_rect_for_profile(profile, active_app_name, menu);
    let fx = f64::from(px);
    let fy = f64::from(py);
    if x < fx || x >= fx + f64::from(pw) || y < fy || y >= fy + f64::from(ph) {
        return None;
    }
    let mut cursor_y = py + MENU_POPOVER_PAD_Y;
    for (entry_idx, entry) in entries.into_iter().enumerate() {
        match entry {
            ResolvedTopBarMenuItem::Item {
                action, enabled, ..
            } => {
                let top = f64::from(cursor_y);
                let bot = f64::from(cursor_y + MENU_ITEM_HEIGHT);
                if y >= top && y < bot {
                    if enabled {
                        return Some((entry_idx, action));
                    }
                    return None;
                }
                cursor_y += MENU_ITEM_HEIGHT;
            }
            ResolvedTopBarMenuItem::Separator => {
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
    check_top_bar_menu_popover_click_for_profile(
        TopBarAppProfile::Files,
        active_app_name,
        menu,
        x,
        y,
    )
}

/// Profile-aware popover click hit-test for `TopBar` dropdowns (`RUSTYOS-068`).
#[must_use]
pub fn check_top_bar_menu_popover_click_for_profile(
    profile: TopBarAppProfile,
    active_app_name: &str,
    menu: TopBarMenuHit,
    x: f64,
    y: f64,
) -> TopBarMenuPopoverClick {
    if menu == TopBarMenuHit::Go && !profile.has_go_menu() {
        return TopBarMenuPopoverClick::Outside;
    }
    let (px, py, pw, ph) = top_bar_menu_popover_rect_for_profile(profile, active_app_name, menu);
    let fx = f64::from(px);
    let fy = f64::from(py);
    if x < fx || x >= fx + f64::from(pw) || y < fy || y >= fy + f64::from(ph) {
        return TopBarMenuPopoverClick::Outside;
    }
    if let Some((_, action)) =
        top_bar_menu_entry_at_for_profile(profile, active_app_name, menu, x, y)
    {
        TopBarMenuPopoverClick::Action(action)
    } else {
        TopBarMenuPopoverClick::Inside
    }
}

/// Returns the hovered entry index (`0..entries.len()`) inside an open menu popover (`RUSTYOS-051`).
#[must_use]
pub fn check_top_bar_menu_popover_hover_row(
    active_app_name: &str,
    menu: TopBarMenuHit,
    x: f64,
    y: f64,
) -> Option<usize> {
    check_top_bar_menu_popover_hover_row_for_profile(
        TopBarAppProfile::Files,
        active_app_name,
        menu,
        x,
        y,
    )
}

/// Profile-aware hover row lookup for an open `TopBar` dropdown popover (`RUSTYOS-068`).
#[must_use]
pub fn check_top_bar_menu_popover_hover_row_for_profile(
    profile: TopBarAppProfile,
    active_app_name: &str,
    menu: TopBarMenuHit,
    x: f64,
    y: f64,
) -> Option<usize> {
    if menu == TopBarMenuHit::Go && !profile.has_go_menu() {
        return None;
    }
    top_bar_menu_entry_at_for_profile(profile, active_app_name, menu, x, y).map(|(idx, _)| idx)
}

/// Hit-tests `(x, y)` against the `TopBar` Left Menu (`Logo`, `App`, `File`, `Edit`, `View`, `Go`, `Window`, `Help`) (`RUSTYOS-038`/`049`).
#[must_use]
pub fn check_top_bar_menu_click(x: f64, y: f64) -> Option<TopBarMenuHit> {
    check_top_bar_menu_click_for_profile(TopBarAppProfile::Desktop, "Desktop", x, y)
}

/// Active-app-width-aware hit-test for the `TopBar` Left Menu (`RUSTYOS-048`/`049`/`067`).
#[must_use]
pub fn check_top_bar_menu_click_for_app(
    active_app_name: &str,
    x: f64,
    y: f64,
) -> Option<TopBarMenuHit> {
    let profile = if active_app_name.eq_ignore_ascii_case("desktop") {
        TopBarAppProfile::Desktop
    } else {
        TopBarAppProfile::Files
    };
    check_top_bar_menu_click_for_profile(profile, active_app_name, x, y)
}

/// Profile-aware hit-test for `TopBar` Left Menu items (omits `Go` outside `Files`) (`RUSTYOS-068`).
#[must_use]
pub fn check_top_bar_menu_click_for_profile(
    profile: TopBarAppProfile,
    active_app_name: &str,
    x: f64,
    y: f64,
) -> Option<TopBarMenuHit> {
    if y < 0.0 || y > f64::from(TOP_BAR_HEIGHT) {
        return None;
    }
    let left = compute_top_bar_left_layout_for_profile(profile, active_app_name, false);
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
        if hit == TopBarMenuHit::Go && !left.has_go {
            continue;
        }
        if x >= f64::from(bx) && x < f64::from(bx + bw) {
            return Some(hit);
        }
    }
    None
}

/// Computes horizontal `(section_idx, title, x, w)` slots for live D-Bus `AppMenuTree` sections (`RUSTYOS-145`).
#[must_use]
pub fn dynamic_app_menu_slots<'a>(
    left: &super::TopBarLeftLayout,
    tree: &'a crate::app_menu::AppMenuTree,
) -> Vec<(usize, &'a str, i32, i32)> {
    let mut cursor = left.app_x + left.app_w + 10;
    let mut slots = Vec::with_capacity(tree.sections.len());
    for (idx, sec) in tree.sections.iter().enumerate() {
        let w = (crate::text::measure_ui_text_width(
            &sec.title,
            crate::shell_render::TOP_BAR_FONT_SIZE,
            crate::text::FontWeight::Regular,
        ) + 14)
            .clamp(34, 140);
        slots.push((idx, sec.title.as_str(), cursor, w));
        cursor += w + 4;
    }
    slots
}

/// Resolves dropdown entries for either a live D-Bus `AppMenuSection` or static `TopBarAppProfile` (`RUSTYOS-145`).
#[must_use]
pub fn resolve_top_bar_menu_entries_with_app_menu(
    profile: TopBarAppProfile,
    active_app_name: &str,
    menu: TopBarMenuHit,
    app_menu: Option<&crate::app_menu::AppMenuTree>,
) -> Vec<ResolvedTopBarMenuItem> {
    if let (TopBarMenuHit::Dynamic(idx), Some(tree)) = (menu, app_menu)
        && let Some(section) = tree.sections.get(idx)
    {
        return section
            .items
            .iter()
            .map(|item| {
                if item.separator {
                    ResolvedTopBarMenuItem::Separator
                } else {
                    let label = if item.checked == Some(true) {
                        format!("✓ {}", item.label)
                    } else {
                        item.label.clone()
                    };
                    ResolvedTopBarMenuItem::Item {
                        label: label.into(),
                        icon: None,
                        shortcut: item.shortcut.clone().map(std::borrow::Cow::Owned),
                        action: TopBarMenuAction::DbusMenuItem(item.id),
                        enabled: item.enabled,
                    }
                }
            })
            .collect();
    }
    resolve_top_bar_menu_entries(profile, active_app_name, menu)
}

/// Computes popover rectangle for either a live D-Bus `AppMenu` section or static profile menu (`RUSTYOS-145`).
#[must_use]
pub fn top_bar_menu_popover_rect_with_app_menu(
    profile: TopBarAppProfile,
    active_app_name: &str,
    menu: TopBarMenuHit,
    app_menu: Option<&crate::app_menu::AppMenuTree>,
) -> (i32, i32, i32, i32) {
    let left = compute_top_bar_left_layout_for_profile(profile, active_app_name, false);
    let anchor_x = if let (TopBarMenuHit::Dynamic(idx), Some(tree)) = (menu, app_menu) {
        dynamic_app_menu_slots(&left, tree)
            .into_iter()
            .find(|(i, _, _, _)| *i == idx)
            .map_or(left.file_x, |(_, _, x, _)| x)
    } else {
        return top_bar_menu_popover_rect_for_profile(profile, active_app_name, menu);
    };
    let entries =
        resolve_top_bar_menu_entries_with_app_menu(profile, active_app_name, menu, app_menu);
    let mut content_h = 0;
    let mut pop_w = 220;
    for entry in &entries {
        match entry {
            ResolvedTopBarMenuItem::Item {
                label, shortcut, ..
            } => {
                content_h += MENU_ITEM_HEIGHT;
                let label_w = crate::text::measure_ui_text_width(
                    label,
                    13.0,
                    crate::text::FontWeight::Regular,
                );
                let shortcut_w = shortcut.as_deref().map_or(0, |s| {
                    18 + crate::text::measure_ui_text_width(
                        s,
                        12.0,
                        crate::text::FontWeight::Regular,
                    )
                });
                pop_w = pop_w.max(14 + label_w + shortcut_w + 24);
            }
            ResolvedTopBarMenuItem::Separator => {
                content_h += MENU_SEPARATOR_HEIGHT;
            }
        }
    }
    let pop_h = content_h + MENU_POPOVER_PAD_Y * 2;
    (anchor_x.max(4), TOP_BAR_HEIGHT + 2, pop_w, pop_h)
}

/// Visible left Menu Bar slot `(hit, title, x, w)`.
pub type TopBarVisibleMenuSlot<'a> = (TopBarMenuHit, std::borrow::Cow<'a, str>, i32, i32);
/// Collapsed overflow menu entry `(hit, title)` moved under the `"»"` overflow pill.
pub type TopBarOverflowMenuItem<'a> = (TopBarMenuHit, std::borrow::Cow<'a, str>);
/// Partitioned `(visible_slots, overflowed_items)` computed by [`visible_top_bar_menu_slots`].
pub type TopBarMenuSlotLayout<'a> = (
    Vec<TopBarVisibleMenuSlot<'a>>,
    Vec<TopBarOverflowMenuItem<'a>>,
);

/// Computes visible left Menu Bar slots `(hit, title, x, w)` before `max_x` and any
/// overflowing sections collapsed into the `"»"` overflow pill.
#[must_use]
pub fn visible_top_bar_menu_slots<'a>(
    profile: TopBarAppProfile,
    active_app_name: &str,
    app_menu: Option<&'a crate::app_menu::AppMenuTree>,
    max_x: i32,
) -> TopBarMenuSlotLayout<'a> {
    use std::borrow::Cow;

    let left = compute_top_bar_left_layout_for_profile(profile, active_app_name, false);
    let candidates: Vec<(TopBarMenuHit, Cow<'a, str>, i32, i32)> = if let Some(tree) = app_menu {
        dynamic_app_menu_slots(&left, tree)
            .into_iter()
            .map(|(idx, title, x, w)| (TopBarMenuHit::Dynamic(idx), Cow::Borrowed(title), x, w))
            .collect()
    } else {
        let mut list = vec![
            (
                TopBarMenuHit::File,
                Cow::Borrowed("File"),
                left.file_x,
                left.file_w,
            ),
            (
                TopBarMenuHit::Edit,
                Cow::Borrowed("Edit"),
                left.edit_x,
                left.edit_w,
            ),
            (
                TopBarMenuHit::View,
                Cow::Borrowed("View"),
                left.view_x,
                left.view_w,
            ),
        ];
        if left.has_go {
            list.push((TopBarMenuHit::Go, Cow::Borrowed("Go"), left.go_x, left.go_w));
        }
        list.push((
            TopBarMenuHit::Window,
            Cow::Borrowed("Window"),
            left.win_x,
            left.win_w,
        ));
        list.push((
            TopBarMenuHit::Help,
            Cow::Borrowed("Help"),
            left.help_x,
            left.help_w,
        ));
        list
    };

    let mut visible = Vec::with_capacity(candidates.len() + 1);
    let mut overflowed = Vec::new();
    let overflow_btn_w = 28;
    let total_last_right = candidates
        .last()
        .map_or(left.app_x + left.app_w, |(_, _, x, w)| *x + *w);
    let needs_overflow = total_last_right > max_x;
    let cutoff = if needs_overflow {
        max_x - overflow_btn_w - 6
    } else {
        max_x
    };

    for (hit, title, x, w) in candidates {
        if !overflowed.is_empty() || x + w > cutoff {
            overflowed.push((hit, title));
        } else {
            visible.push((hit, title, x, w));
        }
    }
    if !overflowed.is_empty() {
        let ox = visible
            .last()
            .map_or(left.app_x + left.app_w + 10, |(_, _, x, w)| *x + *w + 4);
        if ox + overflow_btn_w <= max_x + 4 {
            visible.push((
                TopBarMenuHit::Overflow,
                Cow::Borrowed("»"),
                ox,
                overflow_btn_w,
            ));
        }
    }
    (visible, overflowed)
}

/// Resolves dropdown entries for any [`TopBarMenuHit`], including [`TopBarMenuHit::Overflow`].
#[must_use]
pub fn resolve_top_bar_menu_entries_for_width(
    profile: TopBarAppProfile,
    active_app_name: &str,
    menu: TopBarMenuHit,
    app_menu: Option<&crate::app_menu::AppMenuTree>,
    max_x: i32,
) -> Vec<ResolvedTopBarMenuItem> {
    if menu != TopBarMenuHit::Overflow {
        return resolve_top_bar_menu_entries_with_app_menu(
            profile,
            active_app_name,
            menu,
            app_menu,
        );
    }
    let (_, overflowed) = visible_top_bar_menu_slots(profile, active_app_name, app_menu, max_x);
    let mut entries = Vec::new();
    for (idx, (hit, title)) in overflowed.into_iter().enumerate() {
        if idx > 0 {
            entries.push(ResolvedTopBarMenuItem::Separator);
        }
        entries.push(ResolvedTopBarMenuItem::Item {
            label: std::borrow::Cow::Owned(title.into_owned()),
            icon: None,
            shortcut: None,
            action: TopBarMenuAction::ShowShortcuts,
            enabled: false,
        });
        entries.extend(resolve_top_bar_menu_entries_with_app_menu(
            profile,
            active_app_name,
            hit,
            app_menu,
        ));
    }
    entries
}

/// Width-aware dropdown popover bounds supporting [`TopBarMenuHit::Overflow`].
#[must_use]
pub fn top_bar_menu_popover_rect_for_width(
    profile: TopBarAppProfile,
    active_app_name: &str,
    menu: TopBarMenuHit,
    app_menu: Option<&crate::app_menu::AppMenuTree>,
    max_x: i32,
) -> (i32, i32, i32, i32) {
    if !matches!(menu, TopBarMenuHit::Dynamic(_) | TopBarMenuHit::Overflow) {
        return top_bar_menu_popover_rect_for_profile(profile, active_app_name, menu);
    }
    let left = compute_top_bar_left_layout_for_profile(profile, active_app_name, false);
    let (visible, _) = visible_top_bar_menu_slots(profile, active_app_name, app_menu, max_x);
    let anchor_x = visible
        .iter()
        .find(|(h, _, _, _)| *h == menu)
        .map_or(left.file_x, |(_, _, x, _)| *x);
    let entries =
        resolve_top_bar_menu_entries_for_width(profile, active_app_name, menu, app_menu, max_x);
    let mut content_h = 0;
    let mut pop_w = 220;
    for entry in &entries {
        match entry {
            ResolvedTopBarMenuItem::Item {
                label, shortcut, ..
            } => {
                content_h += MENU_ITEM_HEIGHT;
                let label_w = crate::text::measure_ui_text_width(
                    label,
                    13.0,
                    crate::text::FontWeight::Regular,
                );
                let shortcut_w = shortcut.as_deref().map_or(0, |s| {
                    18 + crate::text::measure_ui_text_width(
                        s,
                        12.0,
                        crate::text::FontWeight::Regular,
                    )
                });
                pop_w = pop_w.max(14 + label_w + shortcut_w + 24);
            }
            ResolvedTopBarMenuItem::Separator => {
                content_h += MENU_SEPARATOR_HEIGHT;
            }
        }
    }
    let pop_h = content_h + MENU_POPOVER_PAD_Y * 2;
    (anchor_x.max(4), TOP_BAR_HEIGHT + 2, pop_w, pop_h)
}

/// Width-aware `TopBar` left header hit-test supporting both live `AppMenuTree` and `Overflow`.
#[must_use]
pub fn check_top_bar_menu_click_for_width(
    profile: TopBarAppProfile,
    active_app_name: &str,
    app_menu: Option<&crate::app_menu::AppMenuTree>,
    max_x: i32,
    x: f64,
    y: f64,
) -> Option<TopBarMenuHit> {
    if y < 0.0 || y > f64::from(TOP_BAR_HEIGHT) {
        return None;
    }
    let left = compute_top_bar_left_layout_for_profile(profile, active_app_name, false);
    if x >= f64::from(TOP_BAR_LOGO_X) && x < f64::from(TOP_BAR_LOGO_X + TOP_BAR_LOGO_W) {
        return Some(TopBarMenuHit::SystemLogo);
    }
    if x >= f64::from(left.app_x) && x < f64::from(left.app_x + left.app_w) {
        return Some(TopBarMenuHit::ActiveApp);
    }
    let (visible, _) = visible_top_bar_menu_slots(profile, active_app_name, app_menu, max_x);
    for (hit, _, bx, bw) in visible {
        if x >= f64::from(bx) && x < f64::from(bx + bw) {
            return Some(hit);
        }
    }
    None
}

/// Width-aware popover click hit-test supporting static, dynamic, and overflow menus.
#[must_use]
pub fn check_top_bar_menu_popover_click_for_width(
    profile: TopBarAppProfile,
    active_app_name: &str,
    menu: TopBarMenuHit,
    app_menu: Option<&crate::app_menu::AppMenuTree>,
    max_x: i32,
    x: f64,
    y: f64,
) -> TopBarMenuPopoverClick {
    let (px, py, pw, ph) =
        top_bar_menu_popover_rect_for_width(profile, active_app_name, menu, app_menu, max_x);
    if x < f64::from(px) || x >= f64::from(px + pw) || y < f64::from(py) || y >= f64::from(py + ph)
    {
        return TopBarMenuPopoverClick::Outside;
    }
    let mut row_y = py + MENU_POPOVER_PAD_Y;
    for entry in
        resolve_top_bar_menu_entries_for_width(profile, active_app_name, menu, app_menu, max_x)
    {
        match entry {
            ResolvedTopBarMenuItem::Item {
                action, enabled, ..
            } => {
                if y >= f64::from(row_y) && y < f64::from(row_y + MENU_ITEM_HEIGHT) {
                    return if enabled {
                        TopBarMenuPopoverClick::Action(action)
                    } else {
                        TopBarMenuPopoverClick::Inside
                    };
                }
                row_y += MENU_ITEM_HEIGHT;
            }
            ResolvedTopBarMenuItem::Separator => {
                row_y += MENU_SEPARATOR_HEIGHT;
            }
        }
    }
    TopBarMenuPopoverClick::Inside
}

/// Computes the hovered entry index (`0..entries.len()`) across static, live `AppMenu`, and `Overflow` menus.
#[must_use]
pub fn check_top_bar_menu_popover_hover_row_for_width(
    profile: TopBarAppProfile,
    active_app_name: &str,
    menu: TopBarMenuHit,
    app_menu: Option<&crate::app_menu::AppMenuTree>,
    max_x: i32,
    x: f64,
    y: f64,
) -> Option<usize> {
    if menu == TopBarMenuHit::Go && !profile.has_go_menu() {
        return None;
    }
    let (px, py, pw, ph) =
        top_bar_menu_popover_rect_for_width(profile, active_app_name, menu, app_menu, max_x);
    if x < f64::from(px) || x >= f64::from(px + pw) || y < f64::from(py) || y >= f64::from(py + ph)
    {
        return None;
    }
    let mut row_y = py + MENU_POPOVER_PAD_Y;
    for (idx, entry) in
        resolve_top_bar_menu_entries_for_width(profile, active_app_name, menu, app_menu, max_x)
            .into_iter()
            .enumerate()
    {
        match entry {
            ResolvedTopBarMenuItem::Item { enabled, .. } => {
                if y >= f64::from(row_y) && y < f64::from(row_y + MENU_ITEM_HEIGHT) {
                    return enabled.then_some(idx);
                }
                row_y += MENU_ITEM_HEIGHT;
            }
            ResolvedTopBarMenuItem::Separator => {
                row_y += MENU_SEPARATOR_HEIGHT;
            }
        }
    }
    None
}

/// Hit-tests `TopBar` left menu headers using live `AppMenuTree` when present (`RUSTYOS-145`).
#[must_use]
pub fn check_top_bar_menu_click_with_app_menu(
    profile: TopBarAppProfile,
    active_app_name: &str,
    app_menu: Option<&crate::app_menu::AppMenuTree>,
    x: f64,
    y: f64,
) -> Option<TopBarMenuHit> {
    if y < 0.0 || y > f64::from(TOP_BAR_HEIGHT) {
        return None;
    }
    let Some(tree) = app_menu else {
        return check_top_bar_menu_click_for_profile(profile, active_app_name, x, y);
    };
    let left = compute_top_bar_left_layout_for_profile(profile, active_app_name, false);
    if x >= f64::from(TOP_BAR_LOGO_X) && x < f64::from(TOP_BAR_LOGO_X + TOP_BAR_LOGO_W) {
        return Some(TopBarMenuHit::SystemLogo);
    }
    if x >= f64::from(left.app_x) && x < f64::from(left.app_x + left.app_w) {
        return Some(TopBarMenuHit::ActiveApp);
    }
    for (idx, _, bx, bw) in dynamic_app_menu_slots(&left, tree) {
        if x >= f64::from(bx) && x < f64::from(bx + bw) {
            return Some(TopBarMenuHit::Dynamic(idx));
        }
    }
    None
}

/// Hit-tests an open `TopBar` dropdown popover using live `AppMenuTree` when present (`RUSTYOS-145`).
#[must_use]
pub fn check_top_bar_menu_popover_click_with_app_menu(
    profile: TopBarAppProfile,
    active_app_name: &str,
    menu: TopBarMenuHit,
    app_menu: Option<&crate::app_menu::AppMenuTree>,
    x: f64,
    y: f64,
) -> TopBarMenuPopoverClick {
    if !matches!(menu, TopBarMenuHit::Dynamic(_)) || app_menu.is_none() {
        return check_top_bar_menu_popover_click_for_profile(profile, active_app_name, menu, x, y);
    }
    let (px, py, pw, ph) =
        top_bar_menu_popover_rect_with_app_menu(profile, active_app_name, menu, app_menu);
    if x < f64::from(px) || x >= f64::from(px + pw) || y < f64::from(py) || y >= f64::from(py + ph)
    {
        return TopBarMenuPopoverClick::Outside;
    }
    let mut row_y = py + MENU_POPOVER_PAD_Y;
    for entry in
        resolve_top_bar_menu_entries_with_app_menu(profile, active_app_name, menu, app_menu)
    {
        match entry {
            ResolvedTopBarMenuItem::Item {
                action, enabled, ..
            } => {
                if y >= f64::from(row_y) && y < f64::from(row_y + MENU_ITEM_HEIGHT) {
                    return if enabled {
                        TopBarMenuPopoverClick::Action(action)
                    } else {
                        TopBarMenuPopoverClick::Inside
                    };
                }
                row_y += MENU_ITEM_HEIGHT;
            }
            ResolvedTopBarMenuItem::Separator => {
                row_y += MENU_SEPARATOR_HEIGHT;
            }
        }
    }
    TopBarMenuPopoverClick::Inside
}

#[cfg(test)]
mod dynamic_app_menu_tests {
    use super::*;
    use crate::app_menu::{AppMenuItem, AppMenuSection, AppMenuTree};

    #[test]
    fn dynamic_app_menu_slots_and_clicks_resolve_dbus_item_id() {
        let tree = AppMenuTree {
            service_name: ":1.99".to_owned(),
            object_path: "/MenuBar/1".to_owned(),
            sections: vec![AppMenuSection {
                id: 1,
                title: "Document".to_owned(),
                items: vec![
                    AppMenuItem {
                        id: 42,
                        label: "Save".to_owned(),
                        shortcut: Some("Ctrl+S".to_owned()),
                        enabled: true,
                        separator: false,
                        checked: Some(true),
                    },
                    AppMenuItem {
                        id: 43,
                        label: String::new(),
                        shortcut: None,
                        enabled: true,
                        separator: true,
                        checked: None,
                    },
                ],
            }],
        };
        let left =
            compute_top_bar_left_layout_for_profile(TopBarAppProfile::Files, "Editor", false);
        let slots = dynamic_app_menu_slots(&left, &tree);
        assert_eq!(slots.len(), 1);
        let (_, title, sx, sw) = slots[0];
        assert_eq!(title, "Document");
        assert!(sw >= 34);

        let hit = check_top_bar_menu_click_with_app_menu(
            TopBarAppProfile::Files,
            "Editor",
            Some(&tree),
            f64::from(sx + 6),
            12.0,
        );
        assert_eq!(hit, Some(TopBarMenuHit::Dynamic(0)));

        let (px, py, _, _) = top_bar_menu_popover_rect_with_app_menu(
            TopBarAppProfile::Files,
            "Editor",
            TopBarMenuHit::Dynamic(0),
            Some(&tree),
        );
        let click = check_top_bar_menu_popover_click_with_app_menu(
            TopBarAppProfile::Files,
            "Editor",
            TopBarMenuHit::Dynamic(0),
            Some(&tree),
            f64::from(px + 20),
            f64::from(py + MENU_POPOVER_PAD_Y + 8),
        );
        assert_eq!(
            click,
            TopBarMenuPopoverClick::Action(TopBarMenuAction::DbusMenuItem(42))
        );
    }
}
