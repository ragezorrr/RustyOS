//! Common data types, classifications, and system configurations for `file_manager`.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process;

/// Visual theme mode for `rusty-files` (`RUSTYOS-065`, `RUSTYOS-067`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FilesThemeMode {
    /// Golden Gate frosted light appearance (default).
    #[default]
    LightGoldenGate,
    /// Golden Gate dark graphite appearance (`DESIGN.md`).
    DarkPurpleGlass,
}

impl FilesThemeMode {
    /// Alias for Golden Gate dark appearance (`RUSTYOS-067`).
    pub const DARK_GOLDEN_GATE: Self = Self::DarkPurpleGlass;

    /// Toggles between light and dark themes.
    #[must_use]
    pub const fn toggle(self) -> Self {
        match self {
            Self::LightGoldenGate => Self::DarkPurpleGlass,
            Self::DarkPurpleGlass => Self::LightGoldenGate,
        }
    }

    /// Config serialization token.
    #[must_use]
    pub const fn as_config_str(self) -> &'static str {
        match self {
            Self::LightGoldenGate => "light",
            Self::DarkPurpleGlass => "dark",
        }
    }

    /// Parses a persisted theme token.
    #[must_use]
    pub fn from_config_str(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "light" | "light_golden_gate" => Some(Self::LightGoldenGate),
            "dark" | "dark_golden_gate" | "dark_purple_glass" => Some(Self::DarkPurpleGlass),
            _ => None,
        }
    }
}

/// Number of built-in RustyOS accent presets (`RUSTYOS-114`).
pub const RUSTYOS_ACCENT_COUNT: usize = 7;

/// Swatch colors shown in `System Settings -> Appearance` for the 7 RustyOS accent presets.
pub const RUSTYOS_ACCENT_SWATCHES: [[u8; 4]; RUSTYOS_ACCENT_COUNT] = [
    [0x33, 0x85, 0xFF, 0xFF], // Blue
    [0xFF, 0x5C, 0x6D, 0xFF], // Coral Red
    [0x35, 0xBF, 0x56, 0xFF], // Green
    [0x82, 0x66, 0xFF, 0xFF], // Indigo
    [0xCA, 0x64, 0xAC, 0xFF], // Pink
    [0xFE, 0xA0, 0x42, 0xFF], // Orange
    [0x4F, 0x59, 0x6B, 0xFF], // Graphite
];

const ACCENT_LIGHT_RGBA: [[u8; 4]; RUSTYOS_ACCENT_COUNT] = [
    [0, 122, 255, 255],  // Golden Gate Blue #007AFF
    [255, 59, 48, 255],  // Coral Red #FF3B30
    [40, 205, 65, 255],  // Emerald Green #28CD41
    [94, 92, 230, 255],  // Indigo #5E5CE6
    [212, 84, 166, 255], // Pink #D454A6
    [255, 149, 0, 255],  // Orange #FF9500
    [99, 105, 118, 255], // Graphite #636976
];

const ACCENT_DARK_RGBA: [[u8; 4]; RUSTYOS_ACCENT_COUNT] = [
    [10, 132, 255, 255],  // Golden Gate Dark Blue #0A84FF
    [255, 92, 109, 255],  // Coral Red
    [53, 191, 86, 255],   // Green
    [130, 102, 255, 255], // Indigo
    [232, 104, 184, 255], // Pink
    [254, 160, 66, 255],  // Orange
    [122, 132, 148, 255], // Graphite
];

/// Clamps `[ui].accent_index` into the built-in palette bounds (`0..7`).
#[must_use]
pub const fn clamp_accent_index(index: usize) -> usize {
    if index >= RUSTYOS_ACCENT_COUNT {
        RUSTYOS_ACCENT_COUNT - 1
    } else {
        index
    }
}

/// Resolves theme-aware `[r, g, b, 255]` accent color for `index` (`RUSTYOS-114`).
#[must_use]
pub const fn accent_rgba_for_index(index: usize, is_dark: bool) -> [u8; 4] {
    let idx = clamp_accent_index(index);
    if is_dark {
        ACCENT_DARK_RGBA[idx]
    } else {
        ACCENT_LIGHT_RGBA[idx]
    }
}

