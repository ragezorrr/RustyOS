//! `TopBar` left-side application and global menus, items, popovers, and click handlers (`RUSTYOS-090`).

use crate::shell_render::tokens::{TOP_BAR_HEIGHT, TOP_BAR_LOGO_W, TOP_BAR_LOGO_X};
use crate::shell_render::top_bar_menus::{
    AppShortcutAction, ResolvedTopBarMenuItem, TopBarAppProfile, resolve_top_bar_menu_entries,
};

use super::panel::compute_top_bar_left_layout_for_profile;

/// Corner radius for concentric Menu Bar and context menu popovers (`DESIGN.md` §12.3, ADR-0058).
pub const MENU_POPOVER_RADIUS: i32 = 13;

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
    /// Open built-in System Settings overlay (`Super+,`) (`RUSTYOS-061`).
    OpenSettings,
    /// Open Software Updates popover and trigger an async check (`RUSTYOS-076`).
    CheckForUpdates,
    /// Reload `~/.config/rustyos/config.toml` immediately (`Super+Shift+R`) (`RUSTYOS-061`).
    ReloadConfig,
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
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "System Settings...",
        icon: Some(crate::surfaces::UiGlyphKind::ControlCenter),
        shortcut: Some("⌘,"),
        action: TopBarMenuAction::OpenSettings,
    },
    TopBarMenuEntry::Item {
        label: "Check for Updates...",
        icon: Some(crate::surfaces::UiGlyphKind::Updates { available: false }),
        shortcut: None,
        action: TopBarMenuAction::CheckForUpdates,
    },
    TopBarMenuEntry::Item {
        label: "Reload Config",
        icon: Some(crate::surfaces::UiGlyphKind::Refresh),
        shortcut: Some("⇧⌘R"),
        action: TopBarMenuAction::ReloadConfig,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Workspace Overview",
        icon: None,
        shortcut: Some("⌘⇥"),
        action: TopBarMenuAction::OpenOverview,
    },
    TopBarMenuEntry::Item {
        label: "Control Center",
        icon: None,
        shortcut: Some("⌥S"),
        action: TopBarMenuAction::OpenControlCenter,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Lock Screen",
        icon: None,
        shortcut: Some("⌘L"),
        action: TopBarMenuAction::LockSession,
    },
    TopBarMenuEntry::Item {
        label: "Sleep",
        icon: None,
        shortcut: None,
        action: TopBarMenuAction::Sleep,
    },
    TopBarMenuEntry::Item {
        label: "Restart...",
        icon: None,
        shortcut: None,
        action: TopBarMenuAction::Restart,
    },
    TopBarMenuEntry::Item {
        label: "Shut Down...",
        icon: None,
        shortcut: None,
        action: TopBarMenuAction::ShutDown,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Log Out...",
        icon: None,
        shortcut: Some("⇧⌘Q"),
        action: TopBarMenuAction::LogOut,
    },
];

pub(crate) const MENU_ACTIVE_APP: &[TopBarMenuEntry] = &[
    TopBarMenuEntry::Item {
        label: "About Files",
        icon: None,
        shortcut: None,
        action: TopBarMenuAction::AboutRustyOs,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "System Settings...",
        icon: Some(crate::surfaces::UiGlyphKind::ControlCenter),
        shortcut: Some("⌘,"),
        action: TopBarMenuAction::OpenSettings,
    },
    TopBarMenuEntry::Item {
        label: "Empty Trash...",
        icon: Some(crate::surfaces::UiGlyphKind::Trash),
        shortcut: Some("⇧⌘⌫"),
        action: TopBarMenuAction::EmptyTrash,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Minimize Window",
        icon: None,
        shortcut: Some("⌥H"),
        action: TopBarMenuAction::MinimizeFocused,
    },
    TopBarMenuEntry::Item {
        label: "Close Window",
        icon: None,
        shortcut: Some("⌥W"),
        action: TopBarMenuAction::CloseFocused,
    },
];

