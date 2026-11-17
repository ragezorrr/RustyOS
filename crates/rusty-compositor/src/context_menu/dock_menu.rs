//! Dock right-click context menu builders, Downloads stack scanning, and Trash management (`RUSTYOS-064`).

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::UNIX_EPOCH;

use rusty_ipc::Urgency;
use rusty_wm::WindowId;
use tracing::warn;

use crate::config::DownloadsSortMode;
use crate::shell_render::{ContextMenuAction, ContextMenuEntry, DockHit};
use crate::surfaces::UiGlyphKind;

pub(crate) const MAX_DOWNLOADS_MENU_ITEMS: usize = 8;
pub(crate) const MAX_DOWNLOADS_SCAN_ENTRIES: usize = 128;

#[derive(Debug, Clone)]
pub struct DownloadsMenuEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub created_secs: u64,
    pub modified_secs: u64,
    pub size_bytes: u64,
    pub extension: String,
}

#[allow(clippy::unnecessary_wraps)]
#[must_use]
pub fn resolve_downloads_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_DOWNLOAD_DIR")
        && !dir.is_empty()
    {
        return Some(PathBuf::from(dir));
    }
    let detected = rusty_services::file_manager::UserFileLocations::detect().downloads;
    if detected.is_dir() {
        return Some(detected);
    }
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        let localized_ru = home.join("Загрузки");
        if localized_ru.is_dir() {
            return Some(localized_ru);
        }
        let en = home.join("Downloads");
        return Some(en);
    }
    Some(detected)
}

#[must_use]
pub fn resolve_trash_root() -> Option<PathBuf> {
    if let Some(data_home) = std::env::var_os("XDG_DATA_HOME")
        && !data_home.is_empty()
    {
        return Some(PathBuf::from(data_home).join("Trash"));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share/Trash"))
}

#[must_use]
pub fn read_downloads_entries(
    downloads_dir: &Path,
    sort_mode: DownloadsSortMode,
    max_items: usize,
) -> Vec<DownloadsMenuEntry> {
    let Ok(read_dir) = fs::read_dir(downloads_dir) else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    for item in read_dir.take(MAX_DOWNLOADS_SCAN_ENTRIES) {
        let Ok(entry) = item else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let is_dir = meta.is_dir();
        let size_bytes = if is_dir { 0 } else { meta.len() };
        let modified_secs = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs());
        let created_secs = meta
            .created()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(modified_secs, |d| d.as_secs());
        let extension = if is_dir {
            "DIR".to_owned()
        } else {
            Path::new(&name)
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.chars().take(4).collect::<String>().to_ascii_uppercase())
                .filter(|ext| !ext.is_empty())
                .unwrap_or_else(|| "FILE".to_owned())
        };
        entries.push(DownloadsMenuEntry {
            name,
            path: entry.path(),
            is_dir,
            created_secs,
            modified_secs,
            size_bytes,
            extension,
        });
    }
    match sort_mode {
        DownloadsSortMode::DateAdded => entries.sort_by(|a, b| {
            b.created_secs.cmp(&a.created_secs).then_with(|| {
                a.name
                    .to_ascii_lowercase()
                    .cmp(&b.name.to_ascii_lowercase())
            })
        }),
        DownloadsSortMode::DateModified => entries.sort_by(|a, b| {
            b.modified_secs.cmp(&a.modified_secs).then_with(|| {
                a.name
                    .to_ascii_lowercase()
                    .cmp(&b.name.to_ascii_lowercase())
            })
        }),
        DownloadsSortMode::Name => entries.sort_by(|a, b| {
            a.name
                .to_ascii_lowercase()
                .cmp(&b.name.to_ascii_lowercase())
        }),
        DownloadsSortMode::Size => entries.sort_by(|a, b| {
            b.size_bytes.cmp(&a.size_bytes).then_with(|| {
                a.name
                    .to_ascii_lowercase()
                    .cmp(&b.name.to_ascii_lowercase())
            })
        }),
        DownloadsSortMode::Kind => entries.sort_by(|a, b| {
            b.is_dir
                .cmp(&a.is_dir)
                .then_with(|| a.extension.cmp(&b.extension))
                .then_with(|| {
                    a.name
                        .to_ascii_lowercase()
                        .cmp(&b.name.to_ascii_lowercase())
                })
        }),
    }
    entries.truncate(max_items);
    entries
}

