//! Standalone file manager backend services for `rusty-files` (`RUSTYOS-065`, ADR-0030).
//!
//! Provides XDG user directories, mount discovery from `/proc/mounts`, `FreeDesktop`
//! `.desktop` application discovery, recent files, `FreeDesktop` Trash specification
//! operations, local tag/preference persistence (`file_tags.toml`), sorting, and search.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::symlink;
use std::path::{Component, Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

/// Visual theme mode for `rusty-files` (`RUSTYOS-065`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FilesThemeMode {
    /// Golden Gate frosted light appearance (default).
    #[default]
    LightGoldenGate,
    /// RustyOS Purple Glass dark appearance (`DESIGN.md`).
    DarkPurpleGlass,
}

impl FilesThemeMode {
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
            "dark" | "dark_purple_glass" => Some(Self::DarkPurpleGlass),
            _ => None,
        }
    }
}

/// Content view modes presented in the Golden Gate view switcher pill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FileViewMode {
    /// Icon grid view (Stage B slot `RUSTYOS-066`).
    Icons,
    /// Multi-column sortable list view.
    List,
    /// Miller Columns hierarchical browser (default in Golden Gate Stage A).
    #[default]
    Columns,
    /// Large preview gallery view (Stage B slot `RUSTYOS-066`).
    Gallery,
}

impl FileViewMode {
    /// Returns whether this mode has active Stage A layout & rendering (`Columns`, `List`).
    #[must_use]
    pub const fn is_stage_a_supported(self) -> bool {
        matches!(self, Self::Columns | Self::List)
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

fn classify_extension_kind(name: &str) -> &'static str {
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
}

fn parse_xdg_user_dirs(content: &str, home: &Path) -> BTreeMap<String, PathBuf> {
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
                cfg.view_mode = if v.is_stage_a_supported() {
                    v
                } else {
                    FileViewMode::Columns
                };
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

/// Lists entries for any sidebar/navigation `ListingTarget`.
///
/// # Errors
/// Returns an `io::Error` if listing the target directory fails.
pub fn list_entries_for_target(
    target: &ListingTarget,
    locations: &UserFileLocations,
    tags: &FileTagsConfig,
) -> io::Result<Vec<FileEntry>> {
    match target {
        ListingTarget::Directory(path) => list_directory_at(path),
        ListingTarget::Applications => Ok(scan_applications_in(&locations.app_dirs)),
        ListingTarget::Recents => Ok(collect_recent_entries(locations)),
        ListingTarget::Shared => Ok(collect_shared_entries(locations)),
        ListingTarget::Network => Ok(collect_network_entries(locations)),
        ListingTarget::Trash => list_trash_entries(&locations.trash_dir),
        ListingTarget::Tag(color) => Ok(collect_tagged_entries(tags, *color)),
    }
}

/// Reads metadata for all entries immediately inside `dir`.
///
/// # Errors
/// Returns an `io::Error` if `dir` cannot be opened or read.
pub fn list_directory_at(dir: &Path) -> io::Result<Vec<FileEntry>> {
    let mut entries = Vec::new();
    for entry_res in fs::read_dir(dir)? {
        let Ok(entry) = entry_res else {
            continue;
        };
        let path = entry.path();
        if let Some(item) = build_file_entry(&path, None) {
            entries.push(item);
        }
    }
    Ok(entries)
}

fn build_file_entry(path: &Path, name_override: Option<String>) -> Option<FileEntry> {
    let sym_meta = fs::symlink_metadata(path).ok()?;
    let file_type = sym_meta.file_type();
    let name = name_override.unwrap_or_else(|| {
        path.file_name().map_or_else(
            || path.to_string_lossy().into_owned(),
            |n| n.to_string_lossy().into_owned(),
        )
    });
    let hidden = name.starts_with('.');
    let mut target_meta = sym_meta.clone();
    let symlink_target_is_dir = if file_type.is_symlink() {
        match fs::metadata(path) {
            Ok(resolved) => {
                let is_dir = resolved.is_dir();
                target_meta = resolved;
                is_dir
            }
            Err(_) => false,
        }
    } else {
        false
    };

    let kind = if file_type.is_dir() {
        FileEntryKind::Directory
    } else if file_type.is_symlink() {
        FileEntryKind::Symlink
    } else if file_type.is_file() {
        FileEntryKind::RegularFile
    } else {
        FileEntryKind::Other
    };
    let size_bytes = if target_meta.is_file() {
        target_meta.len()
    } else {
        0
    };
    let modified_unix_secs = target_meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));

