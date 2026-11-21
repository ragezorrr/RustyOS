//! Application profile classification and dynamic `TopBar` dropdown menu definitions (`RUSTYOS-068`, ADR-0033).

use std::borrow::Cow;

use super::dock::extract_app_name;
use super::top_bar::{TopBarMenuAction, TopBarMenuEntry, TopBarMenuHit, top_bar_menu_entries};
use crate::surfaces::UiGlyphKind;

/// Functional application category driving `TopBar` menu categories and shortcuts (`RUSTYOS-068`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TopBarAppProfile {
    /// Desktop workspace mode when no client window is focused.
    #[default]
    Desktop,
    /// Built-in `rusty-files` file manager window (`Go` menu visible).
    Files,
    /// Web browsers (`Firefox`, `Chromium`, `Chrome`, `Brave`, `Vivaldi`, `Epiphany`, `Falkon`, `LibreWolf`, `Zen`).
    Browser,
    /// Terminal emulators (`Konsole`, `Foot`, `Kitty`, `Alacritty`, `WezTerm`, `Ghostty`, `GNOME Terminal`, etc.).
    Terminal,
    /// Code & text editors (`VS Code`, `VSCodium`, `Kate`, `Gedit`, `Zed`, `Mousepad`, `Obsidian`, etc.).
    Editor,
    /// Fallback profile for other graphical applications (`File`, `Edit`, `View`, `Window`, `Help`).
    GenericApp,
}

impl TopBarAppProfile {
    /// Returns whether the top-level `Go` category is visible for this application profile.
    #[must_use]
    pub const fn has_go_menu(self) -> bool {
        matches!(self, Self::Files)
    }
}

/// Application-level keyboard shortcut synthesized by `TopBar` into the focused client (`RUSTYOS-068`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppShortcutAction {
    NewTab,
    NewTabShift,
    NewWindow,
    OpenLocation,
    OpenFile,
    SaveFile,
    CloseTab,
    CloseTabShift,
    Undo,
    Redo,
    Cut,
    Copy,
    CopyShift,
    Paste,
    PasteShift,
    SelectAll,
    Find,
    FindShift,
    ReloadPage,
    ZoomIn,
    ZoomOut,
    ZoomReset,
}

/// Profile-resolved `TopBar` dropdown entry with owned or static label text (`RUSTYOS-068`, `RUSTYOS-107`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedTopBarMenuItem {
    Item {
        label: Cow<'static, str>,
        icon: Option<UiGlyphKind>,
        shortcut: Option<&'static str>,
        action: TopBarMenuAction,
        enabled: bool,
    },
    Separator,
}

impl From<&TopBarMenuEntry> for ResolvedTopBarMenuItem {
    fn from(entry: &TopBarMenuEntry) -> Self {
        match entry {
            TopBarMenuEntry::Item {
                label,
                icon,
                shortcut,
                action,
                enabled,
            } => Self::Item {
                label: Cow::Borrowed(label),
                icon: *icon,
                shortcut: *shortcut,
                action: *action,
                enabled: *enabled,
            },
            TopBarMenuEntry::Separator => Self::Separator,
        }
    }
}

struct AppRule {
    tokens: &'static [&'static str],
    title_hints: &'static [&'static str],
    display_name: &'static str,
    profile: TopBarAppProfile,
}