#[must_use]
pub fn read_downloads_stack_entries(
    downloads_dir: &Path,
    sort_mode: DownloadsSortMode,
    max_items: usize,
) -> Vec<crate::shell_render::DockStackEntry> {
    read_downloads_entries(downloads_dir, sort_mode, max_items)
        .into_iter()
        .map(|entry| {
            let detail = if entry.is_dir {
                "Folder".to_owned()
            } else {
                format_bytes_short(entry.size_bytes, false)
            };
            crate::shell_render::DockStackEntry {
                name: entry.name,
                path: entry.path,
                is_dir: entry.is_dir,
                detail,
                extension: entry.extension,
            }
        })
        .collect()
}

#[must_use]
pub fn trash_item_count(trash_root: &Path) -> usize {
    let files_dir = trash_root.join("files");
    let Ok(read_dir) = fs::read_dir(files_dir) else {
        return 0;
    };
    read_dir.filter_map(Result::ok).count()
}

/// Safely empties the trash directory (`RUSTYOS-064`).
///
/// # Errors
///
/// Returns an [`std::io::Error`] if `trash_root` is invalid or a file/directory deletion fails.
pub fn empty_trash_dir_safely(trash_root: &Path) -> std::io::Result<usize> {
    if trash_root.file_name().and_then(|n| n.to_str()) != Some("Trash") {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Refusing to empty non-Trash directory",
        ));
    }
    let mut removed = 0usize;
    for sub in ["files", "info"] {
        let subdir = trash_root.join(sub);
        if !subdir.exists() {
            continue;
        }
        let meta = fs::symlink_metadata(&subdir)?;
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Unsafe Trash subdirectory entry",
            ));
        }
        for entry in fs::read_dir(&subdir)? {
            let entry = entry?;
            let path = entry.path();
            let entry_meta = fs::symlink_metadata(&path)?;
            if entry_meta.file_type().is_symlink() || !entry_meta.is_dir() {
                fs::remove_file(&path)?;
            } else {
                fs::remove_dir_all(&path)?;
            }
            if sub == "files" {
                removed += 1;
            }
        }
    }
    Ok(removed)
}

#[must_use]
pub fn format_bytes_short(bytes: u64, is_dir: bool) -> String {
    if is_dir {
        return "Folder".to_owned();
    }
    if bytes >= 1_048_576 {
        format!("{} MB", bytes / 1_048_576)
    } else if bytes >= 1024 {
        format!("{} KB", (bytes / 1024).max(1))
    } else {
        format!("{bytes} B")
    }
}

#[must_use]
pub fn app_new_window_label_and_icon(app_key: &str, label: &str) -> (String, UiGlyphKind) {
    match app_key.trim().to_ascii_lowercase().as_str() {
        "files" | "rusty-files" => ("New Files Window".to_owned(), UiGlyphKind::Folder),
        "terminal" => (
            "New Terminal Window".to_owned(),
            UiGlyphKind::TerminalPrompt,
        ),
        "browser" => ("New Browser Window".to_owned(), UiGlyphKind::Search),
        "notes" => ("New Note".to_owned(), UiGlyphKind::Plus),
        "settings" => (
            "Open System Settings".to_owned(),
            UiGlyphKind::ControlCenter,
        ),
        _ => (format!("New {label} Window"), UiGlyphKind::Plus),
    }
}

fn command_basename_lower(cmd: &str) -> String {
    let token = cmd.split_whitespace().next().unwrap_or("");
    Path::new(token)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(token)
        .to_ascii_lowercase()
}

fn autostart_matches_candidates(entry: &str, candidates: &[String]) -> bool {
    let base = command_basename_lower(entry);
    !base.is_empty() && candidates.iter().any(|c| base.eq_ignore_ascii_case(c))
}

pub(crate) fn shell_quote_single(raw: &str) -> String {
    if !raw.is_empty()
        && raw
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '-' | '_' | '.'))
    {
        return raw.to_owned();
    }
    let escaped = raw.replace('\'', "'\"'\"'");
    format!("'{escaped}'")
}

#[must_use]
pub fn build_downloads_entries_from_list(
    files: Vec<DownloadsMenuEntry>,
    current_sort: DownloadsSortMode,
) -> Vec<ContextMenuEntry> {
    build_downloads_entries_with_modes(
        files,
        current_sort,
        crate::config::DownloadsDisplayMode::default(),
        crate::config::DownloadsViewMode::default(),
    )
}