    Some(FileEntry {
        name,
        path: path.to_path_buf(),
        kind,
        size_bytes,
        modified_unix_secs,
        hidden,
        symlink_target_is_dir,
        desktop_exec: None,
        trash_original_path: None,
        trash_deletion_date: None,
        subtitle: None,
    })
}

/// Discovers `.desktop` application entries across ordered search directories.
#[must_use]
pub fn scan_applications_in(dirs: &[PathBuf]) -> Vec<FileEntry> {
    let mut seen_ids = BTreeSet::new();
    let mut apps = Vec::new();
    for dir in dirs {
        let Ok(read_dir) = fs::read_dir(dir) else {
            continue;
        };
        for item in read_dir.flatten() {
            let path = item.path();
            let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !file_name.ends_with(".desktop") {
                continue;
            }
            if !seen_ids.insert(file_name.to_owned()) {
                continue;
            }
            let Ok(content) = fs::read_to_string(&path) else {
                continue;
            };
            if let Some(entry) = parse_desktop_entry(&path, &content) {
                apps.push(entry);
            }
        }
    }
    apps
}

fn parse_desktop_entry(path: &Path, content: &str) -> Option<FileEntry> {
    let mut in_main = false;
    let mut name: Option<String> = None;
    let mut exec: Option<String> = None;
    let mut entry_type: Option<String> = None;
    let mut hidden = false;

    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            in_main = line == "[Desktop Entry]";
            continue;
        }
        if !in_main {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        match k.trim() {
            "Name" if name.is_none() => name = Some(v.trim().to_owned()),
            "Exec" => exec = Some(v.trim().to_owned()),
            "Type" => entry_type = Some(v.trim().to_owned()),
            "NoDisplay" | "Hidden" if v.trim().eq_ignore_ascii_case("true") => hidden = true,
            _ => {}
        }
    }

    if hidden || entry_type.as_deref() != Some("Application") {
        return None;
    }
    let name = name?;
    let meta = fs::metadata(path).ok();
    let modified_unix_secs = meta
        .as_ref()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));

    Some(FileEntry {
        name,
        path: path.to_path_buf(),
        kind: FileEntryKind::Application,
        size_bytes: meta.map_or(0, |m| m.len()),
        modified_unix_secs,
        hidden: false,
        symlink_target_is_dir: false,
        desktop_exec: exec,
        trash_original_path: None,
        trash_deletion_date: None,
        subtitle: Some(String::from("Application")),
    })
}

fn collect_recent_entries(locations: &UserFileLocations) -> Vec<FileEntry> {
    let mut seen = BTreeSet::new();
    let mut results = Vec::new();
    let xbel_path = locations.data_home.join("recently-used.xbel");
    if let Ok(xml) = fs::read_to_string(&xbel_path) {
        for path in parse_recently_used_xbel_paths(&xml) {
            if results.len() >= 120 {
                break;
            }
            if seen.insert(path.clone())
                && let Some(entry) = build_file_entry(&path, None)
            {
                results.push(entry);
            }
        }
    }

    for folder in [
        &locations.documents,
        &locations.downloads,
        &locations.desktop,
    ] {
        collect_shallow_recent_files(folder, 1, &mut seen, &mut results);
    }
    results.sort_by_key(|a| std::cmp::Reverse(a.modified_unix_secs));
    results.truncate(120);
    results
}

fn collect_shallow_recent_files(
    dir: &Path,
    depth_remaining: u8,
    seen: &mut BTreeSet<PathBuf>,
    out: &mut Vec<FileEntry>,
) {
    let Ok(read_dir) = fs::read_dir(dir) else {
        return;
    };
    for entry_res in read_dir.flatten() {
        if out.len() >= 200 {
            return;
        }
        let path = entry_res.path();
        let Some(item) = build_file_entry(&path, None) else {
            continue;
        };
        if item.hidden {
            continue;
        }
        if item.kind == FileEntryKind::Directory {
            if depth_remaining > 0 {
                collect_shallow_recent_files(&path, depth_remaining - 1, seen, out);
            }
            continue;
        }
        if item.kind == FileEntryKind::RegularFile && seen.insert(path) {
            out.push(item);
        }
    }
}

