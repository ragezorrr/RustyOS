//! Comprehensive application state machine and coordination for `rusty-files` (`RUSTYOS-065`, ADR-0030, ADR-0044).
//!
//! Submodules manage distinct functional domains:
//! - `types`: shared structures, enums and component state models;
//! - `tabs`: multi-tab state, snapshots, and tab-bar visibility;
//! - `navigation`: folder tree traversal, sidebar navigation, column pane management;
//! - `dialogs`: modal overlays (Quick Look, Get Info, Go to Folder, Rename, Transfer Conflict);
//! - `clipboard`: Cut/Copy/Paste, duplicate, trash moves, background transfers;
//! - `search`: search indexing, tagging, asset preview cache;
//! - `input`: keyboard shortcuts, smooth scroll physics, animations, caret blinking.

#![allow(clippy::wildcard_imports)]

pub(crate) mod clipboard;
pub(crate) mod dialogs;
pub(crate) mod input;
pub(crate) mod navigation;
pub(crate) mod search;
pub(crate) mod tabs;
pub(crate) mod types;

pub(crate) use navigation::*;
pub(crate) use types::*;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Instant, SystemTime};

use rusty_services::file_manager::{
    FileDetailedInfo, FileEntry, FileSortField, FileSortOrder, FileTagColor, FileTagsConfig,
    FileViewMode, FilesThemeMode, ListingTarget, SidebarLocation, UserFileLocations, VolumeInfo,
    discover_mounts,
};

#[allow(clippy::struct_excessive_bools)]
pub(crate) struct FilesAppState {
    pub locations: UserFileLocations,
    pub tags_config: FileTagsConfig,
    pub volumes: Vec<VolumeInfo>,
    pub removable_volumes: Vec<rusty_services::RemovableVolume>,
    pub file_drag: Option<FileDragState>,
    pub sidebar_items: Vec<SidebarRowItem>,
    pub collapsed_sidebar_sections: BTreeSet<&'static str>,
    pub hovered_hit: Option<crate::layout::FilesHit>,
    pub active_sidebar: Option<SidebarLocation>,
    pub view_mode: FileViewMode,
    pub theme: FilesThemeMode,
    pub accent_index: usize,
    pub(crate) theme_locked: bool,
    pub(crate) last_config_mtime: Option<SystemTime>,
    pub sort_field: FileSortField,
    pub sort_order: FileSortOrder,
    pub show_hidden: bool,
    pub show_path_bar: bool,
    pub search_active: bool,
    pub search_query: String,
    pub search_scope: SearchScope,
    pub(crate) saved_root_raw: Option<(ListingTarget, Vec<FileEntry>)>,
    pub(crate) pending_home_search: bool,
    pub(crate) home_search_tx: mpsc::Sender<HomeSearchResult>,
    pub(crate) home_search_rx: mpsc::Receiver<HomeSearchResult>,
    pub last_scroll_activity: Option<Instant>,
    pub overlay_scrollbar_alpha: f32,
    pub columns: Vec<ColumnPane>,
    pub active_column: usize,
    pub column_scroll_x: i32,
    pub list_scroll_y: i32,
    pub icons_scroll_y: i32,
    pub gallery_strip_scroll_x: i32,
    pub last_icon_grid_cols: usize,
    pub last_content_w: i32,
    pub follow_right_column: bool,
    pub sidebar_scroll_y: i32,
    pub(crate) column_scroll_x_anim: SmoothAxis,
    pub(crate) list_scroll_anim: SmoothAxis,
    pub(crate) icons_scroll_anim: SmoothAxis,
    pub(crate) gallery_strip_scroll_anim: SmoothAxis,
    pub(crate) sidebar_scroll_anim: SmoothAxis,
    pub(crate) last_anim_tick: Option<Instant>,
    pub caret_visible: bool,
    pub(crate) last_caret_tick: Instant,
    pub epoch: Instant,
    pub search_field: rusty_ui::TextField,
    pub rename_field: rusty_ui::TextField,
    pub goto_field: rusty_ui::TextField,
    pub focus: rusty_ui::FocusScope,
    pub back_stack: Vec<NavigationHistoryEntry>,
    pub forward_stack: Vec<NavigationHistoryEntry>,
    pub tabs: Vec<FileTabState>,
    pub active_tab: usize,
    pub show_tab_bar: bool,
    pub gallery_more_info: bool,
    pub popup: Option<OpenPopupMenu>,
    pub clipboard: Option<ClipboardItemSet>,
    pub quick_look: Option<QuickLookState>,
    pub get_info: Option<GetInfoState>,
    pub go_to_folder: Option<GoToFolderState>,
    pub inline_rename: Option<InlineRenameState>,
    pub active_transfer: Option<ActiveTransferState>,
    pub status_message: Option<String>,
    pub active_dir_avail_bytes: Option<u64>,
    pub picker_mode: Option<FilePickerMode>,
    pub picker_title: Option<String>,
    pub picker_filename: String,
    pub picker_filename_focused: bool,
    pub picker_filename_field: rusty_ui::TextField,
    pub picker_confirmed: bool,
    pub picker_cancelled: bool,
    pub window_title: String,
    pub window_size: WindowSizeState,
    pub focused: bool,
    pub reduce_transparency: bool,
    pub traffic_hovered: bool,
    pub dirty: bool,
    pub run_state: AppRunState,
    pub loader_gen: u64,
    pub(crate) result_tx: mpsc::Sender<LoadJobResult>,
    pub(crate) result_rx: mpsc::Receiver<LoadJobResult>,
    pub(crate) preview_cache: Vec<PreviewCacheEntry>,
    pub(crate) pending_previews: BTreeSet<PreviewCacheKey>,
    pub(crate) preview_tx: mpsc::Sender<PreviewJobResult>,
    pub(crate) preview_rx: mpsc::Receiver<PreviewJobResult>,
    pub(crate) cached_info: Option<(PathBuf, Result<FileDetailedInfo, String>)>,
    pub(crate) pending_info_path: Option<PathBuf>,
    pub(crate) info_tx: mpsc::Sender<InfoJobResult>,
    pub(crate) info_rx: mpsc::Receiver<InfoJobResult>,
    pub(crate) transfer_tx: mpsc::Sender<TransferWorkerMsg>,
    pub(crate) transfer_rx: mpsc::Receiver<TransferWorkerMsg>,
    pub(crate) archive_tx: mpsc::Sender<ArchiveJobResult>,
    pub(crate) archive_rx: mpsc::Receiver<ArchiveJobResult>,
    pub(crate) active_archive_jobs: usize,
    pub(crate) worker_ping: Option<smithay_client_toolkit::reexports::calloop::ping::Ping>,
}