pub(crate) const MENU_FILE: &[TopBarMenuEntry] = &[
    TopBarMenuEntry::Item {
        label: "New Files Window",
        icon: Some(crate::surfaces::UiGlyphKind::Folder),
        shortcut: Some("⌘N"),
        action: TopBarMenuAction::OpenFiles,
    },
    TopBarMenuEntry::Item {
        label: "New Desktop Folder",
        icon: Some(crate::surfaces::UiGlyphKind::Folder),
        shortcut: Some("⇧⌘N"),
        action: TopBarMenuAction::NewDesktopFolder,
    },
    TopBarMenuEntry::Item {
        label: "New Terminal",
        icon: Some(crate::surfaces::UiGlyphKind::TerminalPrompt),
        shortcut: Some("⌥⏎"),
        action: TopBarMenuAction::NewTerminal,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Spotlight Search",
        icon: Some(crate::surfaces::UiGlyphKind::Search),
        shortcut: Some("⌘Space"),
        action: TopBarMenuAction::OpenSpotlight,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Close Window",
        icon: None,
        shortcut: Some("⌥W"),
        action: TopBarMenuAction::CloseFocused,
    },
];

pub(crate) const MENU_EDIT: &[TopBarMenuEntry] = &[
    TopBarMenuEntry::Item {
        label: "Toggle Float / Tile",
        icon: Some(crate::surfaces::UiGlyphKind::LayoutSplit),
        shortcut: Some("⌥L"),
        action: TopBarMenuAction::ToggleLayout,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Toggle Reduced Motion",
        icon: None,
        shortcut: Some("⌥R"),
        action: TopBarMenuAction::ToggleReducedMotion,
    },
    TopBarMenuEntry::Item {
        label: "Toggle Opaque Panels",
        icon: None,
        shortcut: None,
        action: TopBarMenuAction::ToggleReduceTransparency,
    },
];

pub(crate) const MENU_VIEW: &[TopBarMenuEntry] = &[
    TopBarMenuEntry::Item {
        label: "Workspace Overview",
        icon: None,
        shortcut: Some("⌘⇥"),
        action: TopBarMenuAction::OpenOverview,
    },
    TopBarMenuEntry::Item {
        label: "Control Center",
        icon: None,
        shortcut: Some("⌥S"),
        action: TopBarMenuAction::OpenControlCenter,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Refresh Metrics",
        icon: None,
        shortcut: None,
        action: TopBarMenuAction::RefreshMetrics,
    },
];

pub(crate) const MENU_GO: &[TopBarMenuEntry] = &[
    TopBarMenuEntry::Item {
        label: "Recents",
        icon: Some(crate::surfaces::UiGlyphKind::Search),
        shortcut: Some("⇧⌘F"),
        action: TopBarMenuAction::GoRecents,
    },
    TopBarMenuEntry::Item {
        label: "Desktop",
        icon: Some(crate::surfaces::UiGlyphKind::Folder),
        shortcut: Some("⇧⌘D"),
        action: TopBarMenuAction::GoDesktop,
    },
    TopBarMenuEntry::Item {
        label: "Documents",
        icon: Some(crate::surfaces::UiGlyphKind::Folder),
        shortcut: Some("⇧⌘O"),
        action: TopBarMenuAction::GoDocuments,
    },
    TopBarMenuEntry::Item {
        label: "Downloads",
        icon: Some(crate::surfaces::UiGlyphKind::Folder),
        shortcut: Some("⌥⌘L"),
        action: TopBarMenuAction::GoDownloads,
    },
    TopBarMenuEntry::Item {
        label: "Home",
        icon: Some(crate::surfaces::UiGlyphKind::Folder),
        shortcut: Some("⇧⌘H"),
        action: TopBarMenuAction::GoHome,
    },
    TopBarMenuEntry::Item {
        label: "Trash",
        icon: Some(crate::surfaces::UiGlyphKind::Trash),
        shortcut: Some("⇧⌘⌫"),
        action: TopBarMenuAction::GoTrash,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Workspace Overview",
        icon: None,
        shortcut: Some("⌘▲"),
        action: TopBarMenuAction::OpenOverview,
    },
    TopBarMenuEntry::Item {
        label: "Next Workspace",
        icon: None,
        shortcut: Some("⌘]"),
        action: TopBarMenuAction::NextWorkspace,
    },
    TopBarMenuEntry::Item {
        label: "Previous Workspace",
        icon: None,
        shortcut: Some("⌘["),
        action: TopBarMenuAction::PrevWorkspace,
    },
];