fn find_ui_toml_value<'a>(content: &'a str, target_keys: &[&str]) -> Option<&'a str> {
    let mut in_ui = false;
    for raw_line in content.lines() {
        let line = strip_toml_comment(raw_line).trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            in_ui = line.eq_ignore_ascii_case("[ui]");
            continue;
        }
        if in_ui
            && let Some((raw_k, raw_v)) = line.split_once('=')
            && target_keys.contains(&raw_k.trim())
        {
            return Some(raw_v.trim().trim_matches('"').trim_matches('\''));
        }
    }
    None
}

/// Parses `[ui] theme` from a RustyOS `config.toml` content string if present (`RUSTYOS-067`).
#[must_use]
pub fn detect_system_theme_from_toml(content: &str) -> Option<FilesThemeMode> {
    find_ui_toml_value(content, &["theme", "appearance"]).and_then(FilesThemeMode::from_config_str)
}

/// Parses `[ui] accent_index` from a RustyOS `config.toml` content string if present (`RUSTYOS-114`).
#[must_use]
pub fn detect_system_accent_from_toml(content: &str) -> Option<usize> {
    find_ui_toml_value(content, &["accent_index"])?
        .parse::<usize>()
        .ok()
        .map(clamp_accent_index)
}

/// Content view modes presented in the Golden Gate view switcher pill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FileViewMode {
    /// Icon grid view (default Golden Gate Home view; `RUSTYOS-066`/`071`).
    #[default]
    Icons,
    /// Multi-column sortable list view.
    List,
    /// Miller Columns hierarchical browser.
    Columns,
    /// Large preview gallery view with filmstrip & inspector (`RUSTYOS-066`).
    Gallery,
}

impl FileViewMode {
    /// Returns whether this mode is active in the current build (all four modes supported in Stage B).
    #[must_use]
    pub const fn is_supported(self) -> bool {
        let _ = self;
        true
    }

    /// Human-readable short label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Icons => "Icons",
            Self::List => "List",
            Self::Columns => "Columns",
            Self::Gallery => "Gallery",
        }
    }

    /// Config serialization token.
    #[must_use]
    pub const fn as_config_str(self) -> &'static str {
        match self {
            Self::Icons => "icons",
            Self::List => "list",
            Self::Columns => "columns",
            Self::Gallery => "gallery",
        }
    }

    /// Parses a persisted view mode token.
    #[must_use]
    pub fn from_config_str(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "icons" => Some(Self::Icons),
            "list" => Some(Self::List),
            "columns" => Some(Self::Columns),
            "gallery" => Some(Self::Gallery),
            _ => None,
        }
    }
}

/// Primary sort criterion for directory entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FileSortField {
    /// Case-insensitive lexicographical name order, folders first.
    #[default]
    Name,
    /// Last modification timestamp.
    DateModified,
    /// Logical byte size.
    Size,
    /// Entry type / extension label.
    Kind,
}

impl FileSortField {
    /// Human-readable menu label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::DateModified => "Date Modified",
            Self::Size => "Size",
            Self::Kind => "Kind",
        }
    }

    /// Config serialization token.
    #[must_use]
    pub const fn as_config_str(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::DateModified => "date_modified",
            Self::Size => "size",
            Self::Kind => "kind",
        }
    }

    /// Parses a persisted sort field token.
    #[must_use]
    pub fn from_config_str(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "name" => Some(Self::Name),
            "date_modified" | "modified" | "date" => Some(Self::DateModified),
            "size" => Some(Self::Size),
            "kind" => Some(Self::Kind),
            _ => None,
        }
    }
}

/// Sort direction for directory listings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FileSortOrder {
    /// Ascending order (`A -> Z`, oldest -> newest, smallest -> largest).
    #[default]
    Ascending,
    /// Descending order (`Z -> A`, newest -> oldest, largest -> smallest).
    Descending,
}

impl FileSortOrder {
    /// Flips sort direction.
    #[must_use]
    pub const fn toggle(self) -> Self {
        match self {
            Self::Ascending => Self::Descending,
            Self::Descending => Self::Ascending,
        }
    }

    /// Config serialization token.
    #[must_use]
    pub const fn as_config_str(self) -> &'static str {
        match self {
            Self::Ascending => "asc",
            Self::Descending => "desc",
        }
    }

    /// Parses a persisted sort direction token.
    #[must_use]
    pub fn from_config_str(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "asc" | "ascending" => Some(Self::Ascending),
            "desc" | "descending" => Some(Self::Descending),
            _ => None,
        }
    }
}