impl FilesAppState {
    /// Creates application state using XDG locations, persisted `file_tags.toml`, and CLI options.
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub(crate) fn new(locations: UserFileLocations, opts: &StartupOptions) -> Self {
        let tags_config = FileTagsConfig::load_from_path(&locations.tags_config_path());
        let volumes = discover_mounts();
        let removable_volumes = rusty_services::StorageService::list_removable_volumes();
        let (result_tx, result_rx) = mpsc::channel();
        let (preview_tx, preview_rx) = mpsc::channel();
        let (info_tx, info_rx) = mpsc::channel();
        let (transfer_tx, transfer_rx) = mpsc::channel();
        let (home_search_tx, home_search_rx) = mpsc::channel();
        let (archive_tx, archive_rx) = mpsc::channel();
        let sidebar_items = build_sidebar_items(&locations, &volumes, &removable_volumes);
        let theme = opts
            .theme_override
            .or_else(|| locations.detect_system_theme())
            .unwrap_or(tags_config.theme);
        let accent_index = locations.detect_system_accent().unwrap_or(0);
        let reduce_transparency = locations.detect_reduce_transparency().unwrap_or(false);
        let mut app = Self {
            sort_field: tags_config.sort_field,
            sort_order: tags_config.sort_order,
            show_hidden: tags_config.show_hidden,
            show_path_bar: tags_config.show_path_bar,
            view_mode: tags_config.view_mode,
            locations,
            tags_config,
            volumes,
            sidebar_items,
            collapsed_sidebar_sections: BTreeSet::new(),
            hovered_hit: None,
            active_sidebar: None,
            theme,
            accent_index,
            theme_locked: opts.theme_override.is_some(),
            last_config_mtime: rustyos_config_mtime(),
            search_active: false,
            search_query: String::new(),
            search_scope: SearchScope::ThisFolder,
            saved_root_raw: None,
            pending_home_search: false,
            home_search_tx,
            home_search_rx,
            last_scroll_activity: None,
            overlay_scrollbar_alpha: 0.0,
            columns: Vec::new(),
            active_column: 0,
            column_scroll_x: 0,
            list_scroll_y: 0,
            icons_scroll_y: 0,
            gallery_strip_scroll_x: 0,
            last_icon_grid_cols: 5,
            last_content_w: 836,
            follow_right_column: true,
            sidebar_scroll_y: 0,
            column_scroll_x_anim: SmoothAxis::default(),
            list_scroll_anim: SmoothAxis::default(),
            icons_scroll_anim: SmoothAxis::default(),
            gallery_strip_scroll_anim: SmoothAxis::default(),
            sidebar_scroll_anim: SmoothAxis::default(),
            last_anim_tick: None,
            caret_visible: true,
            last_caret_tick: Instant::now(),
            epoch: Instant::now(),
            search_field: rusty_ui::TextField::search(rusty_ui::Rect::new(0, 0, 180, 32), "Search"),
            rename_field: Self::default_rename_field(),
            goto_field: Self::default_goto_field(),
            focus: rusty_ui::FocusScope::new(),
            back_stack: Vec::new(),
            forward_stack: Vec::new(),
            tabs: Vec::new(),
            active_tab: 0,
            show_tab_bar: false,
            gallery_more_info: false,
            popup: None,
            clipboard: None,
            quick_look: None,
            get_info: None,
            go_to_folder: None,
            inline_rename: None,
            active_transfer: None,
            removable_volumes,
            file_drag: None,
            status_message: None,
            active_dir_avail_bytes: None,
            picker_mode: opts.picker_mode.clone(),
            picker_title: opts.picker_title.clone(),
            picker_filename: match &opts.picker_mode {
                Some(FilePickerMode::SaveFile { default_name }) => default_name.clone(),
                _ => String::new(),
            },
            picker_filename_focused: matches!(
                opts.picker_mode,
                Some(FilePickerMode::SaveFile { .. })
            ),
            picker_filename_field: {
                let mut field =
                    rusty_ui::TextField::new(rusty_ui::Rect::new(0, 0, 200, 26), "File name");
                if let Some(FilePickerMode::SaveFile { default_name }) = &opts.picker_mode {
                    field.buffer.set_text(default_name);
                }
                field
            },
            picker_confirmed: false,
            picker_cancelled: false,
            window_title: opts
                .picker_title
                .clone()
                .unwrap_or_else(|| String::from("Files")),
            window_size: WindowSizeState::Normal,
            focused: true,
            reduce_transparency,
            traffic_hovered: false,
            dirty: true,
            run_state: AppRunState::Running,
            loader_gen: 1,
            result_tx,
            result_rx,
            preview_cache: Vec::new(),
            pending_previews: BTreeSet::new(),
            preview_tx,
            preview_rx,
            cached_info: None,
            pending_info_path: None,
            info_tx,
            info_rx,
            transfer_tx,
            transfer_rx,
            archive_tx,
            archive_rx,
            active_archive_jobs: 0,
            worker_ping: None,
        };

        app.apply_startup_options(opts);
        app
    }

    fn default_rename_field() -> rusty_ui::TextField {
        let mut f = rusty_ui::TextField::new(rusty_ui::Rect::new(0, 0, 220, 26), "Name");
        f.font_size = 12.5;
        f.corner_radius = 6.0;
        f.padding_x = 6;
        f
    }

    fn default_goto_field() -> rusty_ui::TextField {
        let mut f = rusty_ui::TextField::new(rusty_ui::Rect::new(0, 0, 380, 32), "~/");
        f.font_size = 13.0;
        f.corner_radius = 8.0;
        f.padding_x = 10;
        f
    }

    fn apply_startup_options(&mut self, opts: &StartupOptions) {
        if opts.open_trash {
            self.navigate_sidebar(SidebarLocation::Trash, false);
        } else if opts.open_recents {
            self.navigate_sidebar(SidebarLocation::Recents, false);
        } else if let Some(path) = &opts.initial_path {
            self.open_initial_path(path);
        } else {
            self.navigate_sidebar(SidebarLocation::Home, false);
        }
        self.tabs.push(self.snapshot_active_tab());
        if opts.open_test_popup {
            self.popup = Some(OpenPopupMenu::ActionsMenu);
            self.dirty = true;
        }
    }

    /// True while event loop should stay alive.
    #[must_use]
    pub(crate) const fn is_running(&self) -> bool {
        matches!(self.run_state, AppRunState::Running)
    }

    /// Marks the window to exit cleanly on the next event loop tick.
    pub(crate) fn request_quit(&mut self) {
        self.run_state = AppRunState::Exiting;
    }

    /// Reloads system `Light` / `Dark` theme and `[ui].accent_index` if `~/.config/rustyos/config.toml` changed.
    pub(crate) fn poll_system_theme(&mut self) -> bool {
        let mtime = rustyos_config_mtime();
        if mtime.is_some() && mtime != self.last_config_mtime {
            self.last_config_mtime = mtime;
            let mut changed = false;
            if !self.theme_locked
                && let Some(next) = self.locations.detect_system_theme()
                && next != self.theme
            {
                self.theme = next;
                changed = true;
            }
            if let Some(next_accent) = self.locations.detect_system_accent()
                && next_accent != self.accent_index
            {
                self.accent_index = next_accent;
                changed = true;
            }
            if let Some(next_rt) = self.locations.detect_reduce_transparency()
                && next_rt != self.reduce_transparency
            {
                self.reduce_transparency = next_rt;
                changed = true;
            }
            if changed {
                self.dirty = true;
                return true;
            }
        }
        false
    }

    /// True if compositor or traffic-light toggle has the window maximized.
    #[must_use]
    pub(crate) const fn is_maximized(&self) -> bool {
        matches!(self.window_size, WindowSizeState::Maximized)
    }

    /// Updates maximized state flag.
    pub(crate) fn set_maximized(&mut self, maximized: bool) {
        self.window_size = if maximized {
            WindowSizeState::Maximized
        } else {
            WindowSizeState::Normal
        };
    }
}

fn rustyos_config_mtime() -> Option<SystemTime> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    std::fs::metadata(base.join("rustyos/config.toml"))
        .ok()?
        .modified()
        .ok()
}