fn parse_recently_used_xbel_paths(xml: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut rest = xml;
    while let Some(pos) = rest.find("href=\"file://") {
        let after = &rest[pos + "href=\"file://".len()..];
        let Some(end_quote) = after.find('"') else {
            break;
        };
        let encoded = &after[..end_quote];
        let decoded = percent_decode(encoded);
        let p = PathBuf::from(&decoded);
        if p.exists() {
            paths.push(p);
        }
        rest = &after[end_quote + 1..];
    }
    paths
}

fn collect_shared_entries(locations: &UserFileLocations) -> Vec<FileEntry> {
    let mut items = Vec::new();
    if locations.public_share.is_dir() {
        items.push(FileEntry {
            name: String::from("Public"),
            path: locations.public_share.clone(),
            kind: FileEntryKind::Directory,
            size_bytes: 0,
            modified_unix_secs: None,
            hidden: false,
            symlink_target_is_dir: false,
            desktop_exec: None,
            trash_original_path: None,
            trash_deletion_date: None,
            subtitle: Some(String::from("Public Share")),
        });
    }
    items.extend(collect_network_entries(locations));
    items
}

fn collect_network_entries(locations: &UserFileLocations) -> Vec<FileEntry> {
    let mut items = Vec::new();
    for vol in discover_mounts() {
        if !vol.is_network {
            continue;
        }
        items.push(FileEntry {
            name: vol.name,
            path: vol.mount_point,
            kind: FileEntryKind::MountPoint,
            size_bytes: 0,
            modified_unix_secs: None,
            hidden: false,
            symlink_target_is_dir: false,
            desktop_exec: None,
            trash_original_path: None,
            trash_deletion_date: None,
            subtitle: Some(format!("{} ({})", vol.device, vol.fs_type)),
        });
    }
    let uid_gvfs = locations
        .data_home
        .parent()
        .and_then(|_| env::var("XDG_RUNTIME_DIR").ok())
        .map(|rt| PathBuf::from(rt).join("gvfs"));
    if let Some(gvfs_dir) = uid_gvfs
        && let Ok(rd) = fs::read_dir(&gvfs_dir)
    {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir()
                && let Some(name) = p.file_name().and_then(|n| n.to_str())
            {
                items.push(FileEntry {
                    name: name.to_owned(),
                    path: p,
                    kind: FileEntryKind::MountPoint,
                    size_bytes: 0,
                    modified_unix_secs: None,
                    hidden: false,
                    symlink_target_is_dir: false,
                    desktop_exec: None,
                    trash_original_path: None,
                    trash_deletion_date: None,
                    subtitle: Some(String::from("Network Mount")),
                });
            }
        }
    }
    items
}

fn collect_tagged_entries(tags: &FileTagsConfig, color: FileTagColor) -> Vec<FileEntry> {
    tags.paths_with_tag(color)
        .into_iter()
        .filter_map(|p| build_file_entry(&p, None))
        .collect()
}

/// Lists `FreeDesktop` Trash items from `<trash_dir>/files` joined with `.trashinfo`.
///
/// # Errors
/// Returns an `io::Error` if `<trash_dir>/files` exists and cannot be read.
pub fn list_trash_entries(trash_dir: &Path) -> io::Result<Vec<FileEntry>> {
    let files_dir = trash_dir.join("files");
    if !files_dir.exists() {
        return Ok(Vec::new());
    }
    let info_dir = trash_dir.join("info");
    let mut items = Vec::new();
    for entry_res in fs::read_dir(&files_dir)? {
        let Ok(entry) = entry_res else {
            continue;
        };
        let path = entry.path();
        let Some(name_os) = path.file_name() else {
            continue;
        };
        let name = name_os.to_string_lossy().into_owned();
        let Some(mut item) = build_file_entry(&path, Some(name.clone())) else {
            continue;
        };
        let info_path = info_dir.join(format!("{name}.trashinfo"));
        if let Ok(info_text) = fs::read_to_string(&info_path) {
            let (orig, del_date) = parse_trashinfo(&info_text);
            item.trash_original_path = orig;
            item.trash_deletion_date = del_date;
        }
        items.push(item);
    }
    Ok(items)
}