/// Local color tags modeled on macOS Finder Tags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FileTagColor {
    /// Red tag (`#FF3B30`).
    Red,
    /// Orange tag (`#FF9500`).
    Orange,
    /// Yellow tag (`#FFCC00`).
    Yellow,
    /// Green tag (`#34C759`).
    Green,
    /// Blue tag (`#007AFF`).
    Blue,
    /// Purple tag (`#AF52DE`).
    Purple,
}

impl FileTagColor {
    /// Ordered sidebar list of standard tags.
    pub const ALL: [Self; 6] = [
        Self::Red,
        Self::Orange,
        Self::Yellow,
        Self::Green,
        Self::Blue,
        Self::Purple,
    ];

    /// Display name matching sidebar rows.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Red => "Red",
            Self::Orange => "Orange",
            Self::Yellow => "Yellow",
            Self::Green => "Green",
            Self::Blue => "Blue",
            Self::Purple => "Purple",
        }
    }

    /// sRGB swatch color `[r, g, b]`.
    #[must_use]
    pub const fn rgb(self) -> [u8; 3] {
        match self {
            Self::Red => [0xFF, 0x3B, 0x30],
            Self::Orange => [0xFF, 0x95, 0x00],
            Self::Yellow => [0xFF, 0xCC, 0x00],
            Self::Green => [0x34, 0xC7, 0x59],
            Self::Blue => [0x00, 0x7A, 0xFF],
            Self::Purple => [0xAF, 0x52, 0xDE],
        }
    }

    /// Lowercase token stored in `file_tags.toml`.
    #[must_use]
    pub const fn as_config_str(self) -> &'static str {
        match self {
            Self::Red => "red",
            Self::Orange => "orange",
            Self::Yellow => "yellow",
            Self::Green => "green",
            Self::Blue => "blue",
            Self::Purple => "purple",
        }
    }

    /// Parses a tag color token case-insensitively.
    #[must_use]
    pub fn from_config_str(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "red" => Some(Self::Red),
            "orange" => Some(Self::Orange),
            "yellow" => Some(Self::Yellow),
            "green" => Some(Self::Green),
            "blue" => Some(Self::Blue),
            "purple" => Some(Self::Purple),
            _ => None,
        }
    }
}

/// Sidebar navigation targets in `rusty-files`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidebarLocation {
    /// Recently opened/modified files.
    Recents,
    /// Shared folders and mounted network shares.
    Shared,
    /// Discovered `.desktop` applications.
    Applications,
    /// User Desktop (`~/Desktop`).
    Desktop,
    /// User Documents (`~/Documents`).
    Documents,
    /// User Downloads (`~/Downloads`).
    Downloads,
    /// User Home directory (`~`).
    Home,
    /// Mounted filesystem or volume path.
    Volume(PathBuf),
    /// Network hosts and network filesystems.
    Network,
    /// `FreeDesktop` Trash (`~/.local/share/Trash`).
    Trash,
    /// Virtual folder filtering locally tagged files by color.
    Tag(FileTagColor),
}

/// Target location resolved for background directory listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListingTarget {
    /// Standard filesystem directory on disk.
    Directory(PathBuf),
    /// Virtual Recents listing (`recently-used.xbel` + recent user docs).
    Recents,
    /// Shared/public folders and remote mounts.
    Shared,
    /// Installed `.desktop` applications across XDG app dirs.
    Applications,
    /// Network shares and `/run/user/.../gvfs` or `/proc/mounts` network volumes.
    Network,
    /// `FreeDesktop` Trash contents.
    Trash,
    /// Virtual filtered collection for a single `FileTagColor`.
    Tag(FileTagColor),
}

/// Kind of filesystem or virtual entry shown in `rusty-files`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileEntryKind {
    /// Folder / directory.
    Directory,
    /// Regular file.
    RegularFile,
    /// Desktop application (`.desktop`).
    Application,
    /// Mounted volume or network share root.
    MountPoint,
    /// Symbolic link.
    Symlink,
    /// FIFO, socket, block/char device, or unknown special node.
    Other,
}