pub(crate) fn build_sidebar_items(
    locations: &UserFileLocations,
    volumes: &[VolumeInfo],
    removable_volumes: &[rusty_services::RemovableVolume],
) -> Vec<SidebarRowItem> {
    let mut items = vec![
        SidebarRowItem {
            section: None,
            label: String::from("Recents"),
            location: SidebarLocation::Recents,
            icon: SidebarIconKind::Recents,
            removable_device: None,
        },
        SidebarRowItem {
            section: None,
            label: String::from("Shared"),
            location: SidebarLocation::Shared,
            icon: SidebarIconKind::Shared,
            removable_device: None,
        },
        SidebarRowItem {
            section: Some("Favorites"),
            label: String::from("Applications"),
            location: SidebarLocation::Applications,
            icon: SidebarIconKind::Applications,
            removable_device: None,
        },
        SidebarRowItem {
            section: None,
            label: String::from("Desktop"),
            location: SidebarLocation::Desktop,
            icon: SidebarIconKind::Desktop,
            removable_device: None,
        },
        SidebarRowItem {
            section: None,
            label: String::from("Documents"),
            location: SidebarLocation::Documents,
            icon: SidebarIconKind::Documents,
            removable_device: None,
        },
        SidebarRowItem {
            section: None,
            label: String::from("Downloads"),
            location: SidebarLocation::Downloads,
            icon: SidebarIconKind::Downloads,
            removable_device: None,
        },
        SidebarRowItem {
            section: Some("Locations"),
            label: locations.user_label.clone(),
            location: SidebarLocation::Home,
            icon: SidebarIconKind::Home,
            removable_device: None,
        },
    ];

    append_sidebar_volumes(&mut items, volumes, removable_volumes);

    items.push(SidebarRowItem {
        section: None,
        label: String::from("Network"),
        location: SidebarLocation::Network,
        icon: SidebarIconKind::Network,
        removable_device: None,
    });
    items.push(SidebarRowItem {
        section: None,
        label: String::from("Trash"),
        location: SidebarLocation::Trash,
        icon: SidebarIconKind::Trash,
        removable_device: None,
    });

    for (idx, tag) in FileTagColor::ALL.iter().enumerate() {
        items.push(SidebarRowItem {
            section: (idx == 0).then_some("Tags"),
            label: String::from(tag.label()),
            location: SidebarLocation::Tag(*tag),
            icon: SidebarIconKind::Tag(*tag),
            removable_device: None,
        });
    }
    items
}

fn append_sidebar_volumes(
    items: &mut Vec<SidebarRowItem>,
    volumes: &[VolumeInfo],
    removable_volumes: &[rusty_services::RemovableVolume],
) {
    for vol in volumes {
        if !vol.is_network {
            let matched_rem = removable_volumes
                .iter()
                .find(|r| r.mount_point.as_ref() == Some(&vol.mount_point));
            items.push(SidebarRowItem {
                section: None,
                label: matched_rem.map_or_else(|| vol.name.clone(), |r| r.label.clone()),
                location: SidebarLocation::Volume(vol.mount_point.clone()),
                icon: if matched_rem.is_some() {
                    SidebarIconKind::RemovableDrive
                } else {
                    SidebarIconKind::Volume
                },
                removable_device: matched_rem.map(|r| r.device.clone()),
            });
        }
    }

    for rem in removable_volumes {
        let already_listed = rem
            .mount_point
            .as_ref()
            .is_some_and(|mp| volumes.iter().any(|v| &v.mount_point == mp));
        if !already_listed {
            let target_path = rem
                .mount_point
                .clone()
                .unwrap_or_else(|| PathBuf::from(&rem.device));
            items.push(SidebarRowItem {
                section: None,
                label: rem.label.clone(),
                location: SidebarLocation::Volume(target_path),
                icon: SidebarIconKind::RemovableDrive,
                removable_device: Some(rem.device.clone()),
            });
        }
    }
}

pub(crate) fn match_path_to_sidebar(
    locations: &UserFileLocations,
    volumes: &[VolumeInfo],
    path: &Path,
) -> Option<SidebarLocation> {
    if path == locations.home {
        return Some(SidebarLocation::Home);
    }
    if path == locations.desktop {
        return Some(SidebarLocation::Desktop);
    }
    if path == locations.documents {
        return Some(SidebarLocation::Documents);
    }
    if path == locations.downloads {
        return Some(SidebarLocation::Downloads);
    }
    if path == locations.trash_dir || path == locations.trash_dir.join("files") {
        return Some(SidebarLocation::Trash);
    }
    for vol in volumes {
        if path == vol.mount_point {
            return Some(SidebarLocation::Volume(vol.mount_point.clone()));
        }
    }
    None
}

pub(crate) fn sidebar_label_for(
    loc: &SidebarLocation,
    locations: &UserFileLocations,
    volumes: &[VolumeInfo],
) -> String {
    match loc {
        SidebarLocation::Recents => String::from("Recents"),
        SidebarLocation::Shared => String::from("Shared"),
        SidebarLocation::Applications => String::from("Applications"),
        SidebarLocation::Desktop => String::from("Desktop"),
        SidebarLocation::Documents => String::from("Documents"),
        SidebarLocation::Downloads => String::from("Downloads"),
        SidebarLocation::Home => locations.user_label.clone(),
        SidebarLocation::Volume(path) => volumes
            .iter()
            .find(|v| &v.mount_point == path)
            .map_or_else(|| path_display_title(path), |v| v.name.clone()),
        SidebarLocation::Network => String::from("Network"),
        SidebarLocation::Trash => String::from("Trash"),
        SidebarLocation::Tag(tag) => format!("{} Tag", tag.label()),
    }
}

pub(crate) fn path_display_title(path: &Path) -> String {
    if path == Path::new("/") {
        return String::from("RustyOS HD");
    }
    path.file_name()
        .and_then(|n| n.to_str())
        .filter(|s| !s.is_empty())
        .map_or_else(|| path.to_string_lossy().into_owned(), ToOwned::to_owned)
}

pub(crate) fn parse_desktop_exec_tokens(exec_line: &str) -> Option<Vec<String>> {
    let tokens: Vec<String> = exec_line
        .split_whitespace()
        .filter(|tok| !matches!(*tok, "%u" | "%U" | "%f" | "%F" | "%i" | "%c" | "%k"))
        .map(ToOwned::to_owned)
        .collect();
    (!tokens.is_empty()).then_some(tokens)
}

impl FilesAppState {
    /// Refreshes removable volumes from `StorageService` (`UDisks2` / `lsblk`, `RUSTYOS-134`, `RUSTYOS-149`).
    pub(crate) fn refresh_removable_volumes(&mut self) {
        self.volumes = discover_mounts();
        self.removable_volumes = rusty_services::StorageService::list_removable_volumes();
        self.sidebar_items =
            build_sidebar_items(&self.locations, &self.volumes, &self.removable_volumes);
        self.dirty = true;
    }

    /// Safely unmounts and ejects a removable block device (`RUSTYOS-134`, `RUSTYOS-149`).
    pub(crate) fn eject_removable_volume(&mut self, device: &str) {
        match rusty_services::StorageService::unmount_and_eject(device) {
            Ok(msg) => {
                self.status_message = Some(msg);
                self.refresh_removable_volumes();
            }
            Err(err) => {
                self.status_message = Some(format!("Eject failed: {err}"));
                self.dirty = true;
            }
        }
    }

    /// Mounts (if needed) and navigates to a removable volume (`RUSTYOS-134`, `RUSTYOS-149`).
    pub(crate) fn mount_or_open_removable_volume(&mut self, device: &str) {
        if let Some(mp) = self
            .removable_volumes
            .iter()
            .find(|v| v.device == device)
            .and_then(|v| v.mount_point.clone())
        {
            self.navigate_to_directory(mp);
            return;
        }
        match rusty_services::StorageService::mount_volume(device) {
            Ok(mp) => {
                self.refresh_removable_volumes();
                self.navigate_to_directory(mp);
            }
            Err(err) => {
                self.status_message = Some(format!("Mount failed: {err}"));
                self.dirty = true;
            }
        }
    }

    /// Returns all currently selected entries in the active column (multi-select aware, `RUSTYOS-150`).
    #[must_use]
    pub(crate) fn selected_entries(&self) -> Vec<FileEntry> {
        self.columns
            .get(self.active_column)
            .map_or_default(|col| col.selected_entries().into_iter().cloned().collect())
    }

    /// Toggles collapsed state for a sidebar section header (`Favorites`, `Locations`, `Tags`).
    pub(crate) fn toggle_sidebar_section(&mut self, section: &'static str) {
        if !self.collapsed_sidebar_sections.remove(section) {
            self.collapsed_sidebar_sections.insert(section);
        }
        self.sidebar_scroll_y = 0;
        self.sidebar_scroll_anim.set_immediate(0);
        self.dirty = true;
    }