#[must_use]
pub fn build_downloads_entries_with_modes(
    files: Vec<DownloadsMenuEntry>,
    current_sort: DownloadsSortMode,
    display_mode: crate::config::DownloadsDisplayMode,
    view_mode: crate::config::DownloadsViewMode,
) -> Vec<ContextMenuEntry> {
    let mut entries = Vec::new();
    if files.is_empty() {
        entries.push(
            ContextMenuEntry::item(
                "Empty Downloads",
                UiGlyphKind::Folder,
                ContextMenuAction::DownloadsOpenFolder,
            )
            .with_enabled(false),
        );
    } else {
        for item in files {
            let icon = if item.is_dir {
                UiGlyphKind::Folder
            } else {
                UiGlyphKind::Info
            };
            entries.push(
                ContextMenuEntry::item(
                    item.name,
                    icon,
                    ContextMenuAction::DownloadsOpenEntry(item.path),
                )
                .with_hint(format_bytes_short(item.size_bytes, item.is_dir)),
            );
        }
    }

    let sort_sub: Vec<ContextMenuEntry> = [
        DownloadsSortMode::DateAdded,
        DownloadsSortMode::DateModified,
        DownloadsSortMode::Name,
        DownloadsSortMode::Size,
        DownloadsSortMode::Kind,
    ]
    .into_iter()
    .map(|mode| {
        ContextMenuEntry::item(
            mode.label(),
            UiGlyphKind::LayoutSplit,
            ContextMenuAction::DownloadsSetSort(mode),
        )
        .with_checked(current_sort == mode)
    })
    .collect();

    let display_sub: Vec<ContextMenuEntry> = [
        crate::config::DownloadsDisplayMode::Stack,
        crate::config::DownloadsDisplayMode::Folder,
    ]
    .into_iter()
    .map(|mode| {
        ContextMenuEntry::item(
            mode.label(),
            UiGlyphKind::Folder,
            ContextMenuAction::DownloadsSetDisplay(mode),
        )
        .with_checked(display_mode == mode)
    })
    .collect();

    let view_sub: Vec<ContextMenuEntry> = [
        crate::config::DownloadsViewMode::Automatic,
        crate::config::DownloadsViewMode::Fan,
        crate::config::DownloadsViewMode::Grid,
        crate::config::DownloadsViewMode::List,
    ]
    .into_iter()
    .map(|mode| {
        ContextMenuEntry::item(
            mode.label(),
            UiGlyphKind::Grid,
            ContextMenuAction::DownloadsSetView(mode),
        )
        .with_checked(view_mode == mode)
    })
    .collect();

    entries.push(ContextMenuEntry::Separator);
    entries.push(ContextMenuEntry::submenu_item(
        "Sort By",
        UiGlyphKind::LayoutSplit,
        sort_sub,
    ));
    entries.push(ContextMenuEntry::submenu_item(
        "Display as",
        UiGlyphKind::Folder,
        display_sub,
    ));
    entries.push(ContextMenuEntry::submenu_item(
        "View content as",
        UiGlyphKind::Grid,
        view_sub,
    ));
    entries.push(ContextMenuEntry::Separator);
    entries.push(ContextMenuEntry::item(
        "Open \"Downloads\"",
        UiGlyphKind::Folder,
        ContextMenuAction::DownloadsOpenFolder,
    ));
    entries
}

#[must_use]
pub fn build_trash_context_entries() -> Vec<ContextMenuEntry> {
    let count = resolve_trash_root().map_or(0, |root| trash_item_count(&root));
    let empty_label = if count > 0 {
        format!("Empty Trash... ({count})")
    } else {
        "Empty Trash...".to_owned()
    };
    vec![
        ContextMenuEntry::item(
            "Open Trash",
            UiGlyphKind::Folder,
            ContextMenuAction::TrashOpen,
        ),
        ContextMenuEntry::Separator,
        ContextMenuEntry::item(
            empty_label,
            UiGlyphKind::Trash,
            ContextMenuAction::TrashEmpty,
        )
        .with_enabled(count > 0)
        .with_danger(count > 0),
    ]
}

impl crate::state::RustyCompositorState {
    pub(crate) fn autostart_candidates_for_key(&self, app_key: &str) -> Vec<String> {
        let key = app_key.trim().to_ascii_lowercase();
        if key == "settings" {
            return ["rusty-settings", "gnome-control-center", "systemsettings"]
                .into_iter()
                .map(ToOwned::to_owned)
                .collect();
        }
        if let Some(bins) = crate::launcher::LauncherState::canonical_preferred_bins(&key) {
            return bins.iter().map(|s| (*s).to_owned()).collect();
        }
        self.launcher
            .find_command_for_dock_key(&key)
            .into_iter()
            .collect()
    }

    pub(crate) fn is_dock_key_in_autostart(&self, app_key: &str) -> bool {
        let candidates = self.autostart_candidates_for_key(app_key);
        if candidates.is_empty() {
            return false;
        }
        self.user_config
            .autostart
            .iter()
            .any(|cmd| autostart_matches_candidates(cmd, &candidates))
    }