const APP_RULES: &[AppRule] = &[
    AppRule {
        tokens: &["files", "rusty-files", "org.rustyos.files", "rustyos.files"],
        title_hints: &[],
        display_name: "Files",
        profile: TopBarAppProfile::Files,
    },
    AppRule {
        tokens: &[
            "firefox",
            "firefox-esr",
            "firefox-nightly",
            "firefox-developer-edition",
            "navigator",
            "org.mozilla.firefox",
        ],
        title_hints: &["mozilla firefox"],
        display_name: "Firefox",
        profile: TopBarAppProfile::Browser,
    },
    AppRule {
        tokens: &["zen", "zen-browser", "app.zen_browser.zen"],
        title_hints: &["zen browser"],
        display_name: "Zen",
        profile: TopBarAppProfile::Browser,
    },
    AppRule {
        tokens: &["librewolf", "io.gitlab.librewolf-community"],
        title_hints: &["librewolf"],
        display_name: "LibreWolf",
        profile: TopBarAppProfile::Browser,
    },
    AppRule {
        tokens: &["chromium", "chromium-browser", "org.chromium.chromium"],
        title_hints: &["chromium"],
        display_name: "Chromium",
        profile: TopBarAppProfile::Browser,
    },
    AppRule {
        tokens: &["google-chrome", "chrome", "com.google.chrome"],
        title_hints: &["google chrome"],
        display_name: "Chrome",
        profile: TopBarAppProfile::Browser,
    },
    AppRule {
        tokens: &["brave", "brave-browser", "com.brave.browser"],
        title_hints: &["brave browser", "— brave", "- brave"],
        display_name: "Brave",
        profile: TopBarAppProfile::Browser,
    },
    AppRule {
        tokens: &["vivaldi", "vivaldi-stable", "com.vivaldi.vivaldi"],
        title_hints: &["vivaldi"],
        display_name: "Vivaldi",
        profile: TopBarAppProfile::Browser,
    },
    AppRule {
        tokens: &["epiphany", "org.gnome.epiphany"],
        title_hints: &[],
        display_name: "Epiphany",
        profile: TopBarAppProfile::Browser,
    },
    AppRule {
        tokens: &["falkon", "org.kde.falkon"],
        title_hints: &["falkon"],
        display_name: "Falkon",
        profile: TopBarAppProfile::Browser,
    },
    AppRule {
        tokens: &["konsole", "org.kde.konsole"],
        title_hints: &["konsole"],
        display_name: "Konsole",
        profile: TopBarAppProfile::Terminal,
    },
    AppRule {
        tokens: &["foot", "footclient"],
        title_hints: &[],
        display_name: "Foot",
        profile: TopBarAppProfile::Terminal,
    },
    AppRule {
        tokens: &["kitty", "net.kovidgoyal.kitty"],
        title_hints: &["— kitty", "- kitty"],
        display_name: "Kitty",
        profile: TopBarAppProfile::Terminal,
    },
    AppRule {
        tokens: &["alacritty", "org.alacritty"],
        title_hints: &["alacritty"],
        display_name: "Alacritty",
        profile: TopBarAppProfile::Terminal,
    },
    AppRule {
        tokens: &["wezterm", "org.wezfurlong.wezterm", "wezterm-gui"],
        title_hints: &["wezterm"],
        display_name: "WezTerm",
        profile: TopBarAppProfile::Terminal,
    },
    AppRule {
        tokens: &["ghostty", "com.mitchellh.ghostty"],
        title_hints: &["ghostty"],
        display_name: "Ghostty",
        profile: TopBarAppProfile::Terminal,
    },
    AppRule {
        tokens: &[
            "gnome-terminal",
            "gnome-terminal-server",
            "org.gnome.terminal",
        ],
        title_hints: &["gnome terminal"],
        display_name: "GNOME Terminal",
        profile: TopBarAppProfile::Terminal,
    },
    AppRule {
        tokens: &["xfce4-terminal"],
        title_hints: &["xfce4-terminal", "xfce terminal"],
        display_name: "Xfce4 Terminal",
        profile: TopBarAppProfile::Terminal,
    },
    AppRule {
        tokens: &["weston-terminal"],
        title_hints: &["weston terminal"],
        display_name: "Weston Terminal",
        profile: TopBarAppProfile::Terminal,
    },
    AppRule {
        tokens: &["rusty-terminal", "org.rustyos.terminal"],
        title_hints: &[],
        display_name: "Terminal",
        profile: TopBarAppProfile::Terminal,
    },
    AppRule {
        tokens: &[
            "code",
            "code-oss",
            "code-url-handler",
            "com.visualstudio.code",
        ],
        title_hints: &["visual studio code"],
        display_name: "VS Code",
        profile: TopBarAppProfile::Editor,
    },
    AppRule {
        tokens: &["codium", "vscodium", "com.vscodium.codium"],
        title_hints: &["vscodium"],
        display_name: "VSCodium",
        profile: TopBarAppProfile::Editor,
    },
    AppRule {
        tokens: &["kate", "org.kde.kate"],
        title_hints: &["— kate", "- kate"],
        display_name: "Kate",
        profile: TopBarAppProfile::Editor,
    },
    AppRule {
        tokens: &["gedit", "org.gnome.gedit"],
        title_hints: &["gedit"],
        display_name: "Gedit",
        profile: TopBarAppProfile::Editor,
    },
    AppRule {
        tokens: &["gnome-text-editor", "org.gnome.texteditor"],
        title_hints: &["text editor"],
        display_name: "Text Editor",
        profile: TopBarAppProfile::Editor,
    },
    AppRule {
        tokens: &["zed", "dev.zed.zed"],
        title_hints: &["— zed", "- zed"],
        display_name: "Zed",
        profile: TopBarAppProfile::Editor,
    },
    AppRule {
        tokens: &["mousepad", "org.xfce.mousepad"],
        title_hints: &["mousepad"],
        display_name: "Mousepad",
        profile: TopBarAppProfile::Editor,
    },
    AppRule {
        tokens: &["featherpad"],
        title_hints: &["featherpad"],
        display_name: "FeatherPad",
        profile: TopBarAppProfile::Editor,
    },
    AppRule {
        tokens: &["obsidian"],
        title_hints: &["obsidian"],
        display_name: "Obsidian",
        profile: TopBarAppProfile::Editor,
    },
];