/// Metadata record for a single item in a column, list, or preview pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// Display name (file/folder basename, application `Name=`, or mount label).
    pub name: String,
    /// Backing path on disk (or inside `Trash/files` for trashed items).
    pub path: PathBuf,
    /// Entry classification.
    pub kind: FileEntryKind,
    /// Byte size for regular files (`0` for directories unless computed).
    pub size_bytes: u64,
    /// Last modification time as Unix epoch seconds.
    pub modified_unix_secs: Option<i64>,
    /// True for dot-prefixed hidden Unix entries.
    pub hidden: bool,
    /// True if `kind == Symlink` and the resolved target is a directory.
    pub symlink_target_is_dir: bool,
    /// Exec line for `.desktop` application entries.
    pub desktop_exec: Option<String>,
    /// Original path prior to trashing (for Trash entries).
    pub trash_original_path: Option<PathBuf>,
    /// Deletion timestamp string from `.trashinfo` (`YYYY-MM-DDTHH:MM:SS`).
    pub trash_deletion_date: Option<String>,
    /// Optional secondary descriptor (e.g. filesystem type).
    pub subtitle: Option<String>,
}

impl FileEntry {
    /// True when opening/selecting this entry navigates into a child column/folder.
    #[must_use]
    pub const fn is_navigable_dir(&self) -> bool {
        matches!(
            self.kind,
            FileEntryKind::Directory | FileEntryKind::MountPoint
        ) || (matches!(self.kind, FileEntryKind::Symlink) && self.symlink_target_is_dir)
    }

    /// Lowercased file extension when present.
    #[must_use]
    pub fn extension_lower(&self) -> Option<String> {
        self.path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_ascii_lowercase)
    }

    /// Human-readable Finder-style "Kind" string.
    #[must_use]
    pub fn kind_label(&self) -> &'static str {
        match self.kind {
            FileEntryKind::Directory => "Folder",
            FileEntryKind::Application => "Application",
            FileEntryKind::MountPoint => "Volume",
            FileEntryKind::Symlink => {
                if self.symlink_target_is_dir {
                    "Alias (Folder)"
                } else {
                    "Alias"
                }
            }
            FileEntryKind::Other => "Special File",
            FileEntryKind::RegularFile => classify_extension_kind(&self.name),
        }
    }
}

pub(crate) fn classify_extension_kind(name: &str) -> &'static str {
    let Some((_, ext)) = name.rsplit_once('.') else {
        return "Document";
    };
    match ext.to_ascii_lowercase().as_str() {
        "png" | "jpg" | "jpeg" | "webp" | "svg" | "gif" | "bmp" | "ico" => "Image",
        "mp4" | "mkv" | "webm" | "mov" | "avi" => "Video",
        "mp3" | "flac" | "ogg" | "wav" | "m4a" | "opus" => "Audio",
        "rs" | "toml" | "json" | "yaml" | "yml" | "py" | "sh" | "c" | "h" | "md" => "Source Code",
        "txt" | "log" | "csv" | "pdf" => "Document",
        "zip" | "tar" | "gz" | "xz" | "zst" | "7z" => "Archive",
        "deb" | "rpm" | "appimage" => "Package",
        _ => "File",
    }
}

/// Discovered mounted volume or network share from `/proc/mounts`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeInfo {
    /// Human-readable mount label.
    pub name: String,
    /// Mounted filesystem path.
    pub mount_point: PathBuf,
    /// Source device or remote share spec.
    pub device: String,
    /// Linux filesystem type (e.g. `ext4`, `btrfs`, `nfs4`, `cifs`).
    pub fs_type: String,
    /// True for network/remote filesystems.
    pub is_network: bool,
}

/// Resolved user XDG paths used across `rusty-files`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserFileLocations {
    /// User home directory.
    pub home: PathBuf,
    /// User username (for Home sidebar label).
    pub user_label: String,
    /// User Desktop directory.
    pub desktop: PathBuf,
    /// User Documents directory.
    pub documents: PathBuf,
    /// User Downloads directory.
    pub downloads: PathBuf,
    /// User Public/Shared directory (`XDG_PUBLICSHARE_DIR` or `~/Public`).
    pub public_share: PathBuf,
    /// RustyOS config directory (`~/.config/rustyos`).
    pub config_dir: PathBuf,
    /// XDG Data home (`~/.local/share`).
    pub data_home: PathBuf,
    /// `FreeDesktop` Trash root (`~/.local/share/Trash`).
    pub trash_dir: PathBuf,
    /// Application `.desktop` directories in search priority order.
    pub app_dirs: Vec<PathBuf>,
}