    pub(crate) fn matching_windows_for_dock_key(&self, app_key: &str) -> Vec<WindowId> {
        let key = app_key.trim().to_ascii_lowercase();
        self.ordered_visible_window_ids()
            .into_iter()
            .filter(|&win_id| {
                let title = self.window_title(win_id);
                let app_id = self.window_app_id(win_id);
                crate::shell_render::canonical_dock_app_key(app_id.as_deref(), &title, win_id)
                    == key
            })
            .collect()
    }

    pub(crate) fn build_dock_app_context_entries(
        &self,
        hit: DockHit,
        tiles: &[crate::shell_render::DockAppTile],
        alt_held: bool,
    ) -> Option<Vec<ContextMenuEntry>> {
        let (app_key, label) = tiles
            .iter()
            .find(|t| t.hit == hit)
            .map(|t| (t.app_key.clone(), t.label.clone()))
            .or_else(|| {
                self.dock_key_for_hit(hit)
                    .map(|k| (k.clone(), crate::shell_render::dock_label_for_key(&k)))
            })?;
        let matching = self.matching_windows_for_dock_key(&app_key);
        let focused = self.wm.focused_window();
        let mut entries = Vec::new();

        if !matching.is_empty() {
            for &win_id in &matching {
                let minimized = self.is_window_minimized(win_id);
                let glyph = if minimized {
                    UiGlyphKind::WindowMin
                } else {
                    UiGlyphKind::WindowMax
                };
                entries.push(
                    ContextMenuEntry::item(
                        self.window_title(win_id),
                        glyph,
                        ContextMenuAction::DockFocusWindow(win_id),
                    )
                    .with_checked(!minimized && focused == Some(win_id)),
                );
            }
            entries.push(ContextMenuEntry::Separator);
        }

        let (launch_label, icon) = app_new_window_label_and_icon(&app_key, &label);
        entries.push(ContextMenuEntry::item(
            launch_label,
            icon,
            ContextMenuAction::DockLaunchApp(hit),
        ));
        entries.push(ContextMenuEntry::Separator);

        let is_pinned = self
            .user_config
            .dock_pinned_apps
            .iter()
            .any(|p| p.eq_ignore_ascii_case(&app_key));
        let keep_label = if is_pinned {
            "Remove from Dock"
        } else {
            "Keep in Dock"
        };
        let removable = crate::shell_render::is_dock_pin_removable(&app_key);
        let in_autostart = self.is_dock_key_in_autostart(&app_key);

        let options_sub = vec![
            ContextMenuEntry::item(
                keep_label,
                UiGlyphKind::Plus,
                ContextMenuAction::DockToggleKeepInDock(hit),
            )
            .with_checked(is_pinned)
            .with_enabled(removable || !is_pinned),
            ContextMenuEntry::item(
                "Open at Login",
                UiGlyphKind::Refresh,
                ContextMenuAction::DockToggleOpenAtLogin(hit),
            )
            .with_checked(in_autostart),
            ContextMenuEntry::item(
                "Show All Windows",
                UiGlyphKind::Grid,
                ContextMenuAction::DockShowAppWindows(hit),
            ),
        ];
        entries.push(ContextMenuEntry::submenu_item(
            "Options",
            UiGlyphKind::ControlCenter,
            options_sub,
        ));

        let has_running = !matching.is_empty() || (app_key == "settings" && self.settings_active);
        if has_running {
            entries.push(ContextMenuEntry::Separator);
            let quit_entry = if alt_held {
                ContextMenuEntry::item(
                    "Force Quit",
                    UiGlyphKind::Close,
                    ContextMenuAction::DockForceQuitApp(hit),
                )
                .with_danger(true)
            } else {
                ContextMenuEntry::item(
                    "Quit",
                    UiGlyphKind::Close,
                    ContextMenuAction::DockQuitApp(hit),
                )
            };
            entries.push(quit_entry);
        }
        Some(entries)
    }

    pub(crate) fn build_overview_context_entries(&self) -> Vec<ContextMenuEntry> {
        let active_ws = self.wm.active_workspace_id().0;
        let ws_sub: Vec<ContextMenuEntry> = (1..=9_u8)
            .map(|ws| {
                ContextMenuEntry::item(
                    format!("Desktop {ws}"),
                    UiGlyphKind::OverviewSpaces,
                    ContextMenuAction::SwitchWorkspace(ws),
                )
                .with_checked(ws == active_ws)
            })
            .collect();
        let ov_label = if self.overview_active {
            "Exit Mission Control"
        } else {
            "Mission Control"
        };
        vec![
            ContextMenuEntry::item(
                ov_label,
                UiGlyphKind::Grid,
                ContextMenuAction::OverviewToggle,
            ),
            ContextMenuEntry::Separator,
            ContextMenuEntry::submenu_item("Switch Desktop", UiGlyphKind::OverviewSpaces, ws_sub),
        ]
    }