/// Classifies the active window into a canonical display name and [`TopBarAppProfile`] (`RUSTYOS-068`).
#[must_use]
pub fn classify_top_bar_app(app_id: Option<&str>, title: &str) -> (String, TopBarAppProfile) {
    let raw_id = app_id.unwrap_or("").trim();
    let id_lower = raw_id.to_ascii_lowercase();
    let last_seg = id_lower.rsplit('.').next().unwrap_or(id_lower.as_str());
    let title_trimmed = title.trim();
    let title_lower = title_trimmed.to_ascii_lowercase();

    if id_lower == "org.rustyos.desktop" || id_lower == "desktop" || title_lower == "desktop" {
        return ("Desktop".to_owned(), TopBarAppProfile::Desktop);
    }

    if id_lower.is_empty() && title_trimmed.is_empty() {
        return ("Desktop".to_owned(), TopBarAppProfile::Desktop);
    }

    if id_lower == "org.rustyos.files"
        || id_lower == "rusty-files"
        || title_lower == "files"
        || title_lower.starts_with("files —")
        || title_lower.starts_with("files -")
        || (id_lower.is_empty() && title_lower == "rusty-files")
    {
        return ("Files".to_owned(), TopBarAppProfile::Files);
    }

    for rule in APP_RULES {
        let id_match = !id_lower.is_empty()
            && rule
                .tokens
                .iter()
                .any(|tok| id_lower == *tok || last_seg == *tok);
        let title_match = !title_lower.is_empty()
            && rule
                .title_hints
                .iter()
                .any(|hint| title_lower.contains(hint));
        if id_match || title_match {
            return (rule.display_name.to_owned(), rule.profile);
        }
    }

    if id_lower == "org.rustyos.settings" || id_lower == "rusty-settings" {
        return ("Settings".to_owned(), TopBarAppProfile::GenericApp);
    }
    if id_lower == "org.rustyos.updater" || id_lower == "rusty-updater" {
        return ("Software Update".to_owned(), TopBarAppProfile::GenericApp);
    }

    let fallback = if !title_trimmed.is_empty() && !title_trimmed.starts_with("Window #") {
        extract_app_name(title_trimmed)
    } else if !last_seg.is_empty() {
        title_case_segment(last_seg)
    } else {
        "App".to_owned()
    };
    (fallback, TopBarAppProfile::GenericApp)
}

fn title_case_segment(seg: &str) -> String {
    let clean = seg.trim_start_matches("rusty-").replace(['-', '_'], " ");
    let mut out = String::new();
    for word in clean.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    if out.is_empty() {
        "App".to_owned()
    } else {
        out
    }
}