fn parse_trashinfo(content: &str) -> (Option<PathBuf>, Option<String>) {
    let mut orig = None;
    let mut del_date = None;
    for raw in content.lines() {
        let line = raw.trim();
        if let Some(v) = line.strip_prefix("Path=") {
            let decoded = percent_decode(v);
            if !decoded.is_empty() {
                orig = Some(PathBuf::from(decoded));
            }
        } else if let Some(v) = line.strip_prefix("DeletionDate=") {
            del_date = Some(v.to_owned());
        }
    }
    (orig, del_date)
}

/// Applies hidden-file filter, case-insensitive substring query, and sort ordering.
#[must_use]
pub fn filter_and_sort_entries(
    mut entries: Vec<FileEntry>,
    show_hidden: bool,
    query: &str,
    sort_field: FileSortField,
    sort_order: FileSortOrder,
) -> Vec<FileEntry> {
    let needle = query.trim().to_ascii_lowercase();
    entries.retain(|entry| {
        if !show_hidden && entry.hidden {
            return false;
        }
        if needle.is_empty() {
            return true;
        }
        entry.name.to_ascii_lowercase().contains(&needle)
    });

    entries.sort_by(|a, b| compare_entries(a, b, sort_field, sort_order));
    entries
}

fn compare_entries(
    a: &FileEntry,
    b: &FileEntry,
    field: FileSortField,
    order: FileSortOrder,
) -> std::cmp::Ordering {
    let a_dir = a.is_navigable_dir();
    let b_dir = b.is_navigable_dir();
    let dir_cmp = b_dir.cmp(&a_dir);
    if dir_cmp != std::cmp::Ordering::Equal {
        return dir_cmp;
    }

    let primary = match field {
        FileSortField::Name => a
            .name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
            .then_with(|| a.name.cmp(&b.name)),
        FileSortField::DateModified => a
            .modified_unix_secs
            .unwrap_or(0)
            .cmp(&b.modified_unix_secs.unwrap_or(0)),
        FileSortField::Size => a.size_bytes.cmp(&b.size_bytes),
        FileSortField::Kind => a.kind_label().cmp(b.kind_label()).then_with(|| {
            a.name
                .to_ascii_lowercase()
                .cmp(&b.name.to_ascii_lowercase())
        }),
    };
    let ordered = match order {
        FileSortOrder::Ascending => primary,
        FileSortOrder::Descending => primary.reverse(),
    };
    ordered.then_with(|| {
        a.name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
    })
}

/// Validates a single folder/file basename (no path separators, `.`, `..`, or NUL).
///
/// # Errors
/// Returns a static diagnostic message if invalid.
pub fn validate_single_component_name(raw: &str) -> Result<&str, &'static str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Name must not be empty");
    }
    if trimmed == "." || trimmed == ".." {
        return Err("Reserved name '.' or '..' is not allowed");
    }
    if trimmed.contains('/') || trimmed.contains('\0') {
        return Err("Path separators are not allowed in entry names");
    }
    Ok(trimmed)
}

/// Creates a uniquely named directory inside `parent` (`<base_name>`, `<base_name> 2`, ...).
///
/// # Errors
/// Returns an `io::Error` if validation fails or directory creation cannot succeed.
pub fn create_unique_folder(parent: &Path, base_name: &str) -> io::Result<PathBuf> {
    let clean = validate_single_component_name(base_name)
        .map_err(|msg| io::Error::new(io::ErrorKind::InvalidInput, msg))?;
    for idx in 1..=9999_u32 {
        let candidate_name = if idx == 1 {
            clean.to_owned()
        } else {
            format!("{clean} {idx}")
        };
        let target = parent.join(&candidate_name);
        match fs::create_dir(&target) {
            Ok(()) => return Ok(target),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(err),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "Exhausted folder name attempts",
    ))
}

/// Moves `source` into the `FreeDesktop` Trash directory (`files/` + `info/<name>.trashinfo`).
///
/// Handles cross-filesystem moves safely without following symlinks and rejects moving `/`
/// or items already inside `trash_dir`.
///
/// # Errors
/// Returns an `io::Error` if the path is invalid, protected, or filesystem operations fail.
pub fn move_to_trash(source: &Path, trash_dir: &Path) -> io::Result<PathBuf> {
    if source == Path::new("/") {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Refusing to trash filesystem root",
        ));
    }
    if source == trash_dir || source.starts_with(trash_dir) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Path is already inside Trash",
        ));
    }
    let meta = fs::symlink_metadata(source)?;
    let orig_name = source
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Missing file name"))?;

    let files_dir = trash_dir.join("files");
    let info_dir = trash_dir.join("info");
    fs::create_dir_all(&files_dir)?;
    fs::create_dir_all(&info_dir)?;

    let abs_source = absolutize_path(source)?;
    let (trash_name, info_path) =
        reserve_trash_slot(&files_dir, &info_dir, orig_name, &abs_source)?;
    let dest = files_dir.join(&trash_name);

    let move_res = move_entry_no_follow(source, &dest, meta.file_type().is_dir());
    if let Err(err) = move_res {
        let _ = fs::remove_file(&info_path);
        return Err(err);
    }
    Ok(dest)
}

