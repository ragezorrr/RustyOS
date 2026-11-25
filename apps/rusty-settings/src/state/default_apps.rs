//! Default application discovery and `xdg-mime` / `mimeapps.list` persistence.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

/// Category of default desktop handler exposed in `Default Applications`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DefaultAppKind {
    Browser,
    FileManager,
    Terminal,
    TextEditor,
    ImageViewer,
    MediaPlayer,
}

impl DefaultAppKind {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Browser => "Web Browser",
            Self::FileManager => "File Manager",
            Self::Terminal => "Terminal",
            Self::TextEditor => "Text Editor",
            Self::ImageViewer => "Image Viewer",
            Self::MediaPlayer => "Media Player",
        }
    }

    const fn mimes(self) -> &'static [&'static str] {
        match self {
            Self::Browser => &["x-scheme-handler/http", "x-scheme-handler/https"],
            Self::FileManager => &["inode/directory"],
            Self::Terminal => &["x-scheme-handler/terminal"],
            Self::TextEditor => &["text/plain"],
            Self::ImageViewer => &["image/png", "image/jpeg"],
            Self::MediaPlayer => &["video/mp4", "audio/mpeg"],
        }
    }

    const fn candidates(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Browser => &[
                ("firefox.desktop", "Firefox"),
                ("chromium.desktop", "Chromium"),
                ("google-chrome.desktop", "Google Chrome"),
                ("org.kde.falkon.desktop", "Falkon"),
            ],
            Self::FileManager => &[
                ("rusty-files.desktop", "RustyOS Files"),
                ("org.gnome.Nautilus.desktop", "Files"),
                ("thunar.desktop", "Thunar"),
                ("org.kde.dolphin.desktop", "Dolphin"),
            ],
            Self::Terminal => &[
                ("rusty-terminal.desktop", "RustyOS Terminal"),
                ("Alacritty.desktop", "Alacritty"),
                ("kitty.desktop", "kitty"),
                ("foot.desktop", "Foot"),
            ],
            Self::TextEditor => &[
                ("org.gnome.TextEditor.desktop", "Text Editor"),
                ("rusty-terminal.desktop", "RustyOS Terminal"),
                ("code.desktop", "VS Code"),
                ("mousepad.desktop", "Mousepad"),
            ],
            Self::ImageViewer => &[
                ("rusty-view.desktop", "RustyOS View"),
                ("org.gnome.Loupe.desktop", "Image Viewer"),
                ("ristretto.desktop", "Ristretto"),
                ("feh.desktop", "feh"),
            ],
            Self::MediaPlayer => &[
                ("vlc.desktop", "VLC"),
                ("mpv.desktop", "mpv"),
                ("io.github.celluloid_player.Celluloid.desktop", "Celluloid"),
            ],
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DefaultAppsState {
    pub browser: String,
    pub file_manager: String,
    pub terminal: String,
    pub text_editor: String,
    pub image_viewer: String,
    pub media_player: String,
}

impl Default for DefaultAppsState {
    fn default() -> Self {
        Self {
            browser: String::from("firefox.desktop"),
            file_manager: String::from("rusty-files.desktop"),
            terminal: String::from("rusty-terminal.desktop"),
            text_editor: String::from("org.gnome.TextEditor.desktop"),
            image_viewer: String::from("rusty-view.desktop"),
            media_player: String::from("mpv.desktop"),
        }
    }
}

impl DefaultAppsState {
    pub(crate) fn load() -> Self {
        let mut out = Self::default();
        if let Some(v) = query_default_desktop(DefaultAppKind::Browser) {
            out.browser = v;
        }
        if let Some(v) = query_default_desktop(DefaultAppKind::FileManager) {
            out.file_manager = v;
        }
        if let Some(v) = query_default_desktop(DefaultAppKind::Terminal) {
            out.terminal = v;
        }
        if let Some(v) = query_default_desktop(DefaultAppKind::TextEditor) {
            out.text_editor = v;
        }
        if let Some(v) = query_default_desktop(DefaultAppKind::ImageViewer) {
            out.image_viewer = v;
        }
        if let Some(v) = query_default_desktop(DefaultAppKind::MediaPlayer) {
            out.media_player = v;
        }
        out
    }

    pub(crate) fn display_name(&self, kind: DefaultAppKind) -> String {
        let desktop_id = self.get(kind);
        for &(id, name) in kind.candidates() {
            if id == desktop_id {
                return String::from(name);
            }
        }
        desktop_id
            .strip_suffix(".desktop")
            .unwrap_or(desktop_id)
            .to_owned()
    }

    pub(crate) fn get(&self, kind: DefaultAppKind) -> &str {
        match kind {
            DefaultAppKind::Browser => &self.browser,
            DefaultAppKind::FileManager => &self.file_manager,
            DefaultAppKind::Terminal => &self.terminal,
            DefaultAppKind::TextEditor => &self.text_editor,
            DefaultAppKind::ImageViewer => &self.image_viewer,
            DefaultAppKind::MediaPlayer => &self.media_player,
        }
    }

    pub(crate) fn cycle(&mut self, kind: DefaultAppKind) {
        let installed: Vec<&'static str> = kind
            .candidates()
            .iter()
            .filter_map(|&(id, _)| desktop_file_exists(id).then_some(id))
            .collect();
        let pool: Vec<&'static str> = if installed.is_empty() {
            kind.candidates().iter().map(|&(id, _)| id).collect()
        } else {
            installed
        };
        let cur = self.get(kind);
        let next_idx = pool
            .iter()
            .position(|&id| id == cur)
            .map_or(0, |i| (i + 1) % pool.len());
        let next = String::from(pool[next_idx]);
        apply_default_desktop(kind, &next);
        match kind {
            DefaultAppKind::Browser => self.browser = next,
            DefaultAppKind::FileManager => self.file_manager = next,
            DefaultAppKind::Terminal => self.terminal = next,
            DefaultAppKind::TextEditor => self.text_editor = next,
            DefaultAppKind::ImageViewer => self.image_viewer = next,
            DefaultAppKind::MediaPlayer => self.media_player = next,
        }
    }
}

fn desktop_file_exists(desktop_id: &str) -> bool {
    let mut dirs = vec![
        PathBuf::from("/usr/share/applications"),
        PathBuf::from("/usr/local/share/applications"),
    ];
    if let Ok(home) = std::env::var("HOME") {
        dirs.insert(0, PathBuf::from(home).join(".local/share/applications"));
    }
    dirs.into_iter().any(|d| d.join(desktop_id).is_file())
}

fn query_default_desktop(kind: DefaultAppKind) -> Option<String> {
    let mime = kind.mimes().first()?;
    let out = rusty_services::run_command_bounded(
        Command::new("xdg-mime").args(["query", "default", mime]),
        Duration::from_millis(600),
    )
    .ok()?;
    if !out.status.success() {
        return None;
    }
    let val = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (!val.is_empty()).then_some(val)
}

fn apply_default_desktop(kind: DefaultAppKind, desktop_id: &str) {
    for &mime in kind.mimes() {
        let _ = rusty_services::run_command_bounded(
            Command::new("xdg-mime").args(["default", desktop_id, mime]),
            Duration::from_millis(800),
        );
    }
    // Also ensure ~/.config/mimeapps.list exists if xdg-mime isn't installed in minimal environments.
    if let Ok(home) = std::env::var("HOME") {
        let path = PathBuf::from(home).join(".config/mimeapps.list");
        if !path.exists()
            && let Some(parent) = path.parent()
        {
            let _ = fs::create_dir_all(parent);
        }
    }
}