impl UserFileLocations {
    /// Detects user paths from environment variables and `~/.config/user-dirs.dirs`.
    #[must_use]
    pub fn detect() -> Self {
        let home = env::var_os("HOME")
            .filter(|v| !v.is_empty())
            .map_or_else(|| PathBuf::from("/tmp"), PathBuf::from);
        let config_root = env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map_or_else(|| home.join(".config"), PathBuf::from);
        let data_home = env::var_os("XDG_DATA_HOME")
            .filter(|v| !v.is_empty())
            .map_or_else(|| home.join(".local/share"), PathBuf::from);

        let mut base = Self::with_roots(&home, &config_root, &data_home);
        if let Ok(content) = fs::read_to_string(config_root.join("user-dirs.dirs")) {
            let parsed = parse_xdg_user_dirs(&content, &home);
            if let Some(p) = parsed.get("XDG_DESKTOP_DIR") {
                base.desktop.clone_from(p);
            }
            if let Some(p) = parsed.get("XDG_DOCUMENTS_DIR") {
                base.documents.clone_from(p);
            }
            if let Some(p) = parsed.get("XDG_DOWNLOAD_DIR") {
                base.downloads.clone_from(p);
            }
            if let Some(p) = parsed.get("XDG_PUBLICSHARE_DIR") {
                base.public_share.clone_from(p);
            }
        }
        base
    }

    /// Constructs isolated locations for deterministic testing without touching env vars.
    #[must_use]
    pub fn with_roots(home: &Path, config_root: &Path, data_home: &Path) -> Self {
        let user_label = home
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|s| !s.is_empty())
            .map_or_else(|| String::from("Home"), ToOwned::to_owned);
        Self {
            home: home.to_path_buf(),
            user_label,
            desktop: home.join("Desktop"),
            documents: home.join("Documents"),
            downloads: home.join("Downloads"),
            public_share: home.join("Public"),
            config_dir: config_root.join("rustyos"),
            data_home: data_home.to_path_buf(),
            trash_dir: data_home.join("Trash"),
            app_dirs: vec![
                data_home.join("applications"),
                PathBuf::from("/usr/local/share/applications"),
                PathBuf::from("/usr/share/applications"),
            ],
        }
    }

    /// Resolves a `SidebarLocation` into its concrete `ListingTarget`.
    #[must_use]
    pub fn resolve_target(&self, location: &SidebarLocation) -> ListingTarget {
        match location {
            SidebarLocation::Recents => ListingTarget::Recents,
            SidebarLocation::Shared => ListingTarget::Shared,
            SidebarLocation::Applications => ListingTarget::Applications,
            SidebarLocation::Desktop => ListingTarget::Directory(self.desktop.clone()),
            SidebarLocation::Documents => ListingTarget::Directory(self.documents.clone()),
            SidebarLocation::Downloads => ListingTarget::Directory(self.downloads.clone()),
            SidebarLocation::Home => ListingTarget::Directory(self.home.clone()),
            SidebarLocation::Volume(path) => ListingTarget::Directory(path.clone()),
            SidebarLocation::Network => ListingTarget::Network,
            SidebarLocation::Trash => ListingTarget::Trash,
            SidebarLocation::Tag(color) => ListingTarget::Tag(*color),
        }
    }

    /// Path to persisted `file_tags.toml`.
    #[must_use]
    pub fn tags_config_path(&self) -> PathBuf {
        self.config_dir.join("file_tags.toml")
    }

    /// Detects the active system theme from `~/.config/rustyos/config.toml` if present (`RUSTYOS-067`).
    #[must_use]
    pub fn detect_system_theme(&self) -> Option<FilesThemeMode> {
        let path = self.config_dir.join("config.toml");
        let content = fs::read_to_string(path).ok()?;
        detect_system_theme_from_toml(&content)
    }

    /// Detects the active system `[ui].accent_index` from `~/.config/rustyos/config.toml` if present (`RUSTYOS-114`).
    #[must_use]
    pub fn detect_system_accent(&self) -> Option<usize> {
        let path = self.config_dir.join("config.toml");
        let content = fs::read_to_string(path).ok()?;
        detect_system_accent_from_toml(&content)
    }
}