fn reserve_trash_slot(
    files_dir: &Path,
    info_dir: &Path,
    orig_name: &str,
    abs_source: &Path,
) -> io::Result<(String, PathBuf)> {
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0_i64, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
    let del_date = format_iso8601_utc(now_secs);
    let encoded_orig = percent_encode_path(&abs_source.to_string_lossy());
    let trashinfo_body = format!("[Trash Info]\nPath={encoded_orig}\nDeletionDate={del_date}\n");

    for attempt in 1..=9999_u32 {
        let candidate = if attempt == 1 {
            orig_name.to_owned()
        } else {
            format!("{orig_name}.{attempt}")
        };
        let dest_path = files_dir.join(&candidate);
        if dest_path.symlink_metadata().is_ok() {
            continue;
        }
        let info_path = info_dir.join(format!("{candidate}.trashinfo"));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&info_path)
        {
            Ok(mut f) => {
                if let Err(err) = f
                    .write_all(trashinfo_body.as_bytes())
                    .and_then(|()| f.flush())
                {
                    let _ = fs::remove_file(&info_path);
                    return Err(err);
                }
                return Ok((candidate, info_path));
            }
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(err),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "Exhausted unique Trash entry names",
    ))
}

fn move_entry_no_follow(source: &Path, dest: &Path, is_dir: bool) -> io::Result<()> {
    match fs::rename(source, dest) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::CrossesDevices => {
            if let Err(copy_err) = copy_recursive_no_follow(source, dest) {
                let _ = remove_path_no_follow(dest);
                return Err(copy_err);
            }
            if is_dir {
                fs::remove_dir_all(source)?;
            } else {
                fs::remove_file(source)?;
            }
            Ok(())
        }
        Err(err) => Err(err),
    }
}

fn copy_recursive_no_follow(source: &Path, dest: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(source)?;
    let ft = meta.file_type();
    if ft.is_symlink() {
        let target = fs::read_link(source)?;
        symlink(&target, dest)?;
    } else if ft.is_dir() {
        fs::create_dir_all(dest)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            let child_src = entry.path();
            let child_dest = dest.join(entry.file_name());
            copy_recursive_no_follow(&child_src, &child_dest)?;
        }
    } else if ft.is_file() {
        fs::copy(source, dest)?;
    } else {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Cannot move special filesystem node across devices",
        ));
    }
    Ok(())
}

fn remove_path_no_follow(path: &Path) -> io::Result<()> {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return Ok(());
    };
    if meta.file_type().is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

/// Restores a trashed entry named `trash_entry_name` back to its `.trashinfo` original path
/// (or a non-colliding sibling if occupied).
///
/// # Errors
/// Returns an `io::Error` if the `.trashinfo` file is missing/malformed or move fails.
pub fn restore_from_trash(trash_entry_name: &str, trash_dir: &Path) -> io::Result<PathBuf> {
    let clean = validate_single_component_name(trash_entry_name)
        .map_err(|msg| io::Error::new(io::ErrorKind::InvalidInput, msg))?;
    let trashed_path = trash_dir.join("files").join(clean);
    let info_path = trash_dir.join("info").join(format!("{clean}.trashinfo"));
    let meta = fs::symlink_metadata(&trashed_path)?;
    let info_text = fs::read_to_string(&info_path)?;
    let (orig_opt, _) = parse_trashinfo(&info_text);
    let orig_path = orig_opt.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "Missing original Path= in .trashinfo",
        )
    })?;

    if let Some(parent) = orig_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let final_dest = pick_non_colliding_restore_path(&orig_path);
    move_entry_no_follow(&trashed_path, &final_dest, meta.file_type().is_dir())?;
    let _ = fs::remove_file(&info_path);
    Ok(final_dest)
}