pub(crate) const MENU_WINDOW: &[TopBarMenuEntry] = &[
    TopBarMenuEntry::Item {
        label: "Focus Next Window",
        icon: None,
        shortcut: Some("⌥J"),
        action: TopBarMenuAction::FocusNextWindow,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Maximize / Restore",
        icon: None,
        shortcut: Some("⌥M"),
        action: TopBarMenuAction::ToggleMaximizeFocused,
    },
    TopBarMenuEntry::Item {
        label: "Minimize Window",
        icon: None,
        shortcut: Some("⌥H"),
        action: TopBarMenuAction::MinimizeFocused,
    },
    TopBarMenuEntry::Item {
        label: "Toggle Float / Tile",
        icon: None,
        shortcut: Some("⌥L"),
        action: TopBarMenuAction::ToggleLayout,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Close Window",
        icon: None,
        shortcut: Some("⌥W"),
        action: TopBarMenuAction::CloseFocused,
    },
];

pub(crate) const MENU_HELP: &[TopBarMenuEntry] = &[
    TopBarMenuEntry::Item {
        label: "About RustyOS",
        icon: Some(crate::surfaces::UiGlyphKind::Info),
        shortcut: None,
        action: TopBarMenuAction::AboutRustyOs,
    },
    TopBarMenuEntry::Item {
        label: "Keyboard Shortcuts",
        icon: Some(crate::surfaces::UiGlyphKind::Shortcuts),
        shortcut: None,
        action: TopBarMenuAction::ShowShortcuts,
    },
    TopBarMenuEntry::Separator,
    TopBarMenuEntry::Item {
        label: "Refresh Metrics",
        icon: None,
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
            ("System Settings...", TopBarMenuAction::OpenSettings),
            ("Check for Updates...", TopBarMenuAction::CheckForUpdates),
            ("Reload Config", TopBarMenuAction::ReloadConfig),
            ("Workspace Overview", TopBarMenuAction::OpenOverview),
            ("Control Center", TopBarMenuAction::OpenControlCenter),
            ("Lock Screen", TopBarMenuAction::LockSession),
            ("Sleep", TopBarMenuAction::Sleep),
            ("Restart...", TopBarMenuAction::Restart),
            ("Shut Down...", TopBarMenuAction::ShutDown),
            ("Log Out...", TopBarMenuAction::LogOut),
        ],
        TopBarMenuHit::ActiveApp => &[
            ("About Files", TopBarMenuAction::AboutRustyOs),
            ("System Settings...", TopBarMenuAction::OpenSettings),
            ("Empty Trash...", TopBarMenuAction::EmptyTrash),
            ("Minimize Window", TopBarMenuAction::MinimizeFocused),
            ("Close Window", TopBarMenuAction::CloseFocused),
        ],
        TopBarMenuHit::File => &[
            ("New Files Window", TopBarMenuAction::OpenFiles),
            ("New Desktop Folder", TopBarMenuAction::NewDesktopFolder),
            ("New Terminal", TopBarMenuAction::NewTerminal),
            ("Spotlight Search", TopBarMenuAction::OpenSpotlight),
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

pub(crate) const MENU_POPOVER_PAD_Y: i32 = 5;
pub(crate) const MENU_ITEM_HEIGHT: i32 = 24;
pub(crate) const MENU_SEPARATOR_HEIGHT: i32 = 9;

/// Computes the glass dropdown popover rectangle anchored beneath `menu` (`RUSTYOS-048`/`051`).
#[must_use]
pub fn top_bar_menu_popover_rect(
    active_app_name: &str,
    menu: TopBarMenuHit,
) -> (i32, i32, i32, i32) {
    top_bar_menu_popover_rect_for_profile(TopBarAppProfile::Files, active_app_name, menu)
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
        TopBarMenuHit::File => left.file_x,
        TopBarMenuHit::Edit => left.edit_x,
        TopBarMenuHit::View => left.view_x,
        TopBarMenuHit::Go => left.go_x,
        TopBarMenuHit::Window => left.win_x,
        TopBarMenuHit::Help => left.help_x,
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
                let shortcut_w = shortcut.map_or(0, |s| {
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
            ResolvedTopBarMenuItem::Item { action, .. } => {
                let top = f64::from(cursor_y);
                let bot = f64::from(cursor_y + MENU_ITEM_HEIGHT);
                if y >= top && y < bot {
                    return Some((entry_idx, action));
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
    check_top_bar_menu_click_for_profile(TopBarAppProfile::Files, "Files", x, y)
}

/// Active-app-width-aware hit-test for the `TopBar` Left Menu (`RUSTYOS-048`/`049`/`067`).
#[must_use]
pub fn check_top_bar_menu_click_for_app(
    active_app_name: &str,
    x: f64,
    y: f64,
) -> Option<TopBarMenuHit> {
    check_top_bar_menu_click_for_profile(TopBarAppProfile::Files, active_app_name, x, y)
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