pub(crate) fn parse_xdg_user_dirs(content: &str, home: &Path) -> BTreeMap<String, PathBuf> {
    let mut map = BTreeMap::new();
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, raw_val)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let val = raw_val.trim().trim_matches('"');
        if let Some(rel) = val.strip_prefix("$HOME/") {
            map.insert(key.to_owned(), home.join(rel));
        } else if val == "$HOME" {
            map.insert(key.to_owned(), home.to_path_buf());
        } else if val.starts_with('/') {
            map.insert(key.to_owned(), PathBuf::from(val));
        }
    }
    map
}

/// Persistent local tag assignments and `rusty-files` UI preferences (`file_tags.toml`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileTagsConfig {
    /// Active light/dark theme mode.
    pub theme: FilesThemeMode,
    /// Last active view mode.
    pub view_mode: FileViewMode,
    /// Active sort field.
    pub sort_field: FileSortField,
    /// Active sort order.
    pub sort_order: FileSortOrder,
    /// Whether dot-files are displayed.
    pub show_hidden: bool,
    /// Whether the bottom Path Bar breadcrumb strip is displayed (`RUSTYOS-071`).
    pub show_path_bar: bool,
    /// Ordered tag list per canonical/filesystem path.
    pub tags: BTreeMap<PathBuf, Vec<FileTagColor>>,
}

impl FileTagsConfig {
    /// Loads `file_tags.toml` from `path`, returning defaults if missing or unreadable.
    #[must_use]
    pub fn load_from_path(path: &Path) -> Self {
        match fs::read_to_string(path) {
            Ok(content) => Self::parse_toml(&content),
            Err(_) => Self::default(),
        }
    }