    /// Updates pointer hover hit target, marking the frame dirty only on state transitions.
    pub(crate) fn set_hovered_hit(&mut self, hit: Option<crate::layout::FilesHit>) {
        if self.hovered_hit != hit {
            self.hovered_hit = hit;
            self.dirty = true;
        }
    }

    /// Resolves the paths chosen in `FilePickerMode` without exiting (testable helper, `RUSTYOS-154`).
    #[must_use]
    pub(crate) fn resolve_picker_selection(&self) -> Vec<PathBuf> {
        let Some(mode) = &self.picker_mode else {
            return Vec::new();
        };
        match mode {
            FilePickerMode::OpenFile => self
                .selected_entry()
                .filter(|e| !e.is_navigable_dir())
                .map_or_default(|e| vec![e.path.clone()]),
            FilePickerMode::OpenFiles => self
                .selected_entries()
                .into_iter()
                .filter(|e| !e.is_navigable_dir())
                .map(|e| e.path)
                .collect(),
            FilePickerMode::OpenFolder => {
                if let Some(entry) = self.selected_entry()
                    && entry.is_navigable_dir()
                {
                    return vec![entry.path.clone()];
                }
                self.active_directory_path().into_iter().collect()
            }
            FilePickerMode::SaveFile { .. } => {
                let name = self.picker_filename.trim();
                if name.is_empty() {
                    return Vec::new();
                }
                self.active_directory_path()
                    .map_or_default(|dir| vec![dir.join(name)])
            }
        }
    }

    /// Confirms the active `FileChooser` dialog: navigates into a selected folder in OpenFile/OpenFiles mode,
    /// or prints chosen paths to `stdout` and requests clean exit (`RUSTYOS-154`).
    pub(crate) fn confirm_file_picker(&mut self) {
        if matches!(
            self.picker_mode,
            Some(FilePickerMode::OpenFile | FilePickerMode::OpenFiles)
        ) && let Some(entry) = self.selected_entry().cloned()
            && entry.is_navigable_dir()
        {
            self.navigate_to_directory(entry.path);
            return;
        }
        let chosen = self.resolve_picker_selection();
        if chosen.is_empty() {
            self.status_message = Some(String::from("Please select a target first"));
            self.dirty = true;
            return;
        }
        for path in chosen {
            println!("{}", path.display());
        }
        self.picker_confirmed = true;
        self.picker_cancelled = false;
        self.run_state = AppRunState::Exiting;
    }

    /// Cancels the active `FileChooser` dialog and exits with status 1 (`RUSTYOS-154`).
    pub(crate) fn cancel_file_picker(&mut self) {
        self.picker_confirmed = false;
        self.picker_cancelled = true;
        self.run_state = AppRunState::Exiting;
    }

    /// Returns the filesystem path of the active column when it displays a real directory.
    #[must_use]
    pub(crate) fn active_directory_path(&self) -> Option<PathBuf> {
        let col = self
            .columns
            .get(self.active_column)
            .or_else(|| self.columns.first())?;
        match &col.target {
            ListingTarget::Directory(dir) => Some(dir.clone()),
            _ => None,
        }
    }

    /// Spawns `rusty-terminal` (or a system terminal fallback) in `dir` (`RUSTYOS-149`).
    pub(crate) fn action_open_in_terminal(&mut self, dir: &Path) {
        let target_dir = if dir.is_dir() {
            dir.to_path_buf()
        } else {
            dir.parent()
                .map_or_else(|| self.locations.home.clone(), Path::to_path_buf)
        };
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Ok(exe) = std::env::current_exe()
            && let Some(parent) = exe.parent()
        {
            let sibling = parent.join("rusty-terminal");
            if sibling.is_file() {
                candidates.push(sibling);
            }
        }
        candidates.push(PathBuf::from("rusty-terminal"));
        if let Ok(term) = std::env::var("TERMINAL")
            && !term.trim().is_empty()
        {
            candidates.push(PathBuf::from(term));
        }
        candidates.push(PathBuf::from("alacritty"));
        candidates.push(PathBuf::from("foot"));
        candidates.push(PathBuf::from("xterm"));

        for bin in &candidates {
            if std::process::Command::new(bin)
                .current_dir(&target_dir)
                .spawn()
                .is_ok()
            {
                self.status_message = Some(format!(
                    "Opened terminal in {}",
                    path_display_title(&target_dir)
                ));
                self.dirty = true;
                return;
            }
        }
        self.status_message = Some(String::from("No terminal emulator found"));
        self.dirty = true;
    }

    /// Extracts an archive (`.zip`, `.tar.*`, `.tgz`) into its parent directory on a background thread (`RUSTYOS-149`).
    pub(crate) fn action_extract_archive(&mut self, archive_path: PathBuf) {
        let Some(parent_dir) = archive_path.parent().map(Path::to_path_buf) else {
            self.status_message = Some(String::from("Cannot determine target folder"));
            self.dirty = true;
            return;
        };
        let name = path_display_title(&archive_path);
        self.status_message = Some(format!("Extracting {name}..."));
        self.active_archive_jobs += 1;
        self.dirty = true;
        let tx = self.archive_tx.clone();
        let ping = self.worker_ping.clone();
        std::thread::spawn(move || {
            let outcome = extract_archive_to_dir(&archive_path, &parent_dir, &name);
            let _ = tx.send(ArchiveJobResult {
                target_dir: parent_dir,
                outcome,
            });
            if let Some(p) = ping {
                p.ping();
            }
        });
    }

    /// Compresses a file or directory to `<name>.zip` in its parent directory on a background thread (`RUSTYOS-149`).
    pub(crate) fn action_compress_to_zip(&mut self, item_path: &Path) {
        let Some(parent_dir) = item_path.parent().map(Path::to_path_buf) else {
            self.status_message = Some(String::from("Cannot determine target folder"));
            self.dirty = true;
            return;
        };
        let Some(item_name) = item_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
        else {
            return;
        };
        let zip_path = unique_zip_output_path(&parent_dir, &item_name);
        let zip_name = path_display_title(&zip_path);
        self.status_message = Some(format!("Compressing to {zip_name}..."));
        self.active_archive_jobs += 1;
        self.dirty = true;
        let tx = self.archive_tx.clone();
        let ping = self.worker_ping.clone();
        std::thread::spawn(move || {
            let outcome = compress_to_zip_archive(&parent_dir, &zip_path, &item_name, &zip_name);
            let _ = tx.send(ArchiveJobResult {
                target_dir: parent_dir,
                outcome,
            });
            if let Some(p) = ping {
                p.ping();
            }
        });
    }
}

/// Validates archive member paths to prevent Zip-Slip / Tar-Slip directory traversal (`RUSTYOS-168`).
pub(crate) fn validate_archive_member_paths(listing_output: &str) -> Result<(), String> {
    for raw_line in listing_output.lines() {
        let entry = raw_line.trim();
        if entry.is_empty() {
            continue;
        }
        if entry.starts_with('/') || entry.starts_with('\\') || entry.contains('\0') {
            return Err(format!("Blocked unsafe absolute archive entry: {entry}"));
        }
        let path = Path::new(entry);
        if path.components().any(|c| {
            matches!(
                c,
                std::path::Component::ParentDir | std::path::Component::RootDir
            )
        }) {
            return Err(format!("Blocked archive traversal entry: {entry}"));
        }
    }
    Ok(())
}

fn check_archive_safety(archive_path: &Path, is_zip: bool) -> Result<(), String> {
    let mut cmd = if is_zip {
        let mut c = std::process::Command::new("unzip");
        c.args(["-Z1", "--"]).arg(archive_path);
        c
    } else {
        let mut c = std::process::Command::new("tar");
        c.arg("-tf").arg(archive_path);
        c
    };
    match rusty_services::run_command_bounded(&mut cmd, std::time::Duration::from_secs(15)) {
        Ok(out) if out.status.success() => {
            validate_archive_member_paths(&String::from_utf8_lossy(&out.stdout))
        }
        Ok(out) => {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_owned();
            Err(if err.is_empty() {
                "Failed to inspect archive entries before extraction".to_owned()
            } else {
                err
            })
        }
        Err(_) => Ok(()),
    }
}