    pub(crate) fn build_downloads_context_entries(&self) -> Vec<ContextMenuEntry> {
        let current_sort = self.user_config.downloads_sort;
        let files = resolve_downloads_dir().map_or_default(|dir| {
            read_downloads_entries(&dir, current_sort, MAX_DOWNLOADS_MENU_ITEMS)
        });
        build_downloads_entries_with_modes(
            files,
            current_sort,
            self.user_config.dock_downloads_display,
            self.user_config.dock_downloads_view,
        )
    }

    pub(crate) fn refresh_downloads_context_menu(&mut self) {
        if self.context_menu.is_some() {
            let entries = self.build_downloads_context_entries();
            if let Some(ref mut ctx) = self.context_menu {
                ctx.entries = entries;
                ctx.open_submenu_parent = None;
                ctx.submenu_selected_row = None;
                ctx.selected_row = None;
            }
        }
        self.shell_redraw_needed = true;
    }

    pub(crate) fn toggle_dock_app_autostart(&mut self, hit: DockHit) {
        let Some(app_key) = self.dock_key_for_hit(hit) else {
            return;
        };
        let candidates = self.autostart_candidates_for_key(&app_key);
        let in_autostart = self.is_dock_key_in_autostart(&app_key);

        if in_autostart {
            self.user_config
                .autostart
                .retain(|entry| !autostart_matches_candidates(entry, &candidates));
        } else {
            let command = candidates.first().map_or_else(
                || format!("rusty-{app_key}"),
                |c| {
                    if c.contains(' ') {
                        shell_quote_single(c)
                    } else {
                        c.clone()
                    }
                },
            );
            if !self.user_config.autostart.contains(&command) {
                self.user_config.autostart.push(command);
            }
        }
        self.sync_and_save_user_config();
    }

    pub(crate) fn quit_dock_app(&mut self, hit: DockHit, force: bool) {
        let Some(app_key) = self.dock_key_for_hit(hit) else {
            return;
        };
        if app_key == "settings" {
            self.close_settings();
        }
        let matching = self.matching_windows_for_dock_key(&app_key);
        if force {
            let self_pid = std::process::id();
            let pids: BTreeSet<u32> = matching
                .iter()
                .filter_map(|&id| self.window_client_pid(id))
                .filter(|&pid| pid > 1 && pid != self_pid)
                .collect();
            if pids.is_empty() {
                for id in matching {
                    self.close_window_by_id(id);
                }
            } else {
                self.kill_pids_gracefully_forced(&pids);
            }
        } else {
            for id in matching {
                self.close_window_by_id(id);
            }
        }
    }

    pub(crate) fn spawn_empty_trash_worker(&mut self) {
        let Some(trash_root) = resolve_trash_root() else {
            self.show_osd("Trash unavailable", None, false);
            return;
        };
        self.spawn_empty_trash_worker_at(trash_root);
    }

    pub(crate) fn spawn_empty_trash_worker_at(&mut self, trash_root: PathBuf) {
        if self.trash_empty_rx.is_some() {
            self.show_osd("Emptying Trash...", None, false);
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.trash_empty_rx = Some(rx);
        self.show_osd("Emptying Trash...", None, false);
        std::thread::spawn(move || {
            let result = empty_trash_dir_safely(&trash_root).map_err(|err| err.to_string());
            let _ = tx.send(result);
        });
    }

    /// Drains non-blocking background Trash cleanup completion without stalling the event loop (`RUSTYOS-064`).
    pub fn drain_trash_empty_results(&mut self) {
        let Some(ref rx) = self.trash_empty_rx else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(count)) => {
                self.trash_empty_rx = None;
                let msg = if count == 0 {
                    "Trash Already Empty".to_owned()
                } else {
                    format!("Emptied Trash ({count})")
                };
                let _ =
                    self.push_notification(Urgency::Low, "Trash Emptied".to_owned(), msg.clone());
                self.show_osd(msg, None, false);
                self.shell_redraw_needed = true;
            }
            Ok(Err(err)) => {
                self.trash_empty_rx = None;
                warn!(error = %err, "Background Trash cleanup failed");
                let _ =
                    self.push_notification(Urgency::Critical, "Empty Trash Failed".to_owned(), err);
                self.show_osd("Empty Trash Failed", None, false);
                self.shell_redraw_needed = true;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.trash_empty_rx = None;
            }
        }
    }
}