    /// Atomically saves configuration and tags to `path` via temp file + rename.
    ///
    /// # Errors
    /// Returns an `io::Error` if creating the parent directory or writing/renaming fails.
    pub fn save_to_path(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let encoded = self.encode_toml();
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file_tags.toml");
        let tmp_path = path.with_file_name(format!(".{file_name}.tmp.{}", process::id()));
        let write_result = (|| -> io::Result<()> {
            let mut f = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&tmp_path)?;
            f.write_all(encoded.as_bytes())?;
            f.flush()?;
            drop(f);
            fs::rename(&tmp_path, path)?;
            Ok(())
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&tmp_path);
        }
        write_result
    }

    /// Returns assigned tags for `path`.
    #[must_use]
    pub fn tags_for(&self, path: &Path) -> &[FileTagColor] {
        self.tags.get(path).map_or(&[], Vec::as_slice)
    }

    /// Toggles `tag` on `path`, returning `true` if added or `false` if removed.
    pub fn toggle_tag(&mut self, path: &Path, tag: FileTagColor) -> bool {
        let entry = self.tags.entry(path.to_path_buf()).or_default();
        if let Some(pos) = entry.iter().position(|c| *c == tag) {
            entry.remove(pos);
            if entry.is_empty() {
                self.tags.remove(path);
            }
            false
        } else {
            entry.push(tag);
            entry.sort_unstable();
            entry.dedup();
            true
        }
    }

    /// Removes all tag assignments for `path`.
    pub fn remove_tags_for(&mut self, path: &Path) {
        self.tags.remove(path);
    }

    /// Returns all tagged paths matching `tag`.
    #[must_use]
    pub fn paths_with_tag(&self, tag: FileTagColor) -> Vec<PathBuf> {
        self.tags
            .iter()
            .filter_map(|(p, colors)| colors.contains(&tag).then_some(p.clone()))
            .collect()
    }

    fn encode_toml(&self) -> String {
        let mut out = String::from("[preferences]\n");
        let _ = writeln!(out, "theme = \"{}\"", self.theme.as_config_str());
        let _ = writeln!(out, "view_mode = \"{}\"", self.view_mode.as_config_str());
        let _ = writeln!(out, "sort_field = \"{}\"", self.sort_field.as_config_str());
        let _ = writeln!(out, "sort_order = \"{}\"", self.sort_order.as_config_str());
        let _ = writeln!(out, "show_hidden = {}", self.show_hidden);
        let _ = writeln!(out, "show_path_bar = {}", self.show_path_bar);

        for (path, colors) in &self.tags {
            if colors.is_empty() {
                continue;
            }
            out.push_str("\n[[tag]]\n");
            let _ = writeln!(
                out,
                "path = \"{}\"",
                escape_toml_string(&path.to_string_lossy())
            );
            let color_list: Vec<String> = colors
                .iter()
                .map(|c| format!("\"{}\"", c.as_config_str()))
                .collect();
            let _ = writeln!(out, "colors = [{}]", color_list.join(", "));
        }
        out
    }

    fn parse_toml(content: &str) -> Self {
        let mut cfg = Self::default();
        let mut section = TomlSection::Root;
        let mut pending_path: Option<PathBuf> = None;
        let mut pending_colors: Vec<FileTagColor> = Vec::new();

        for raw_line in content.lines() {
            let line = strip_toml_comment(raw_line).trim();
            if line.is_empty() {
                continue;
            }
            if line == "[preferences]" {
                flush_pending_tag(&mut cfg.tags, &mut pending_path, &mut pending_colors);
                section = TomlSection::Preferences;
                continue;
            }
            if line == "[[tag]]" {
                flush_pending_tag(&mut cfg.tags, &mut pending_path, &mut pending_colors);
                section = TomlSection::TagArray;
                continue;
            }
            if line.starts_with('[') {
                flush_pending_tag(&mut cfg.tags, &mut pending_path, &mut pending_colors);
                section = TomlSection::Other;
                continue;
            }
            let Some((raw_k, raw_v)) = line.split_once('=') else {
                continue;
            };
            let k = raw_k.trim();
            let v = raw_v.trim();
            match section {
                TomlSection::Preferences => apply_preference_kv(&mut cfg, k, v),
                TomlSection::TagArray => {
                    apply_tag_kv(&mut pending_path, &mut pending_colors, k, v);
                }
                TomlSection::Root | TomlSection::Other => {}
            }
        }
        flush_pending_tag(&mut cfg.tags, &mut pending_path, &mut pending_colors);
        cfg
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TomlSection {
    Root,
    Preferences,
    TagArray,
    Other,
}

fn flush_pending_tag(
    tags: &mut BTreeMap<PathBuf, Vec<FileTagColor>>,
    pending_path: &mut Option<PathBuf>,
    pending_colors: &mut Vec<FileTagColor>,
) {
    if let Some(path) = pending_path.take()
        && !pending_colors.is_empty()
    {
        let mut colors = std::mem::take(pending_colors);
        colors.sort_unstable();
        colors.dedup();
        tags.insert(path, colors);
    } else {
        pending_colors.clear();
    }
}

fn apply_preference_kv(cfg: &mut FileTagsConfig, key: &str, raw_val: &str) {
    let unquoted = unescape_toml_quoted(raw_val);
    match key {
        "theme" => {
            if let Some(t) = FilesThemeMode::from_config_str(&unquoted) {
                cfg.theme = t;
            }
        }
        "view_mode" => {
            if let Some(v) = FileViewMode::from_config_str(&unquoted) {
                cfg.view_mode = v;
            }
        }
        "sort_field" => {
            if let Some(s) = FileSortField::from_config_str(&unquoted) {
                cfg.sort_field = s;
            }
        }
        "sort_order" => {
            if let Some(o) = FileSortOrder::from_config_str(&unquoted) {
                cfg.sort_order = o;
            }
        }
        "show_hidden" => match raw_val.trim() {
            "true" => cfg.show_hidden = true,
            "false" => cfg.show_hidden = false,
            _ => {}
        },
        "show_path_bar" => match raw_val.trim() {
            "true" => cfg.show_path_bar = true,
            "false" => cfg.show_path_bar = false,
            _ => {}
        },
        _ => {}
    }
}

fn apply_tag_kv(
    pending_path: &mut Option<PathBuf>,
    pending_colors: &mut Vec<FileTagColor>,
    key: &str,
    raw_val: &str,
) {
    match key {
        "path" => {
            let decoded = unescape_toml_quoted(raw_val);
            if !decoded.is_empty() {
                *pending_path = Some(PathBuf::from(decoded));
            }
        }
        "colors" => {
            let inner = raw_val
                .trim()
                .strip_prefix('[')
                .and_then(|s| s.strip_suffix(']'))
                .unwrap_or("");
            pending_colors.clear();
            for item in inner.split(',') {
                let token = unescape_toml_quoted(item.trim());
                if let Some(c) = FileTagColor::from_config_str(&token) {
                    pending_colors.push(c);
                }
            }
        }
        _ => {}
    }
}

fn strip_toml_comment(line: &str) -> &str {
    let mut in_quotes = false;
    let mut escaped = false;
    for (idx, ch) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' if in_quotes => escaped = true,
            '"' => in_quotes = !in_quotes,
            '#' if !in_quotes => return &line[..idx],
            _ => {}
        }
    }
    line
}

