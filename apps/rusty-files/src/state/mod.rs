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
use std::time::Instant;

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
    pub sidebar_items: Vec<SidebarRowItem>,
    pub active_sidebar: Option<SidebarLocation>,
    pub view_mode: FileViewMode,
    pub theme: FilesThemeMode,
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
    pub window_title: String,
    pub window_size: WindowSizeState,
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
}

impl FilesAppState {
    /// Creates application state using XDG locations, persisted `file_tags.toml`, and CLI options.
    #[must_use]
    pub(crate) fn new(locations: UserFileLocations, opts: &StartupOptions) -> Self {
        let tags_config = FileTagsConfig::load_from_path(&locations.tags_config_path());
        let volumes = discover_mounts();
        let (result_tx, result_rx) = mpsc::channel();
        let (preview_tx, preview_rx) = mpsc::channel();
        let (info_tx, info_rx) = mpsc::channel();
        let (transfer_tx, transfer_rx) = mpsc::channel();
        let (home_search_tx, home_search_rx) = mpsc::channel();
        let sidebar_items = build_sidebar_items(&locations, &volumes);

        let theme = opts
            .theme_override
            .or_else(|| locations.detect_system_theme())
            .unwrap_or(tags_config.theme);
        let view_mode = tags_config.view_mode;

        let mut app = Self {
            sort_field: tags_config.sort_field,
            sort_order: tags_config.sort_order,
            show_hidden: tags_config.show_hidden,
            show_path_bar: tags_config.show_path_bar,
            locations,
            tags_config,
            volumes,
            sidebar_items,
            active_sidebar: None,
            view_mode,
            theme,
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
            status_message: None,
            window_title: String::from("Files"),
            window_size: WindowSizeState::Normal,
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
        };

        if opts.open_trash {
            app.navigate_sidebar(SidebarLocation::Trash, false);
        } else if opts.open_recents {
            app.navigate_sidebar(SidebarLocation::Recents, false);
        } else if let Some(path) = &opts.initial_path {
            app.open_initial_path(path);
        } else {
            app.navigate_sidebar(SidebarLocation::Home, false);
        }
        app.tabs.push(app.snapshot_active_tab());
        if opts.open_test_popup {
            app.popup = Some(OpenPopupMenu::ActionsMenu);
            app.dirty = true;
        }
        app
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

pub(crate) fn build_sidebar_items(
    locations: &UserFileLocations,
    volumes: &[VolumeInfo],
) -> Vec<SidebarRowItem> {
    let mut items = vec![
        SidebarRowItem {
            section: None,
            label: String::from("Recents"),
            location: SidebarLocation::Recents,
            icon: SidebarIconKind::Recents,
        },
        SidebarRowItem {
            section: None,
            label: String::from("Shared"),
            location: SidebarLocation::Shared,
            icon: SidebarIconKind::Shared,
        },
        SidebarRowItem {
            section: Some("Favorites"),
            label: String::from("Applications"),
            location: SidebarLocation::Applications,
            icon: SidebarIconKind::Applications,
        },
        SidebarRowItem {
            section: None,
            label: String::from("Desktop"),
            location: SidebarLocation::Desktop,
            icon: SidebarIconKind::Desktop,
        },
        SidebarRowItem {
            section: None,
            label: String::from("Documents"),
            location: SidebarLocation::Documents,
            icon: SidebarIconKind::Documents,
        },
        SidebarRowItem {
            section: None,
            label: String::from("Downloads"),
            location: SidebarLocation::Downloads,
            icon: SidebarIconKind::Downloads,
        },
        SidebarRowItem {
            section: Some("Locations"),
            label: locations.user_label.clone(),
            location: SidebarLocation::Home,
            icon: SidebarIconKind::Home,
        },
    ];

    for vol in volumes {
        if !vol.is_network {
            items.push(SidebarRowItem {
                section: None,
                label: vol.name.clone(),
                location: SidebarLocation::Volume(vol.mount_point.clone()),
                icon: SidebarIconKind::Volume,
            });
        }
    }

    items.push(SidebarRowItem {
        section: None,
        label: String::from("Network"),
        location: SidebarLocation::Network,
        icon: SidebarIconKind::Network,
    });
    items.push(SidebarRowItem {
        section: None,
        label: String::from("Trash"),
        location: SidebarLocation::Trash,
        icon: SidebarIconKind::Trash,
    });

    for (idx, tag) in FileTagColor::ALL.iter().enumerate() {
        items.push(SidebarRowItem {
            section: (idx == 0).then_some("Tags"),
            label: String::from(tag.label()),
            location: SidebarLocation::Tag(*tag),
            icon: SidebarIconKind::Tag(*tag),
        });
    }
    items
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
        assert_eq!(state.theme, FilesThemeMode::LightGoldenGate);
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
        assert_eq!(state.theme, FilesThemeMode::DarkPurpleGlass);
        state.toggle_show_hidden();
        assert!(state.show_hidden);
        state.set_view_mode(FileViewMode::List);
        assert_eq!(state.view_mode, FileViewMode::List);

        let notes_path = locs.documents.join("notes.txt");
        state.toggle_tag_on_path(&notes_path, FileTagColor::Green);
        assert_eq!(state.tags_for(&notes_path), &[FileTagColor::Green]);

        // Reload fresh app state -> persisted preferences & tags are restored
        let reloaded = FilesAppState::new(locs.clone(), &StartupOptions::default());
        assert_eq!(reloaded.theme, FilesThemeMode::DarkPurpleGlass);
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
}