const fn item(
    label: &'static str,
    icon: Option<UiGlyphKind>,
    shortcut: Option<&'static str>,
    action: TopBarMenuAction,
) -> ResolvedTopBarMenuItem {
    ResolvedTopBarMenuItem::Item {
        label: Cow::Borrowed(label),
        icon,
        shortcut,
        action,
        enabled: true,
    }
}

fn build_active_app_menu(
    profile: TopBarAppProfile,
    active_app_name: &str,
) -> Vec<ResolvedTopBarMenuItem> {
    let app = if active_app_name.trim().is_empty() {
        "Desktop"
    } else {
        active_app_name.trim()
    };
    let is_desktop = matches!(profile, TopBarAppProfile::Desktop)
        || app.eq_ignore_ascii_case("desktop")
        || (matches!(profile, TopBarAppProfile::GenericApp) && active_app_name.trim().is_empty());
    vec![
        ResolvedTopBarMenuItem::Item {
            label: Cow::Owned(format!("Hide {app}")),
            icon: None,
            shortcut: None,
            action: TopBarMenuAction::HideActiveApp,
            enabled: !is_desktop,
        },
        ResolvedTopBarMenuItem::Item {
            label: Cow::Borrowed("Hide Others"),
            icon: None,
            shortcut: None,
            action: TopBarMenuAction::HideOtherApps,
            enabled: true,
        },
        ResolvedTopBarMenuItem::Separator,
        ResolvedTopBarMenuItem::Item {
            label: Cow::Borrowed("Quit"),
            icon: None,
            shortcut: None,
            action: TopBarMenuAction::QuitActiveApp,
            enabled: !is_desktop,
        },
    ]
}

fn build_file_menu(profile: TopBarAppProfile) -> Vec<ResolvedTopBarMenuItem> {
    match profile {
        TopBarAppProfile::Desktop | TopBarAppProfile::Files => {
            top_bar_menu_entries(TopBarMenuHit::File)
                .iter()
                .map(ResolvedTopBarMenuItem::from)
                .collect()
        }
        TopBarAppProfile::Browser => build_browser_file_menu(),
        TopBarAppProfile::Terminal => build_terminal_file_menu(),
        TopBarAppProfile::Editor => build_doc_file_menu("New File", "Close Editor", true),
        TopBarAppProfile::GenericApp => build_doc_file_menu("New", "Close Window", false),
    }
}

fn build_browser_file_menu() -> Vec<ResolvedTopBarMenuItem> {
    vec![
        item(
            "New Tab",
            None,
            Some("⌃T"),
            TopBarMenuAction::AppShortcut(AppShortcutAction::NewTab),
        ),
        item(
            "New Window",
            None,
            Some("⌃N"),
            TopBarMenuAction::AppShortcut(AppShortcutAction::NewWindow),
        ),
        ResolvedTopBarMenuItem::Separator,
        item(
            "Open Location...",
            None,
            Some("⌃L"),
            TopBarMenuAction::AppShortcut(AppShortcutAction::OpenLocation),
        ),
        item(
            "Save Page As...",
            None,
            Some("⌃S"),
            TopBarMenuAction::AppShortcut(AppShortcutAction::SaveFile),
        ),
        ResolvedTopBarMenuItem::Separator,
        item(
            "Close Tab",
            None,
            Some("⌃W"),
            TopBarMenuAction::AppShortcut(AppShortcutAction::CloseTab),
        ),
        item(
            "Close Window",
            None,
            Some("⌥Q"),
            TopBarMenuAction::CloseFocused,
        ),
    ]
}

fn build_terminal_file_menu() -> Vec<ResolvedTopBarMenuItem> {
    vec![
        item(
            "New Terminal Window",
            None,
            None,
            TopBarMenuAction::NewTerminal,
        ),
        item(
            "New Tab",
            None,
            Some("⇧⌃T"),
            TopBarMenuAction::AppShortcut(AppShortcutAction::NewTabShift),
        ),
        ResolvedTopBarMenuItem::Separator,
        item(
            "Close Tab",
            None,
            Some("⇧⌃W"),
            TopBarMenuAction::AppShortcut(AppShortcutAction::CloseTabShift),
        ),
        item(
            "Close Window",
            None,
            Some("⌥Q"),
            TopBarMenuAction::CloseFocused,
        ),
    ]
}