fn pick_non_colliding_restore_path(orig_path: &Path) -> PathBuf {
    if orig_path.symlink_metadata().is_err() {
        return orig_path.to_path_buf();
    }
    let parent = orig_path.parent().unwrap_or_else(|| Path::new("/"));
    let stem = orig_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("restored");
    let ext = orig_path.extension().and_then(|e| e.to_str());
    for n in 1..=999_u32 {
        let suffix = if n == 1 {
            String::from(" (restored)")
        } else {
            format!(" (restored {n})")
        };
        let file_name = match ext {
            Some(e) => format!("{stem}{suffix}.{e}"),
            None => format!("{stem}{suffix}"),
        };
        let candidate = parent.join(file_name);
        if candidate.symlink_metadata().is_err() {
            return candidate;
        }
    }
    orig_path.to_path_buf()
}

/// Permanently deletes all contents inside `<trash_dir>/files` and `<trash_dir>/info`.
///
/// Returns the number of trashed items deleted.
///
/// # Errors
/// Returns an `io::Error` if iterating or deleting entries fails.
pub fn empty_trash(trash_dir: &Path) -> io::Result<usize> {
    let files_dir = trash_dir.join("files");
    let info_dir = trash_dir.join("info");
    let mut removed = 0_usize;

    if files_dir.is_dir() {
        for entry_res in fs::read_dir(&files_dir)? {
            let entry = entry_res?;
            remove_path_no_follow(&entry.path())?;
            removed += 1;
        }
    }
    if info_dir.is_dir() {
        for entry_res in fs::read_dir(&info_dir)? {
            let entry = entry_res?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("trashinfo") {
                remove_path_no_follow(&path)?;
            }
        }
    }
    Ok(removed)
}

fn absolutize_path(path: &Path) -> io::Result<PathBuf> {
    let raw = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()?.join(path)
    };
    let mut clean = PathBuf::new();
    for comp in raw.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                clean.pop();
            }
            other => clean.push(other.as_os_str()),
        }
    }
    Ok(clean)
}

fn percent_encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for byte in path.as_bytes() {
        let safe = byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.' | b'~');
        if safe {
            out.push(*byte as char);
        } else {
            out.push('%');
            out.push(hex_upper(byte >> 4));
            out.push(hex_upper(byte & 0x0F));
        }
    }
    out
}

const fn hex_upper(nibble: u8) -> char {
    match nibble & 0x0F {
        0..=9 => (b'0' + (nibble & 0x0F)) as char,
        10..=15 => (b'A' + ((nibble & 0x0F) - 10)) as char,
        _ => '0',
    }
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) =
                (from_hex_digit(bytes[i + 1]), from_hex_digit(bytes[i + 2]))
        {
            out.push((hi << 4) | lo);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

const fn from_hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Formats byte sizes in macOS decimal Finder units (`1 KB = 1000 B`).
#[must_use]
pub fn format_byte_size(bytes: u64) -> String {
    const KB: u64 = 1_000;
    const MB: u64 = 1_000_000;
    const GB: u64 = 1_000_000_000;
    if bytes < KB {
        format!("{bytes} B")
    } else if bytes < MB {
        let tenths = (bytes * 10) / KB;
        format!("{}.{} KB", tenths / 10, tenths % 10)
    } else if bytes < GB {
        let tenths = (bytes * 10) / MB;
        format!("{}.{} MB", tenths / 10, tenths % 10)
    } else {
        let tenths = (bytes * 10) / GB;
        format!("{}.{} GB", tenths / 10, tenths % 10)
    }
}

/// Formats Unix seconds into ISO-like UTC date-time (`YYYY-MM-DD HH:MM`) using pure arithmetic.
#[must_use]
pub fn format_unix_timestamp(unix_secs: i64) -> String {
    let (y, m, d, hh, mm, _ss) = decompose_unix_utc(unix_secs);
    format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}")
}

fn format_iso8601_utc(unix_secs: i64) -> String {
    let (y, m, d, hh, mm, ss) = decompose_unix_utc(unix_secs);
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}")
}