fn extract_archive_to_dir(
    archive_path: &Path,
    parent_dir: &Path,
    name: &str,
) -> Result<String, String> {
    let is_zip = archive_path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"));
    check_archive_safety(archive_path, is_zip)?;

    // Always extract into a fresh non-colliding sibling subfolder so existing files are never
    // silently overwritten (`RUSTYOS-168`).
    let stem = archive_path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("extracted");
    let raw_target = parent_dir.join(stem);
    let dest_dir = if raw_target.exists() {
        rusty_services::file_manager::pick_unique_copy_path(&raw_target)
    } else {
        raw_target
    };
    std::fs::create_dir_all(&dest_dir)
        .map_err(|err| format!("Failed to create extraction folder: {err}"))?;

    let archive_str = archive_path.to_string_lossy();
    let dir_str = dest_dir.to_string_lossy();
    let folder_name = dest_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(stem);
    let ok_msg = format!("Extracted {name} to {folder_name}");
    if is_zip {
        run_archive_command(
            "unzip",
            &["-n", "-q", &archive_str, "-d", &dir_str],
            parent_dir,
            ok_msg.clone(),
        )
        .or_else(|_| {
            run_archive_command(
                "bsdtar",
                &["-xf", &archive_str, "-C", &dir_str],
                parent_dir,
                ok_msg.clone(),
            )
        })
        .or_else(|_| {
            run_archive_command(
                "python3",
                &["-m", "zipfile", "-e", &archive_str, &dir_str],
                parent_dir,
                ok_msg,
            )
        })
    } else {
        run_archive_command(
            "tar",
            &["--no-same-owner", "-xf", &archive_str, "-C", &dir_str],
            parent_dir,
            ok_msg.clone(),
        )
        .or_else(|_| {
            run_archive_command(
                "bsdtar",
                &["-xf", &archive_str, "-C", &dir_str],
                parent_dir,
                ok_msg,
            )
        })
    }
}

fn compress_to_zip_archive(
    parent_dir: &Path,
    zip_path: &Path,
    item_name: &str,
    zip_name: &str,
) -> Result<String, String> {
    if item_name.is_empty() || item_name.contains('/') || item_name.contains('\0') {
        return Err("Invalid item name for archive compression".to_owned());
    }
    let zip_str = zip_path.to_string_lossy();
    let ok_msg = format!("Created {zip_name}");
    // Pass `--` before `item_name` so filenames starting with `-` can never be parsed as CLI flags (`RUSTYOS-168`).
    run_archive_command(
        "zip",
        &["-r", "-q", &zip_str, "--", item_name],
        parent_dir,
        ok_msg.clone(),
    )
    .or_else(|_| {
        run_archive_command(
            "bsdtar",
            &["-a", "-cf", &zip_str, "--", item_name],
            parent_dir,
            ok_msg.clone(),
        )
    })
    .or_else(|_| {
        run_archive_command(
            "python3",
            &["-m", "zipfile", "-c", &zip_str, item_name],
            parent_dir,
            ok_msg,
        )
    })
}

/// Returns true when `path` has a supported archive extension for `"Extract Here"`.
#[must_use]
pub(crate) fn is_extractable_archive(path: &Path) -> bool {
    if path.extension().is_some_and(|ext| {
        ext.eq_ignore_ascii_case("zip")
            || ext.eq_ignore_ascii_case("tar")
            || ext.eq_ignore_ascii_case("tgz")
    }) {
        return true;
    }
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".tar.gz")
        || lower.ends_with(".tar.zst")
        || lower.ends_with(".tar.xz")
        || lower.ends_with(".tar.bz2")
}

fn unique_zip_output_path(parent_dir: &Path, item_name: &str) -> PathBuf {
    let base = parent_dir.join(format!("{item_name}.zip"));
    if !base.exists() {
        return base;
    }
    for n in 2..=999 {
        let candidate = parent_dir.join(format!("{item_name} ({n}).zip"));
        if !candidate.exists() {
            return candidate;
        }
    }
    base
}

fn run_archive_command(
    program: &str,
    args: &[&str],
    cwd: &Path,
    ok_msg: String,
) -> Result<String, String> {
    let output = std::process::Command::new(program)
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|err| format!("{program} unavailable: {err}"))?;
    if output.status.success() {
        Ok(ok_msg)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        Err(if stderr.is_empty() {
            format!("{program} exited with status {}", output.status)
        } else {
            stderr
        })
    }
}

/// Classification of a regular file for direct program execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExecutableFileState {
    /// ELF binary or `#!` script that already has an execute permission bit (`+x`).
    Runnable,
    /// ELF binary or `.AppImage`/`.run` executable missing the execute permission bit.
    BinaryMissingExecBit,
    /// `#!` or `.sh` script missing the execute permission bit (opens in editor by default,
    /// offers "Allow Executing & Run" in the context menu).
    ScriptMissingExecBit,
    /// Ordinary document or non-executable file.
    NotExecutable,
}

/// Inspects metadata and the first 4 magic bytes to classify whether `path` can be
/// launched directly as a program (safely ignoring `+x` flags on non-executable FAT/NTFS files).
#[must_use]
pub(crate) fn classify_executable_file(path: &Path) -> ExecutableFileState {
    use std::io::Read as _;
    use std::os::unix::fs::PermissionsExt as _;

    let Ok(meta) = std::fs::metadata(path) else {
        return ExecutableFileState::NotExecutable;
    };
    if !meta.is_file() {
        return ExecutableFileState::NotExecutable;
    }
    let has_exec_bit = meta.permissions().mode() & 0o111 != 0;

    let mut header = [0_u8; 4];
    let n = std::fs::File::open(path)
        .and_then(|mut f| f.read(&mut header))
        .unwrap_or(0);
    let is_elf = n >= 4 && header == [0x7f, b'E', b'L', b'F'];
    let is_shebang = n >= 2 && header[0] == b'#' && header[1] == b'!';

    let lower_ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    let is_binary_ext = matches!(lower_ext.as_deref(), Some("appimage" | "run" | "bin"));
    let is_script_ext = matches!(lower_ext.as_deref(), Some("sh" | "bash" | "zsh"));

    if has_exec_bit && (is_elf || is_shebang) {
        ExecutableFileState::Runnable
    } else if is_elf || is_binary_ext {
        ExecutableFileState::BinaryMissingExecBit
    } else if is_shebang || is_script_ext {
        ExecutableFileState::ScriptMissingExecBit
    } else {
        ExecutableFileState::NotExecutable
    }
}