fn build_doc_file_menu(
    new_label: &'static str,
    close_tab_label: &'static str,
    has_close_tab: bool,
) -> Vec<ResolvedTopBarMenuItem> {
    let mut entries = vec![
        item(
            new_label,
            None,
            Some("⌃N"),
            TopBarMenuAction::AppShortcut(AppShortcutAction::NewWindow),
        ),
        item(
            "Open...",
            None,
            Some("⌃O"),
            TopBarMenuAction::AppShortcut(AppShortcutAction::OpenFile),
        ),
        item(
            "Save",
            None,
            Some("⌃S"),
            TopBarMenuAction::AppShortcut(AppShortcutAction::SaveFile),
        ),
        ResolvedTopBarMenuItem::Separator,
    ];
    if has_close_tab {
        entries.push(item(
            close_tab_label,
            None,
            Some("⌃W"),
            TopBarMenuAction::AppShortcut(AppShortcutAction::CloseTab),
        ));
    }
    entries.push(item(
        "Close Window",
        None,
        Some("⌥Q"),
        TopBarMenuAction::CloseFocused,
    ));
    entries
}

fn build_edit_menu(profile: TopBarAppProfile) -> Vec<ResolvedTopBarMenuItem> {
    match profile {
        TopBarAppProfile::Desktop | TopBarAppProfile::Files => {
            top_bar_menu_entries(TopBarMenuHit::Edit)
                .iter()
                .map(ResolvedTopBarMenuItem::from)
                .collect()
        }
        TopBarAppProfile::Terminal => vec![
            item(
                "Copy",
                None,
                Some("⇧⌃C"),
                TopBarMenuAction::AppShortcut(AppShortcutAction::CopyShift),
            ),
            item(
                "Paste",
                None,
                Some("⇧⌃V"),
                TopBarMenuAction::AppShortcut(AppShortcutAction::PasteShift),
            ),
            ResolvedTopBarMenuItem::Separator,
            item(
                "Find...",
                None,
                Some("⇧⌃F"),
                TopBarMenuAction::AppShortcut(AppShortcutAction::FindShift),
            ),
        ],
        TopBarAppProfile::Browser | TopBarAppProfile::Editor | TopBarAppProfile::GenericApp => {
            let find_label = if matches!(profile, TopBarAppProfile::Browser) {
                "Find in Page..."
            } else {
                "Find..."
            };
            vec![
                item(
                    "Undo",
                    None,
                    Some("⌃Z"),
                    TopBarMenuAction::AppShortcut(AppShortcutAction::Undo),
                ),
                item(
                    "Redo",
                    None,
                    Some("⇧⌃Z"),
                    TopBarMenuAction::AppShortcut(AppShortcutAction::Redo),
                ),
                ResolvedTopBarMenuItem::Separator,
                item(
                    "Cut",
                    None,
                    Some("⌃X"),
                    TopBarMenuAction::AppShortcut(AppShortcutAction::Cut),
                ),
                item(
                    "Copy",
                    None,
                    Some("⌃C"),
                    TopBarMenuAction::AppShortcut(AppShortcutAction::Copy),
                ),
                item(
                    "Paste",
                    None,
                    Some("⌃V"),
                    TopBarMenuAction::AppShortcut(AppShortcutAction::Paste),
                ),
                item(
                    "Select All",
                    None,
                    Some("⌃A"),
                    TopBarMenuAction::AppShortcut(AppShortcutAction::SelectAll),
                ),
                ResolvedTopBarMenuItem::Separator,
                item(
                    find_label,
                    None,
                    Some("⌃F"),
                    TopBarMenuAction::AppShortcut(AppShortcutAction::Find),
                ),
            ]
        }
    }
}