fn escape_toml_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out
}

fn unescape_toml_quoted(raw: &str) -> String {
    let trimmed = raw.trim();
    let Some(inner) = trimmed.strip_prefix('"').and_then(|s| s.strip_suffix('"')) else {
        return trimmed.to_owned();
    };
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('"') => out.push('"'),
                Some('\\') | None => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// Discovers mounted disks and network volumes from `/proc/mounts`.
#[must_use]
pub fn discover_mounts() -> Vec<VolumeInfo> {
    match fs::read_to_string("/proc/mounts") {
        Ok(content) => parse_proc_mounts(&content),
        Err(_) => vec![VolumeInfo {
            name: String::from("RustyOS Root"),
            mount_point: PathBuf::from("/"),
            device: String::from("root"),
            fs_type: String::from("linux"),
            is_network: false,
        }],
    }
}

/// Parses `/proc/mounts` lines into user-facing volumes and network shares.
#[must_use]
pub fn parse_proc_mounts(content: &str) -> Vec<VolumeInfo> {
    let mut out = Vec::new();
    let mut seen_paths = BTreeSet::new();
    for line in content.lines() {
        let mut fields = line.split_whitespace();
        let (Some(raw_dev), Some(raw_mount), Some(fs_type)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let mount_str = unescape_proc_mount_field(raw_mount);
        let dev_str = unescape_proc_mount_field(raw_dev);
        let is_network = is_network_fs(fs_type);
        if !is_relevant_mount(&mount_str, fs_type, is_network) {
            continue;
        }
        let mount_path = PathBuf::from(&mount_str);
        if !seen_paths.insert(mount_path.clone()) {
            continue;
        }
        let name = if mount_str == "/" {
            String::from("RustyOS HD")
        } else {
            mount_path
                .file_name()
                .and_then(|n| n.to_str())
                .filter(|s| !s.is_empty())
                .map_or_else(|| mount_str.clone(), ToOwned::to_owned)
        };
        out.push(VolumeInfo {
            name,
            mount_point: mount_path,
            device: dev_str,
            fs_type: fs_type.to_owned(),
            is_network,
        });
    }
    out
}

fn is_network_fs(fs_type: &str) -> bool {
    matches!(
        fs_type,
        "nfs"
            | "nfs4"
            | "cifs"
            | "smb3"
            | "sshfs"
            | "fuse.sshfs"
            | "davfs"
            | "fuse.rclone"
            | "9p"
            | "fuse.gvfsd-fuse"
    )
}

fn is_relevant_mount(mount_point: &str, fs_type: &str, is_network: bool) -> bool {
    if is_network {
        return true;
    }
    if mount_point == "/" {
        return true;
    }
    let ignored_fs = matches!(
        fs_type,
        "proc"
            | "sysfs"
            | "devtmpfs"
            | "devpts"
            | "tmpfs"
            | "cgroup"
            | "cgroup2"
            | "pstore"
            | "securityfs"
            | "debugfs"
            | "tracefs"
            | "configfs"
            | "fusectl"
            | "hugetlbfs"
            | "mqueue"
            | "binfmt_misc"
            | "autofs"
            | "efivarfs"
            | "squashfs"
            | "ramfs"
            | "rpc_pipefs"
            | "nsfs"
            | "bpf"
    );
    if ignored_fs {
        return false;
    }
    mount_point.starts_with("/media/")
        || mount_point.starts_with("/run/media/")
        || mount_point.starts_with("/mnt/")
}

fn unescape_proc_mount_field(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\'
            && i + 3 < bytes.len()
            && is_octal_digit(bytes[i + 1])
            && is_octal_digit(bytes[i + 2])
            && is_octal_digit(bytes[i + 3])
        {
            let val =
                ((bytes[i + 1] - b'0') << 6) | ((bytes[i + 2] - b'0') << 3) | (bytes[i + 3] - b'0');
            out.push(val);
            i += 4;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

const fn is_octal_digit(b: u8) -> bool {
    b >= b'0' && b <= b'7'
}