/// Grants execute permission bits matching existing read bits (at minimum owner `+x`).
pub(crate) fn ensure_executable_permission(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;

    let meta = std::fs::metadata(path)?;
    let mode = meta.permissions().mode();
    if mode & 0o111 != 0 {
        return Ok(());
    }
    let exec_bits = ((mode & 0o444) >> 2) | 0o100;
    let mut perms = meta.permissions();
    perms.set_mode(mode | exec_bits);
    std::fs::set_permissions(path, perms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    struct TestHomeGuard {
        root: PathBuf,
    }

    impl TestHomeGuard {
        fn new(label: &str) -> std::io::Result<Self> {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let root = std::env::temp_dir().join(format!(
                "rustyos-files-state-{label}-{}-{unique}",
                std::process::id()
            ));
            fs::create_dir_all(root.join("home/Documents/SubDir"))?;
            fs::create_dir_all(root.join("home/Downloads"))?;
            fs::create_dir_all(root.join("home/Desktop"))?;
            fs::write(root.join("home/Documents/notes.txt"), b"hello")?;
            fs::write(root.join("home/Documents/SubDir/deep.rs"), b"fn main() {}")?;
            fs::write(root.join("home/Documents/.hidden"), b"secret")?;
            Ok(Self { root })
        }

        fn locations(&self) -> UserFileLocations {
            UserFileLocations::with_roots(
                &self.root.join("home"),
                &self.root.join("config"),
                &self.root.join("data"),
            )
        }
    }

    impl Drop for TestHomeGuard {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn miller_columns_history_and_stage_b_guard_work() -> Result<(), Box<dyn std::error::Error>> {
        let guard = TestHomeGuard::new("miller")?;
        let mut state = FilesAppState::new(guard.locations(), &StartupOptions::default());
        state.complete_pending_loads_sync();
        assert_eq!(state.theme, FilesThemeMode::Light);
        assert_eq!(state.view_mode, FileViewMode::Icons);
        assert!(!state.show_path_bar);
        state.set_view_mode(FileViewMode::Columns);

        state.navigate_sidebar(SidebarLocation::Documents, true);
        state.complete_pending_loads_sync();
        assert!(state.can_go_back());
        assert_eq!(state.columns.len(), 1);
        assert_eq!(state.columns[0].visible_entries.len(), 2);

        // Select SubDir (sorted first since directories lead) -> opens column 1
        state.select_entry(0, 0, false);
        state.complete_pending_loads_sync();
        assert_eq!(state.columns.len(), 2);
        assert_eq!(state.columns[1].visible_entries[0].name, "deep.rs");

        // History Back & Forward
        state.go_back();
        state.complete_pending_loads_sync();
        assert_eq!(state.window_title, "home");
        assert!(state.can_go_forward());

        state.go_forward();
        state.complete_pending_loads_sync();
        assert_eq!(state.window_title, "Documents");

        // Stage B views (`Icons`, `Gallery`) switch & persist cleanly
        state.set_view_mode(FileViewMode::Icons);
        assert_eq!(state.view_mode, FileViewMode::Icons);
        state.set_view_mode(FileViewMode::Gallery);
        assert_eq!(state.view_mode, FileViewMode::Gallery);
        state.set_view_mode(FileViewMode::Columns);
        assert_eq!(state.view_mode, FileViewMode::Columns);

        // Path Bar toggle persists & Home SearchScope finds deep files across subdirectories
        state.toggle_path_bar();
        assert!(state.show_path_bar);
        assert!(FilesAppState::new(guard.locations(), &StartupOptions::default()).show_path_bar);

        state.search_active = true;
        state.set_search_query(String::from("deep"));
        state.set_search_scope(SearchScope::Home);
        state.complete_pending_loads_sync();
        assert_eq!(state.columns[0].visible_entries.len(), 1);
        assert_eq!(state.columns[0].visible_entries[0].name, "deep.rs");
        state.set_search_scope(SearchScope::ThisFolder);
        state.set_search_query(String::new());

        // Ctrl+Shift+N creates a new folder via keyboard shortcut
        state.handle_key_input("N", Some("N"), true, true);
        state.complete_pending_loads_sync();
        assert!(guard.locations().documents.join("untitled folder").is_dir());
        Ok(())
    }

    #[test]
    fn theme_toggle_tags_hidden_and_trash_actions_persist() -> Result<(), Box<dyn std::error::Error>>
    {
        let guard = TestHomeGuard::new("persist")?;
        let locs = guard.locations();
        let mut state = FilesAppState::new(
            locs.clone(),
            &StartupOptions {
                initial_path: Some(locs.documents.clone()),
                ..Default::default()
            },
        );
        state.complete_pending_loads_sync();

        state.toggle_theme();
        assert_eq!(state.theme, FilesThemeMode::Dark);
        state.toggle_show_hidden();
        assert!(state.show_hidden);
        state.set_view_mode(FileViewMode::List);
        assert_eq!(state.view_mode, FileViewMode::List);

        let notes_path = locs.documents.join("notes.txt");
        state.toggle_tag_on_path(&notes_path, FileTagColor::Green);
        assert_eq!(state.tags_for(&notes_path), &[FileTagColor::Green]);

        // Reload fresh app state -> persisted preferences & tags are restored
        let reloaded = FilesAppState::new(locs.clone(), &StartupOptions::default());
        assert_eq!(reloaded.theme, FilesThemeMode::Dark);
        assert_eq!(reloaded.view_mode, FileViewMode::List);
        assert!(reloaded.show_hidden);
        assert_eq!(reloaded.tags_for(&notes_path), &[FileTagColor::Green]);

        // Test New Folder & Move to Trash + Empty Trash
        state.action_new_folder();
        state.complete_pending_loads_sync();
        assert!(locs.documents.join("untitled folder").is_dir());

        let idx = state.columns[0]
            .visible_entries
            .iter()
            .position(|e| e.name == "notes.txt")
            .ok_or("notes.txt missing")?;
        state.select_entry(0, idx, false);
        state.action_move_selected_to_trash();
        state.complete_pending_loads_sync();
        assert!(!notes_path.exists());
        assert_eq!(state.tags_for(&notes_path), []);

        state.navigate_sidebar(SidebarLocation::Trash, false);
        state.complete_pending_loads_sync();
        assert_eq!(state.columns[0].visible_entries.len(), 1);
        state.action_empty_trash();
        state.complete_pending_loads_sync();
        assert_eq!(state.columns[0].visible_entries, []);
        Ok(())
    }

    #[test]
    fn stage_b_tabs_rename_goto_quicklook_getinfo_and_clipboard_work()
    -> Result<(), Box<dyn std::error::Error>> {
        let guard = TestHomeGuard::new("stage-b-flows")?;
        let locs = guard.locations();
        let mut state = FilesAppState::new(locs.clone(), &StartupOptions::default());
        state.complete_pending_loads_sync();

        // 1. Multi-tab isolation & cycling
        assert!(!state.tab_bar_visible());
        state.new_tab(Some(ListingTarget::Directory(locs.documents.clone())));
        state.complete_pending_loads_sync();
        assert!(state.tab_bar_visible());
        assert_eq!(state.tab_titles().len(), 2);
        assert_eq!(state.window_title, "Documents");

        state.handle_key_input("Tab", None, true, false);
        assert_eq!(state.active_tab, 0);
        assert_eq!(state.window_title, "home");

        state.handle_key_input("Tab", None, true, true);
        assert_eq!(state.active_tab, 1);
        assert_eq!(state.window_title, "Documents");

        // 2. Path Bar breadcrumbs inside Documents/SubDir
        state.set_view_mode(FileViewMode::Icons);
        state.last_icon_grid_cols = 4;
        state.select_entry(0, 0, true); // opens SubDir
        state.complete_pending_loads_sync();
        let crumbs = state.path_breadcrumbs();
        let crumb_labels: Vec<&str> = crumbs.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(crumb_labels, vec!["home", "Documents", "SubDir"]);

        // 3. Inline Rename with tag preservation
        let deep_rs = locs.documents.join("SubDir/deep.rs");
        state.toggle_tag_on_path(&deep_rs, FileTagColor::Blue);
        state.select_entry(0, 0, false);
        state.handle_key_input("F2", None, false, false);
        assert!(state.inline_rename.is_some());
        state.handle_key_input("BackSpace", None, false, false); // remove 's' -> deep.r
        state.handle_key_input("", Some("s"), false, false);
        if let Some(rn) = &mut state.inline_rename {
            rn.draft = String::from("renamed.rs");
        }
        state.handle_key_input("Return", None, false, false);
        state.complete_pending_loads_sync();
        let renamed = locs.documents.join("SubDir/renamed.rs");
        assert!(renamed.exists());
        assert_eq!(state.tags_for(&renamed), &[FileTagColor::Blue]);

        // 4. Quick Look toggle (`Space`) & close (`Escape`)
        let target_entry = state.columns[0]
            .visible_entries
            .iter()
            .position(|e| e.name == "renamed.rs")
            .ok_or("renamed.rs missing")?;
        state.select_entry(0, target_entry, false);
        state.handle_key_input("Space", Some(" "), false, false);
        assert!(state.quick_look.is_some());
        state.handle_key_input("Escape", None, false, false);
        assert!(state.quick_look.is_none());

        // 5. Get Info & Go to Folder
        state.handle_key_input("i", Some("i"), true, false);
        assert!(state.get_info.is_some());
        state.handle_key_input("Escape", None, false, false);
        assert!(state.get_info.is_none());

        state.open_go_to_folder();
        if let Some(gt) = &mut state.go_to_folder {
            gt.input = String::from("~/Downloads");
        }
        state.submit_go_to_folder();
        state.complete_pending_loads_sync();
        assert!(state.go_to_folder.is_none());
        assert_eq!(state.window_title, "Downloads");

        // 6. Close second tab -> collapses back to 1 tab
        state.close_active_tab();
        assert_eq!(state.tab_titles().len(), 1);
        assert!(!state.tab_bar_visible());
        Ok(())
    }

    #[test]
    fn smooth_scroll_axes_and_caret_blink_work() -> Result<(), Box<dyn std::error::Error>> {
        let guard = TestHomeGuard::new("smooth-scroll-caret")?;
        let locs = guard.locations();
        for idx in 0..30 {
            let _ = std::fs::File::create(locs.documents.join(format!("item_{idx:02}.txt")));
        }
        let mut state = FilesAppState::new(locs.clone(), &StartupOptions::default());
        state.complete_pending_loads_sync();
        state.set_view_mode(FileViewMode::Columns);

        let col = state
            .columns
            .first()
            .and_then(|c| c.visible_entries.iter().position(|e| e.name == "Documents"))
            .ok_or("Documents missing")?;
        state.select_entry(0, col, false);
        state.complete_pending_loads_sync();

        state.scroll_axis(ScrollTargetArea::Column(1), 88.0, true, 160);
        assert!(state.has_active_animations());
        assert!((state.overlay_scrollbar_alpha - 1.0).abs() < 0.01);
        let t0 = state.last_anim_tick.unwrap_or_else(Instant::now);
        let _ = state.tick_animations(t0 + Duration::from_millis(16));
        assert!(state.columns[1].scroll_y > 0);
        assert!(state.columns[1].scroll_y < 88);
        for step in 2..=30 {
            let _ = state.tick_animations(t0 + Duration::from_millis(step * 16));
        }
        assert_eq!(state.columns[1].scroll_y, 88);
        assert!(!state.has_active_animations());
        let scroll_at = state.last_scroll_activity.unwrap_or(t0);
        let _ = state.tick_animations(scroll_at + Duration::from_millis(950));
        assert!(state.overlay_scrollbar_alpha > 0.0 && state.overlay_scrollbar_alpha < 1.0);
        let _ = state.tick_animations(scroll_at + Duration::from_millis(1200));
        assert_eq!(state.overlay_scrollbar_alpha, 0.0);

        state.search_active = true;
        state.set_search_query(String::from("item"));
        assert!(state.caret_visible);
        let flip_at = state.last_caret_tick + Duration::from_millis(550);
        assert!(state.update_caret_blink(flip_at));
        assert!(!state.caret_visible);
        state.reset_caret_blink();
        assert!(state.caret_visible);
        Ok(())
    }

    #[test]
    fn accent_config_parses_and_updates_palette() -> Result<(), Box<dyn std::error::Error>> {
        let guard = TestHomeGuard::new("accent-sync")?;
        let locs = guard.locations();
        std::fs::create_dir_all(&locs.config_dir)?;
        std::fs::write(
            locs.config_dir.join("config.toml"),
            "[ui]\ntheme = \"dark\"\naccent_index = 2\n",
        )?;
        let state = FilesAppState::new(locs, &StartupOptions::default());
        assert_eq!(state.theme, FilesThemeMode::Dark);
        assert_eq!(state.accent_index, 2);
        let pal = crate::render::palette_for_with_accent(state.theme, state.accent_index);
        assert_eq!(
            pal.accent,
            rusty_services::file_manager::accent_rgba_for_index(2, true)
        );
        Ok(())
    }

    #[test]
    fn unified_text_field_editing_and_clipboard_work_for_search_and_dialogs()
    -> Result<(), Box<dyn std::error::Error>> {
        let guard = TestHomeGuard::new("text-field-clipboard")?;
        let locs = guard.locations();
        let mut state = FilesAppState::new(locs.clone(), &StartupOptions::default());
        state.complete_pending_loads_sync();

        let mut fe = rusty_ui::UiFontEngine::new();
        let mut mem = rusty_ui::MemoryClipboard::new();
        let ctrl = rusty_ui::InputModifiers {
            ctrl: true,
            shift: false,
            alt: false,
        };
        state.search_active = true;
        assert!(state.apply_focused_paste("report", &mut fe));
        assert_eq!(state.search_query, "report");
        assert!(state.handle_focused_text_key(
            rusty_ui::TextInputKey::Char('a'),
            ctrl,
            &mut fe,
            &mut mem
        ));
        assert!(state.handle_focused_text_key(
            rusty_ui::TextInputKey::Char('c'),
            ctrl,
            &mut fe,
            &mut mem
        ));
        assert_eq!(
            rusty_ui::ClipboardSink::get_text(&mut mem).as_deref(),
            Some("report")
        );

        state.open_go_to_folder();
        assert!(state.handle_focused_text_key(
            rusty_ui::TextInputKey::Char('a'),
            ctrl,
            &mut fe,
            &mut mem
        ));
        assert!(state.apply_focused_paste("~/Documents", &mut fe));
        assert_eq!(
            state.go_to_folder.as_ref().map(|g| g.input.as_str()),
            Some("~/Documents")
        );
        assert!(state.handle_focused_text_key(
            rusty_ui::TextInputKey::Escape,
            rusty_ui::InputModifiers::default(),
            &mut fe,
            &mut mem,
        ));
        assert!(state.go_to_folder.is_none());
        Ok(())
    }

    #[test]
    fn file_drag_activation_and_drop_guards_work() {
        let mut drag = FileDragState::start(vec![PathBuf::from("/tmp/folder_a")], (100.0, 100.0));
        assert!(!drag.update_pos((102.0, 103.0)));
        assert!(drag.update_pos((108.0, 100.0)));
        assert!(!drag.can_drop_on(std::path::Path::new("/tmp/folder_a")));
        assert!(!drag.can_drop_on(std::path::Path::new("/tmp/folder_a/sub")));
        assert!(drag.can_drop_on(std::path::Path::new("/tmp/folder_b")));
    }

    #[test]
    fn removable_volumes_appear_in_sidebar_with_eject_hit_target_and_status_space()
    -> Result<(), Box<dyn std::error::Error>> {
        let guard = TestHomeGuard::new("removable-and-space")?;
        let locs = guard.locations();
        let rem = vec![rusty_services::RemovableVolume {
            device: String::from("/dev/sdb1"),
            label: String::from("BACKUP_USB"),
            mount_point: Some(PathBuf::from("/run/media/alice/BACKUP_USB")),
            size_bytes: 32_000_000_000,
            removable: true,
            ejectable: true,
        }];
        let items = build_sidebar_items(&locs, &[], &rem);
        let usb_item = items
            .iter()
            .find(|i| i.removable_device.as_deref() == Some("/dev/sdb1"));
        assert!(usb_item.is_some());
        if let Some(item) = usb_item {
            assert_eq!(item.label, "BACKUP_USB");
            assert_eq!(item.icon, SidebarIconKind::RemovableDrive);
        }

        let mut state = FilesAppState::new(locs, &StartupOptions::default());
        state.complete_pending_loads_sync();
        state.removable_volumes = rem;
        state.sidebar_items = build_sidebar_items(&state.locations, &[], &state.removable_volumes);
        state.active_dir_avail_bytes = Some(184_200_000_000);
        assert!(state.status_summary().contains("184.2 GB available"));

        let layout = crate::layout::compute_layout(&state, 960, 620);
        let usb_row = layout
            .sidebar_rows
            .iter()
            .find(|r| r.removable_device.as_deref() == Some("/dev/sdb1"));
        assert!(usb_row.is_some());
        if let Some(row) = usb_row {
            let eject_r = row.eject_rect;
            assert!(eject_r.is_some());
            if let Some(er) = eject_r {
                let hit = crate::layout::hit_test(
                    &layout,
                    f64::from(er.x + er.w / 2),
                    f64::from(er.y + er.h / 2),
                );
                assert_eq!(
                    hit,
                    Some(crate::layout::FilesHit::SidebarRemovableEject(
                        String::from("/dev/sdb1")
                    ))
                );
            }
        }

        assert!(is_extractable_archive(Path::new("/tmp/release.zip")));
        assert!(is_extractable_archive(Path::new("/tmp/archive.tar.gz")));
        assert!(is_extractable_archive(Path::new("/tmp/pkg.tar.zst")));
        assert!(!is_extractable_archive(Path::new("/tmp/notes.txt")));
        Ok(())
    }

    #[test]
    fn multi_select_collapsible_sidebar_and_hover_state_work()
    -> Result<(), Box<dyn std::error::Error>> {
        let guard = TestHomeGuard::new("multiselect-sidebar-hover")?;
        let locs = guard.locations();
        fs::write(locs.home.join("a.txt"), "a")?;
        fs::write(locs.home.join("b.txt"), "b")?;
        fs::write(locs.home.join("c.txt"), "c")?;

        let mut state = FilesAppState::new(locs, &StartupOptions::default());
        state.complete_pending_loads_sync();
        let total = state.columns[0].visible_entries.len();
        assert!(total >= 3);

        // Single select -> Shift+Click range -> Ctrl+Click toggle -> Ctrl+A
        state.select_entry_with_modifiers(0, 0, false, false, false);
        assert_eq!(state.selected_entries().len(), 1);
        state.select_entry_with_modifiers(0, 2, false, true, false);
        assert_eq!(state.selected_entries().len(), 3);
        assert!(state.status_summary().starts_with("3 of "));
        state.select_entry_with_modifiers(0, 1, true, false, false);
        assert_eq!(state.selected_entries().len(), 2);

        state.action_copy_selected();
        assert_eq!(state.clipboard.as_ref().map(|c| c.paths.len()), Some(2));
        state.select_all_active_column();
        assert_eq!(state.selected_entries().len(), total);

        // Collapsible sidebar section hides rows and reduces total height
        let expanded_h =
            sidebar_total_height(&state.sidebar_items, &state.collapsed_sidebar_sections);
        let expanded_rows = crate::layout::compute_layout(&state, 960, 620)
            .sidebar_rows
            .len();
        state.toggle_sidebar_section("Tags");
        let collapsed_h =
            sidebar_total_height(&state.sidebar_items, &state.collapsed_sidebar_sections);
        let collapsed_rows = crate::layout::compute_layout(&state, 960, 620)
            .sidebar_rows
            .len();
        assert!(collapsed_h < expanded_h);
        assert!(collapsed_rows < expanded_rows);

        // Hover transitions mark dirty only on change
        state.dirty = false;
        state.set_hovered_hit(Some(crate::layout::FilesHit::SortButton));
        assert!(state.dirty);
        state.dirty = false;
        state.set_hovered_hit(Some(crate::layout::FilesHit::SortButton));
        assert!(!state.dirty);

        // FileChooser picker modes resolve selected files or save target path
        state.picker_mode = Some(FilePickerMode::SaveFile {
            default_name: String::from("export.zip"),
        });
        state.picker_filename = String::from("export.zip");
        let save_paths = state.resolve_picker_selection();
        assert_eq!(save_paths.len(), 1);
        assert!(save_paths[0].ends_with("export.zip"));
        Ok(())
    }

    #[test]
    fn compress_and_extract_zip_archive_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
        let guard = TestHomeGuard::new("zip-roundtrip")?;
        let src_dir = guard.root.join("src");
        let dst_dir = guard.root.join("dst");
        fs::create_dir_all(&src_dir)?;
        fs::create_dir_all(&dst_dir)?;
        let sample = src_dir.join("hello.txt");
        fs::write(&sample, b"rustyos zip test")?;
        let zip_path = src_dir.join("hello.txt.zip");

        compress_to_zip_archive(&src_dir, &zip_path, "hello.txt", "hello.txt.zip")
            .map_err(std::io::Error::other)?;
        assert!(zip_path.is_file());

        extract_archive_to_dir(&zip_path, &dst_dir, "hello.txt.zip")
            .map_err(std::io::Error::other)?;
        assert_eq!(
            fs::read_to_string(dst_dir.join("hello.txt").join("hello.txt"))?,
            "rustyos zip test"
        );
        // Re-extracting never overwrites the existing folder (`RUSTYOS-168`).
        extract_archive_to_dir(&zip_path, &dst_dir, "hello.txt.zip")
            .map_err(std::io::Error::other)?;
        assert_eq!(
            fs::read_to_string(dst_dir.join("hello copy.txt").join("hello.txt"))?,
            "rustyos zip test"
        );
        Ok(())
    }

    #[test]
    fn classifies_and_grants_executable_files() -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::PermissionsExt as _;

        let guard = TestHomeGuard::new("exec-classify")?;
        let elf_no_exec = guard.root.join("my-tool");
        fs::write(&elf_no_exec, b"\x7fELF\x02\x01\x01\x00")?;
        fs::set_permissions(&elf_no_exec, fs::Permissions::from_mode(0o644))?;
        assert_eq!(
            classify_executable_file(&elf_no_exec),
            ExecutableFileState::BinaryMissingExecBit
        );

        ensure_executable_permission(&elf_no_exec)?;
        assert_eq!(
            classify_executable_file(&elf_no_exec),
            ExecutableFileState::Runnable
        );

        let script = guard.root.join("launch.sh");
        fs::write(&script, b"#!/bin/sh\necho hi\n")?;
        fs::set_permissions(&script, fs::Permissions::from_mode(0o644))?;
        assert_eq!(
            classify_executable_file(&script),
            ExecutableFileState::ScriptMissingExecBit
        );
        ensure_executable_permission(&script)?;
        assert_eq!(
            classify_executable_file(&script),
            ExecutableFileState::Runnable
        );

        // Plain text file with +x (e.g. FAT32 mount) must NOT be treated as runnable.
        let doc = guard.root.join("notes.txt");
        fs::write(&doc, b"just a text file")?;
        fs::set_permissions(&doc, fs::Permissions::from_mode(0o755))?;
        assert_eq!(
            classify_executable_file(&doc),
            ExecutableFileState::NotExecutable
        );
        Ok(())
    }

    #[test]
    fn archive_member_validator_blocks_zip_slip_and_save_file_focuses_filename() {
        assert!(validate_archive_member_paths("docs/readme.txt\nimg/logo.png\n").is_ok());
        assert!(validate_archive_member_paths("../escape.txt\n").is_err());
        assert!(validate_archive_member_paths("nested/../../etc/passwd\n").is_err());
        assert!(validate_archive_member_paths("/etc/shadow\n").is_err());

        let opts = StartupOptions {
            picker_mode: Some(FilePickerMode::SaveFile {
                default_name: "report.pdf".to_owned(),
            }),
            ..StartupOptions::default()
        };
        let mut state = FilesAppState::new(UserFileLocations::detect(), &opts);
        assert!(state.picker_filename_focused);
        assert!(state.has_focused_text_input());
        state.cancel_file_picker();
        assert!(state.picker_cancelled);
        assert!(!state.picker_confirmed);
    }
}