fn build_view_menu(profile: TopBarAppProfile) -> Vec<ResolvedTopBarMenuItem> {
    match profile {
        TopBarAppProfile::Desktop | TopBarAppProfile::Files => {
            top_bar_menu_entries(TopBarMenuHit::View)
                .iter()
                .map(ResolvedTopBarMenuItem::from)
                .collect()
        }
        TopBarAppProfile::Browser => vec![
            item(
                "Reload Page",
                None,
                Some("⌃R"),
                TopBarMenuAction::AppShortcut(AppShortcutAction::ReloadPage),
            ),
            ResolvedTopBarMenuItem::Separator,
            item(
                "Zoom In",
                None,
                Some("⌃+"),
                TopBarMenuAction::AppShortcut(AppShortcutAction::ZoomIn),
            ),
            item(
                "Zoom Out",
                None,
                Some("⌃-"),
                TopBarMenuAction::AppShortcut(AppShortcutAction::ZoomOut),
            ),
            item(
                "Actual Size",
                None,
                Some("⌃0"),
                TopBarMenuAction::AppShortcut(AppShortcutAction::ZoomReset),
            ),
            ResolvedTopBarMenuItem::Separator,
            item(
                "Toggle Fullscreen",
                None,
                Some("⌥F"),
                TopBarMenuAction::ToggleFullscreenFocused,
            ),
        ],
        TopBarAppProfile::Terminal | TopBarAppProfile::Editor => vec![
            item(
                "Zoom In",
                None,
                Some("⌃+"),
                TopBarMenuAction::AppShortcut(AppShortcutAction::ZoomIn),
            ),
            item(
                "Zoom Out",
                None,
                Some("⌃-"),
                TopBarMenuAction::AppShortcut(AppShortcutAction::ZoomOut),
            ),
            item(
                "Actual Size",
                None,
                Some("⌃0"),
                TopBarMenuAction::AppShortcut(AppShortcutAction::ZoomReset),
            ),
            ResolvedTopBarMenuItem::Separator,
            item(
                "Toggle Fullscreen",
                None,
                Some("⌥F"),
                TopBarMenuAction::ToggleFullscreenFocused,
            ),
            item(
                "Toggle Float / Tile",
                None,
                Some("⌥L"),
                TopBarMenuAction::ToggleLayout,
            ),
        ],
        TopBarAppProfile::GenericApp => vec![
            item(
                "Toggle Fullscreen",
                None,
                Some("⌥F"),
                TopBarMenuAction::ToggleFullscreenFocused,
            ),
            item(
                "Workspace Overview",
                None,
                Some("⌘⇥"),
                TopBarMenuAction::OpenOverview,
            ),
            item(
                "Toggle Float / Tile",
                None,
                Some("⌥L"),
                TopBarMenuAction::ToggleLayout,
            ),
        ],
    }
}

/// Resolves dropdown entries for `menu` adapted to `profile` and `active_app_name` (`RUSTYOS-068`).
#[must_use]
pub fn resolve_top_bar_menu_entries(
    profile: TopBarAppProfile,
    active_app_name: &str,
    menu: TopBarMenuHit,
) -> Vec<ResolvedTopBarMenuItem> {
    match menu {
        TopBarMenuHit::SystemLogo => top_bar_menu_entries(TopBarMenuHit::SystemLogo)
            .iter()
            .map(ResolvedTopBarMenuItem::from)
            .collect(),
        TopBarMenuHit::ActiveApp => build_active_app_menu(profile, active_app_name),
        TopBarMenuHit::File => build_file_menu(profile),
        TopBarMenuHit::Edit => build_edit_menu(profile),
        TopBarMenuHit::View => build_view_menu(profile),
        TopBarMenuHit::Go => {
            if profile.has_go_menu() {
                top_bar_menu_entries(TopBarMenuHit::Go)
                    .iter()
                    .map(ResolvedTopBarMenuItem::from)
                    .collect()
            } else {
                Vec::new()
            }
        }
        TopBarMenuHit::Window => top_bar_menu_entries(TopBarMenuHit::Window)
            .iter()
            .map(ResolvedTopBarMenuItem::from)
            .collect(),
        TopBarMenuHit::Help => top_bar_menu_entries(TopBarMenuHit::Help)
            .iter()
            .map(ResolvedTopBarMenuItem::from)
            .collect(),
        TopBarMenuHit::Dynamic(_) => Vec::new(),
    }
}