fn decompose_unix_utc(unix_secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    let clamped = unix_secs.max(0);
    let days = clamped.div_euclid(86_400);
    let secs_of_day = u32::try_from(clamped.rem_euclid(86_400)).unwrap_or(0);
    let hh = secs_of_day / 3_600;
    let mm = (secs_of_day % 3_600) / 60;
    let ss = secs_of_day % 60;

    // Howard Hinnant `civil_from_days` algorithm (pure safe integer math).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let m_signed = mp + if mp < 10 { 3 } else { -9 };
    let y_adj = y + i64::from(m_signed <= 2);
    let m = u32::try_from(m_signed).unwrap_or(1);
    (y_adj, m, d, hh, mm, ss)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDirGuard {
        path: PathBuf,
    }

    impl TempDirGuard {
        fn new(label: &str) -> io::Result<Self> {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let dir = env::temp_dir().join(format!(
                "rustyos-fm-test-{label}-{}-{unique}",
                process::id()
            ));
            fs::create_dir_all(&dir)?;
            Ok(Self { path: dir })
        }
    }

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn proc_mounts_parser_filters_pseudo_fs_and_decodes_spaces() {
        let sample = "\
proc /proc proc rw,nosuid,nodev,noexec,relatime 0 0
/dev/nvme0n1p2 / ext4 rw,relatime 0 0
tmpfs /run tmpfs rw,nosuid,nodev 0 0
/dev/sdb1 /run/media/anc/My\\040Drive exfat rw,nosuid 0 0
//nas.local/media /mnt/iNAS cifs rw,username=anc 0 0
";
        let vols = parse_proc_mounts(sample);
        assert_eq!(vols.len(), 3);
        assert_eq!(vols[0].mount_point, PathBuf::from("/"));
        assert_eq!(vols[0].name, "RustyOS HD");
        assert!(!vols[0].is_network);

        assert_eq!(
            vols[1].mount_point,
            PathBuf::from("/run/media/anc/My Drive")
        );
        assert_eq!(vols[1].name, "My Drive");
        assert!(!vols[1].is_network);

        assert_eq!(vols[2].name, "iNAS");
        assert!(vols[2].is_network);
    }

    #[test]
    fn tags_config_roundtrip_and_toggle_work() -> Result<(), Box<dyn std::error::Error>> {
        let guard = TempDirGuard::new("tags-cfg")?;
        let cfg_path = guard.path.join("rustyos/file_tags.toml");

        let mut cfg = FileTagsConfig {
            theme: FilesThemeMode::DarkPurpleGlass,
            view_mode: FileViewMode::List,
            sort_field: FileSortField::Size,
            sort_order: FileSortOrder::Descending,
            show_hidden: true,
            ..Default::default()
        };

        let target = PathBuf::from("/home/anc/Documents/Quote \"A\"\\Draft.txt");
        assert!(cfg.toggle_tag(&target, FileTagColor::Blue));
        assert!(cfg.toggle_tag(&target, FileTagColor::Red));
        assert_eq!(
            cfg.tags_for(&target),
            &[FileTagColor::Red, FileTagColor::Blue]
        );
        assert_eq!(cfg.paths_with_tag(FileTagColor::Blue), vec![target.clone()]);

        cfg.save_to_path(&cfg_path)?;
        let loaded = FileTagsConfig::load_from_path(&cfg_path);
        assert_eq!(loaded, cfg);

        let mut mutated = loaded;
        assert!(!mutated.toggle_tag(&target, FileTagColor::Red));
        assert_eq!(mutated.tags_for(&target), &[FileTagColor::Blue]);
        assert!(!mutated.toggle_tag(&target, FileTagColor::Blue));
        assert_eq!(mutated.tags_for(&target), []);
        Ok(())
    }

    #[test]
    fn xdg_user_dirs_and_applications_discovery_work() -> Result<(), Box<dyn std::error::Error>> {
        let guard = TempDirGuard::new("xdg-apps")?;
        let home = guard.path.join("home/shackett");
        let cfg_root = guard.path.join("cfg");
        let data_home = guard.path.join("data");
        let user_apps = data_home.join("applications");
        let sys_apps = guard.path.join("sys_applications");
        fs::create_dir_all(&home)?;
        fs::create_dir_all(&user_apps)?;
        fs::create_dir_all(&sys_apps)?;

        let parsed = parse_xdg_user_dirs(
            "XDG_DESKTOP_DIR=\"$HOME/MyDesktop\"\nXDG_DOWNLOAD_DIR=\"$HOME/DL\"\n",
            &home,
        );
        assert_eq!(parsed.get("XDG_DESKTOP_DIR"), Some(&home.join("MyDesktop")));
        assert_eq!(parsed.get("XDG_DOWNLOAD_DIR"), Some(&home.join("DL")));

        fs::write(
            user_apps.join("editor.desktop"),
            "[Desktop Entry]\nType=Application\nName=RustyEdit User\nExec=rusty-edit %F\n",
        )?;
        fs::write(
            sys_apps.join("editor.desktop"),
            "[Desktop Entry]\nType=Application\nName=RustyEdit Sys\nExec=old-edit\n",
        )?;
        fs::write(
            sys_apps.join("hidden.desktop"),
            "[Desktop Entry]\nType=Application\nName=Hidden Helper\nNoDisplay=true\nExec=noop\n",
        )?;

        let mut locs = UserFileLocations::with_roots(&home, &cfg_root, &data_home);
        locs.app_dirs = vec![user_apps, sys_apps];
        let apps = list_entries_for_target(
            &ListingTarget::Applications,
            &locs,
            &FileTagsConfig::default(),
        )?;
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].name, "RustyEdit User");
        assert_eq!(apps[0].kind, FileEntryKind::Application);
        assert_eq!(apps[0].desktop_exec.as_deref(), Some("rusty-edit %F"));
        Ok(())
    }

    #[test]
    fn create_folder_trash_restore_and_empty_work() -> Result<(), Box<dyn std::error::Error>> {
        let guard = TempDirGuard::new("trash-ops")?;
        let work = guard.path.join("work");
        let trash = guard.path.join("Trash");
        fs::create_dir_all(&work)?;

        let f1 = create_unique_folder(&work, "untitled folder")?;
        let f2 = create_unique_folder(&work, "untitled folder")?;
        assert!(f1.ends_with("untitled folder"));
        assert!(f2.ends_with("untitled folder 2"));
        assert!(validate_single_component_name("bad/name").is_err());
        assert!(validate_single_component_name("..").is_err());

        let doc = work.join("hello world.txt");
        fs::write(&doc, b"golden gate")?;
        let trashed_doc = move_to_trash(&doc, &trash)?;
        assert!(!doc.exists());
        assert!(trashed_doc.exists());

        let trash_items = list_trash_entries(&trash)?;
        assert_eq!(trash_items.len(), 1);
        assert_eq!(trash_items[0].name, "hello world.txt");
        assert_eq!(trash_items[0].trash_original_path.as_ref(), Some(&doc));

        let restored = restore_from_trash("hello world.txt", &trash)?;
        assert_eq!(restored, doc);
        assert_eq!(fs::read(&doc)?, b"golden gate");
        assert_eq!(list_trash_entries(&trash)?, []);

        move_to_trash(&f2, &trash)?;
        assert_eq!(list_trash_entries(&trash)?.len(), 1);
        let deleted = empty_trash(&trash)?;
        assert_eq!(deleted, 1);
        assert_eq!(list_trash_entries(&trash)?, []);
        assert!(trash.exists());
        Ok(())
    }

    #[test]
    fn sort_filter_and_formatters_work() -> Result<(), Box<dyn std::error::Error>> {
        let guard = TempDirGuard::new("sort-fmt")?;
        fs::create_dir_all(guard.path.join("alpha_dir"))?;
        fs::write(guard.path.join(".secret"), b"hi")?;
        fs::write(guard.path.join("beta.rs"), b"1234567890")?;
        fs::write(guard.path.join("Alpha.txt"), b"123")?;

        let raw = list_directory_at(&guard.path)?;
        let visible_name = filter_and_sort_entries(
            raw.clone(),
            false,
            "",
            FileSortField::Name,
            FileSortOrder::Ascending,
        );
        let names: Vec<&str> = visible_name.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["alpha_dir", "Alpha.txt", "beta.rs"]);

        let filtered = filter_and_sort_entries(
            raw,
            true,
            "sec",
            FileSortField::Size,
            FileSortOrder::Descending,
        );
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, ".secret");

        assert_eq!(format_byte_size(512), "512 B");
        assert_eq!(format_byte_size(1_500), "1.5 KB");
        assert_eq!(format_byte_size(2_400_000), "2.4 MB");
        assert_eq!(format_unix_timestamp(0), "1970-01-01 00:00");
        assert_eq!(format_unix_timestamp(1_763_114_400), "2025-11-14 10:00");
        Ok(())
    }
}
