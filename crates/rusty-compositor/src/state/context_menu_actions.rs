//! Right-click Context Menu builders, actions, and safe background Trash cleanup (`RUSTYOS-064`).

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::time::UNIX_EPOCH;

use rusty_ipc::Urgency;
use rusty_wm::{LayoutMode, OutputId, WindowId, WmAction, WorkspaceId};
use smithay::reexports::wayland_server::Resource;
use smithay::utils::{Logical, Point, Serial, Size};
use tracing::{info, warn};

use crate::config::DownloadsSortMode;
use crate::shell_render::{
    ContextMenuAction, ContextMenuEntry, ContextMenuState, ContextMenuTarget, DOCK_PINNED_COUNT,
    DockHit, dock_slot_rect,
};
use crate::surfaces::UiGlyphKind;

use super::RustyCompositorState;

const MAX_DOWNLOADS_MENU_ITEMS: usize = 8;
const MAX_DOWNLOADS_SCAN_ENTRIES: usize = 128;

#[derive(Debug, Clone)]
pub(crate) struct DownloadsMenuEntry {
    pub(crate) name: String,
    pub(crate) path: PathBuf,
    pub(crate) is_dir: bool,
    pub(crate) modified_secs: u64,
    pub(crate) size_bytes: u64,
}

pub(crate) fn resolve_desktop_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_DESKTOP_DIR")
        && !dir.is_empty()
    {
        return Some(PathBuf::from(dir));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Desktop"))
}

pub(crate) fn resolve_downloads_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_DOWNLOAD_DIR")
        && !dir.is_empty()
    {
        return Some(PathBuf::from(dir));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Downloads"))
}

pub(crate) fn resolve_trash_root() -> Option<PathBuf> {
    if let Some(data_home) = std::env::var_os("XDG_DATA_HOME")
        && !data_home.is_empty()
    {
        return Some(PathBuf::from(data_home).join("Trash"));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share/Trash"))
}

pub(crate) fn create_unique_desktop_folder(desktop_dir: &Path) -> std::io::Result<PathBuf> {
    fs::create_dir_all(desktop_dir)?;
    for idx in 1..=999_u32 {
        let name = if idx == 1 {
            "New Folder".to_owned()
        } else {
            format!("New Folder {idx}")
        };
        let candidate = desktop_dir.join(name);
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(err),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "Exhausted New Folder candidate names",
    ))
}

pub(crate) fn read_downloads_entries(
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
        entries.push(DownloadsMenuEntry {
            name,
            path: entry.path(),
            is_dir,
            modified_secs,
            size_bytes,
        });
    }
    match sort_mode {
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
    }
    entries.truncate(max_items);
    entries
}

pub(crate) fn trash_item_count(trash_root: &Path) -> usize {
    let files_dir = trash_root.join("files");
    let Ok(read_dir) = fs::read_dir(files_dir) else {
        return 0;
    };
    read_dir.filter_map(Result::ok).count()
}

pub(crate) fn empty_trash_dir_safely(trash_root: &Path) -> std::io::Result<usize> {
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

fn format_bytes_short(bytes: u64, is_dir: bool) -> String {
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

fn dock_slot_index_for_hit(hit: DockHit, window_count: usize) -> Option<usize> {
    let left_count = DOCK_PINNED_COUNT + window_count;
    match hit {
        DockHit::PinnedFiles => Some(0),
        DockHit::ToggleOverview => Some(1),
        DockHit::PinnedTerminal => Some(2),
        DockHit::PinnedBrowser => Some(3),
        DockHit::PinnedNotes => Some(4),
        DockHit::PinnedSettings => Some(5),
        DockHit::WindowSlot(idx) if idx < window_count => Some(DOCK_PINNED_COUNT + idx),
        DockHit::PinnedDownloads => Some(left_count),
        DockHit::PinnedTrash => Some(left_count + 1),
        DockHit::WindowSlot(_) => None,
    }
}

fn pinned_app_label_and_icon(hit: DockHit) -> (&'static str, UiGlyphKind) {
    match hit {
        DockHit::PinnedFiles => ("New Files Window", UiGlyphKind::Folder),
        DockHit::PinnedTerminal => ("New Terminal Window", UiGlyphKind::TerminalPrompt),
        DockHit::PinnedBrowser => ("New Browser Window", UiGlyphKind::Search),
        DockHit::PinnedNotes => ("New Note", UiGlyphKind::Plus),
        DockHit::PinnedSettings => ("Open System Settings", UiGlyphKind::ControlCenter),
        _ => ("Open", UiGlyphKind::Info),
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

fn autostart_matches_candidates(entry: &str, candidates: &[&str]) -> bool {
    let base = command_basename_lower(entry);
    !base.is_empty() && candidates.iter().any(|c| base.eq_ignore_ascii_case(c))
}

fn shell_quote_single(raw: &str) -> String {
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

fn toggle_quit_force_in_entries(entries: &mut [ContextMenuEntry], alt_held: bool) -> bool {
    let mut changed = false;
    for entry in entries {
        let ContextMenuEntry::Item {
            label,
            danger,
            action,
            submenu,
            ..
        } = entry
        else {
            continue;
        };
        match action {
            ContextMenuAction::WindowQuit(id) if alt_held => {
                *action = ContextMenuAction::WindowForceQuit(*id);
                "Force Quit".clone_into(label);
                *danger = true;
                changed = true;
            }
            ContextMenuAction::WindowForceQuit(id) if !alt_held => {
                *action = ContextMenuAction::WindowQuit(*id);
                "Quit".clone_into(label);
                *danger = false;
                changed = true;
            }
            ContextMenuAction::DockQuitApp(hit) if alt_held => {
                *action = ContextMenuAction::DockForceQuitApp(*hit);
                "Force Quit".clone_into(label);
                *danger = true;
                changed = true;
            }
            ContextMenuAction::DockForceQuitApp(hit) if !alt_held => {
                *action = ContextMenuAction::DockQuitApp(*hit);
                "Quit".clone_into(label);
                *danger = false;
                changed = true;
            }
            _ => {}
        }
        if toggle_quit_force_in_entries(submenu, alt_held) {
            changed = true;
        }
    }
    changed
}

impl RustyCompositorState {
    /// Closes any active right-click context menu (`RUSTYOS-064`).
    pub fn close_context_menu(&mut self) {
        if self.context_menu.take().is_some() {
            self.shell_redraw_needed = true;
        }
    }

    /// Dynamically swaps `Quit` <-> `Force Quit` when `Alt` is pressed or released while open (`RUSTYOS-064`).
    pub fn sync_context_menu_alt_modifier(&mut self, alt_held: bool) {
        if let Some(ref mut ctx) = self.context_menu
            && toggle_quit_force_in_entries(&mut ctx.entries, alt_held)
        {
            self.shell_redraw_needed = true;
        }
    }

    fn output_and_local_anchor(
        &self,
        global_pos: Point<f64, Logical>,
    ) -> (OutputId, Point<i32, Logical>) {
        let target = self
            .output_map
            .output_under(global_pos)
            .and_then(|id| self.output_map.get(id))
            .unwrap_or_else(|| self.output_map.primary());
        let geo = target.logical_geometry();
        #[allow(clippy::cast_possible_truncation)]
        let local = Point::from((
            (global_pos.x - f64::from(geo.loc.x)).round() as i32,
            (global_pos.y - f64::from(geo.loc.y)).round() as i32,
        ));
        (target.id, local)
    }

    fn install_context_menu(
        &mut self,
        output_id: OutputId,
        anchor_local: Point<i32, Logical>,
        target: ContextMenuTarget,
        entries: Vec<ContextMenuEntry>,
    ) {
        self.close_top_bar_menu();
        self.clear_shell_hover_states();
        self.context_menu = Some(ContextMenuState::new(
            output_id,
            anchor_local,
            target,
            entries,
        ));
        self.shell_redraw_needed = true;
    }

    /// Opens the macOS-aligned Desktop right-click context menu at `global_pos` (`RUSTYOS-064`).
    pub fn open_desktop_context_menu(&mut self, global_pos: Point<f64, Logical>) {
        if self.is_session_locked() {
            return;
        }
        let (output_id, anchor_local) = self.output_and_local_anchor(global_pos);
        let tiling_active = self.wm.active_layout_mode() == LayoutMode::MasterStack;
        let entries = vec![
            ContextMenuEntry::item(
                "New Folder",
                UiGlyphKind::Plus,
                ContextMenuAction::DesktopNewFolder,
            ),
            ContextMenuEntry::item(
                "Open Desktop in Files",
                UiGlyphKind::Folder,
                ContextMenuAction::DesktopOpenFiles,
            ),
            ContextMenuEntry::Separator,
            ContextMenuEntry::item(
                "Change Wallpaper...",
                UiGlyphKind::Sun,
                ContextMenuAction::DesktopChangeWallpaper,
            ),
            ContextMenuEntry::item(
                "Display Settings...",
                UiGlyphKind::ControlCenter,
                ContextMenuAction::DesktopDisplaySettings,
            ),
            ContextMenuEntry::Separator,
            ContextMenuEntry::item(
                "Open Terminal",
                UiGlyphKind::TerminalPrompt,
                ContextMenuAction::DesktopOpenTerminal,
            )
            .with_hint("Alt+Return"),
            ContextMenuEntry::item(
                "Mission Control",
                UiGlyphKind::Grid,
                ContextMenuAction::DesktopMissionControl,
            ),
            ContextMenuEntry::item(
                "Tile Windows",
                UiGlyphKind::LayoutSplit,
                ContextMenuAction::DesktopToggleLayoutMode,
            )
            .with_hint("Alt+L")
            .with_checked(tiling_active),
        ];
        self.install_context_menu(output_id, anchor_local, ContextMenuTarget::Desktop, entries);
    }

    /// Opens the macOS-aligned Dock slot context menu (`RUSTYOS-064`).
    pub fn open_dock_context_menu(
        &mut self,
        hit: DockHit,
        dock_screen_size: Size<i32, smithay::utils::Physical>,
        global_pos: Point<f64, Logical>,
        alt_held: bool,
    ) {
        if self.is_session_locked() {
            return;
        }
        let primary = self.output_map.primary();
        let output_id = primary.id;
        let geo = primary.logical_geometry();
        let win_count = self.ordered_visible_window_ids().len();
        let anchor_local = dock_slot_index_for_hit(hit, win_count)
            .and_then(|slot| {
                dock_slot_rect(dock_screen_size.w, dock_screen_size.h, win_count, slot)
            })
            .map_or_else(
                || {
                    #[allow(clippy::cast_possible_truncation)]
                    Point::from((
                        (global_pos.x - f64::from(geo.loc.x)).round() as i32,
                        (global_pos.y - f64::from(geo.loc.y)).round() as i32,
                    ))
                },
                |rect| Point::from((rect.loc.x, rect.loc.y - 6)),
            );

        let entries = match hit {
            DockHit::PinnedFiles
            | DockHit::PinnedTerminal
            | DockHit::PinnedBrowser
            | DockHit::PinnedNotes
            | DockHit::PinnedSettings => self.build_pinned_app_context_entries(hit, alt_held),
            DockHit::ToggleOverview => self.build_overview_context_entries(),
            DockHit::WindowSlot(idx) => {
                let Some(items) = self.build_window_slot_context_entries(idx, alt_held) else {
                    return;
                };
                items
            }
            DockHit::PinnedDownloads => self.build_downloads_context_entries(),
            DockHit::PinnedTrash => Self::build_trash_context_entries(),
        };
        self.install_context_menu(
            output_id,
            anchor_local,
            ContextMenuTarget::DockSlot(hit),
            entries,
        );
    }

    fn autostart_candidates_for_hit(hit: DockHit) -> Option<&'static [&'static str]> {
        if hit == DockHit::PinnedSettings {
            return Some(&["rusty-settings", "gnome-control-center", "systemsettings"]);
        }
        Self::dock_preferred_candidates(hit)
    }

    fn is_dock_app_in_autostart(&self, hit: DockHit) -> bool {
        let Some(candidates) = Self::autostart_candidates_for_hit(hit) else {
            return false;
        };
        self.user_config
            .autostart
            .iter()
            .any(|cmd| autostart_matches_candidates(cmd, candidates))
    }

    fn matching_windows_for_dock_app(&self, hit: DockHit) -> Vec<WindowId> {
        let Some(candidates) = Self::autostart_candidates_for_hit(hit) else {
            return Vec::new();
        };
        self.ordered_visible_window_ids()
            .into_iter()
            .filter(|&win_id| {
                let app_id = self
                    .window_app_id(win_id)
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                let title = self.window_title(win_id).to_ascii_lowercase();
                candidates
                    .iter()
                    .any(|cand| app_id.contains(cand) || title.contains(cand))
            })
            .collect()
    }

    fn build_pinned_app_context_entries(
        &self,
        hit: DockHit,
        alt_held: bool,
    ) -> Vec<ContextMenuEntry> {
        let (launch_label, icon) = pinned_app_label_and_icon(hit);
        let in_autostart = self.is_dock_app_in_autostart(hit);
        let has_running = !self.matching_windows_for_dock_app(hit).is_empty()
            || (hit == DockHit::PinnedSettings && self.settings_active);
        let options_sub = vec![
            ContextMenuEntry::item(
                "Open at Login",
                UiGlyphKind::Refresh,
                ContextMenuAction::DockToggleOpenAtLogin(hit),
            )
            .with_checked(in_autostart),
            ContextMenuEntry::item(
                "Show All Windows",
                UiGlyphKind::Grid,
                ContextMenuAction::DockShowAllWindows,
            ),
        ];

        let mut entries = vec![
            ContextMenuEntry::item(launch_label, icon, ContextMenuAction::DockLaunchApp(hit)),
            ContextMenuEntry::Separator,
            ContextMenuEntry::submenu_item("Options", UiGlyphKind::ControlCenter, options_sub),
        ];
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
        entries
    }

    fn build_overview_context_entries(&self) -> Vec<ContextMenuEntry> {
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

    fn build_window_slot_context_entries(
        &self,
        idx: usize,
        alt_held: bool,
    ) -> Option<Vec<ContextMenuEntry>> {
        let ids = self.ordered_visible_window_ids();
        let &win_id = ids.get(idx)?;
        let minimized = self.is_window_minimized(win_id);
        let focused = !minimized && self.wm.focused_window() == Some(win_id);
        let cur_ws = self.wm.window(win_id).map_or(1, |w| w.workspace.0);

        let focus_label = if minimized {
            "Restore Window"
        } else if focused {
            "Focused Window"
        } else {
            "Show Window"
        };
        let move_sub: Vec<ContextMenuEntry> = (1..=9_u8)
            .map(|ws| {
                ContextMenuEntry::item(
                    format!("Desktop {ws}"),
                    UiGlyphKind::OverviewSpaces,
                    ContextMenuAction::WindowMoveToWorkspace(win_id, ws),
                )
                .with_checked(ws == cur_ws)
            })
            .collect();

        let quit_entry = if alt_held {
            ContextMenuEntry::item(
                "Force Quit",
                UiGlyphKind::Close,
                ContextMenuAction::WindowForceQuit(win_id),
            )
            .with_danger(true)
        } else {
            ContextMenuEntry::item(
                "Quit",
                UiGlyphKind::Close,
                ContextMenuAction::WindowQuit(win_id),
            )
        };

        Some(vec![
            ContextMenuEntry::item(
                focus_label,
                UiGlyphKind::WindowMax,
                ContextMenuAction::WindowFocusOrRestore(win_id),
            )
            .with_checked(focused),
            ContextMenuEntry::item(
                "Minimize",
                UiGlyphKind::WindowMin,
                ContextMenuAction::WindowMinimize(win_id),
            )
            .with_enabled(!minimized),
            ContextMenuEntry::Separator,
            ContextMenuEntry::submenu_item(
                "Move to Desktop",
                UiGlyphKind::OverviewSpaces,
                move_sub,
            ),
            ContextMenuEntry::Separator,
            quit_entry,
        ])
    }

    fn build_downloads_context_entries(&self) -> Vec<ContextMenuEntry> {
        let current_sort = self.user_config.downloads_sort;
        let files = resolve_downloads_dir().map_or_default(|dir| {
            read_downloads_entries(&dir, current_sort, MAX_DOWNLOADS_MENU_ITEMS)
        });
        Self::build_downloads_entries_from_list(files, current_sort)
    }

    pub(crate) fn build_downloads_entries_from_list(
        files: Vec<DownloadsMenuEntry>,
        current_sort: DownloadsSortMode,
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
            DownloadsSortMode::DateModified,
            DownloadsSortMode::Name,
            DownloadsSortMode::Size,
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

        entries.push(ContextMenuEntry::Separator);
        entries.push(ContextMenuEntry::submenu_item(
            "Sort By",
            UiGlyphKind::LayoutSplit,
            sort_sub,
        ));
        entries.push(ContextMenuEntry::Separator);
        entries.push(ContextMenuEntry::item(
            "Open \"Downloads\"",
            UiGlyphKind::Folder,
            ContextMenuAction::DownloadsOpenFolder,
        ));
        entries
    }

    fn build_trash_context_entries() -> Vec<ContextMenuEntry> {
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

    /// Executes a selected action from the active [`ContextMenuState`] (`RUSTYOS-064`).
    pub fn execute_context_menu_action(&mut self, action: ContextMenuAction, serial: Serial) {
        let keep_open = matches!(action, ContextMenuAction::DownloadsSetSort(_));
        match action {
            ContextMenuAction::DesktopNewFolder => self.create_desktop_folder_action(),
            ContextMenuAction::DesktopOpenFiles => {
                if let Some(dir) = resolve_desktop_dir() {
                    self.launch_path_with_xdg_open(&dir);
                }
            }
            ContextMenuAction::DesktopChangeWallpaper => {
                self.settings_tab = crate::shell_render::SettingsTab::Appearance;
                self.open_settings();
            }
            ContextMenuAction::DesktopDisplaySettings => {
                self.settings_tab = crate::shell_render::SettingsTab::Displays;
                self.open_settings();
            }
            ContextMenuAction::DesktopOpenTerminal => {
                self.handle_dock_hit(DockHit::PinnedTerminal, serial);
            }
            ContextMenuAction::DesktopMissionControl | ContextMenuAction::DockShowAllWindows => {
                self.overview_active = false;
                self.toggle_overview();
            }
            ContextMenuAction::DesktopToggleLayoutMode => {
                self.apply_wm_action(WmAction::ToggleLayoutMode, serial);
            }
            ContextMenuAction::DockLaunchApp(hit) => {
                if hit == DockHit::PinnedSettings {
                    self.open_settings();
                } else {
                    self.handle_dock_hit(hit, serial);
                }
            }
            ContextMenuAction::DockToggleOpenAtLogin(hit) => {
                self.toggle_dock_app_autostart(hit);
            }
            ContextMenuAction::DockQuitApp(hit) => self.quit_dock_app(hit, false),
            ContextMenuAction::DockForceQuitApp(hit) => self.quit_dock_app(hit, true),
            ContextMenuAction::OverviewToggle => self.toggle_overview(),
            ContextMenuAction::SwitchWorkspace(ws) => {
                self.apply_wm_action(
                    WmAction::SwitchWorkspace(WorkspaceId::new_clamped(ws)),
                    serial,
                );
            }
            ContextMenuAction::WindowFocusOrRestore(win_id) => {
                if self.is_window_minimized(win_id) {
                    self.restore_window(win_id, serial);
                } else {
                    self.wm.focus_and_raise(win_id);
                    self.sync_space_with_wm(serial);
                }
            }
            ContextMenuAction::WindowMinimize(win_id) => self.minimize_window(win_id, serial),
            ContextMenuAction::WindowMoveToWorkspace(win_id, ws) => {
                if self.is_window_minimized(win_id) {
                    self.restore_window(win_id, serial);
                }
                self.wm.focus_and_raise(win_id);
                self.apply_wm_action(
                    WmAction::MoveFocusedToWorkspace(WorkspaceId::new_clamped(ws)),
                    serial,
                );
            }
            ContextMenuAction::WindowQuit(win_id) => self.close_window_by_id(win_id),
            ContextMenuAction::WindowForceQuit(win_id) => self.force_quit_window_by_id(win_id),
            ContextMenuAction::DownloadsOpenFolder => {
                self.handle_dock_hit(DockHit::PinnedDownloads, serial);
            }
            ContextMenuAction::DownloadsOpenEntry(path) => self.launch_path_with_xdg_open(&path),
            ContextMenuAction::DownloadsSetSort(mode) => {
                self.user_config.downloads_sort = mode;
                self.sync_and_save_user_config();
                if self.context_menu.is_some() {
                    let refreshed = resolve_downloads_dir().map_or_default(|dir| {
                        read_downloads_entries(&dir, mode, MAX_DOWNLOADS_MENU_ITEMS)
                    });
                    let entries = Self::build_downloads_entries_from_list(refreshed, mode);
                    if let Some(ref mut ctx) = self.context_menu {
                        ctx.entries = entries;
                        ctx.open_submenu_parent = None;
                        ctx.submenu_selected_row = None;
                        ctx.selected_row = None;
                    }
                }
                self.shell_redraw_needed = true;
            }
            ContextMenuAction::TrashOpen => self.handle_dock_hit(DockHit::PinnedTrash, serial),
            ContextMenuAction::TrashEmpty => self.spawn_empty_trash_worker(),
            ContextMenuAction::OpenSubmenu => return,
        }
        if !keep_open {
            self.close_context_menu();
        }
    }

    fn launch_path_with_xdg_open(&mut self, path: &Path) {
        let Some(path_str) = path.to_str() else {
            return;
        };
        let ipc_path = rusty_ipc::default_ipc_socket_path(&self.socket_name);
        let cmd = format!("xdg-open {}", shell_quote_single(path_str));
        let _ = self
            .launcher
            .launch_command(&cmd, &self.socket_name, &ipc_path);
    }

    fn create_desktop_folder_action(&mut self) {
        let Some(desktop_dir) = resolve_desktop_dir() else {
            self.show_osd("Desktop path unavailable", None, false);
            return;
        };
        match create_unique_desktop_folder(&desktop_dir) {
            Ok(created) => {
                let folder_name = created
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("New Folder");
                self.show_osd(format!("Created: {folder_name}"), None, false);
            }
            Err(err) => {
                warn!(?err, "Failed to create Desktop folder");
                self.show_osd("Folder creation failed", None, false);
            }
        }
    }

    fn toggle_dock_app_autostart(&mut self, hit: DockHit) {
        let Some(candidates) = Self::autostart_candidates_for_hit(hit) else {
            return;
        };
        let had = self
            .user_config
            .autostart
            .iter()
            .any(|cmd| autostart_matches_candidates(cmd, candidates));
        if had {
            self.user_config
                .autostart
                .retain(|cmd| !autostart_matches_candidates(cmd, candidates));
            self.show_osd("Removed Open at Login", None, false);
        } else {
            let cmd = self
                .launcher
                .find_available_command(candidates)
                .unwrap_or_else(|| candidates[0].to_owned());
            self.user_config.autostart.push(cmd);
            self.show_osd("Enabled Open at Login", None, false);
        }
        self.sync_and_save_user_config();
    }

    fn close_window_by_id(&mut self, win_id: WindowId) {
        if let Some(win) = self.smithay_window_for_id(win_id) {
            if let Some(toplevel) = win.toplevel() {
                info!(
                    window_id = win_id.0,
                    "Sending xdg_toplevel::close from context menu"
                );
                toplevel.send_close();
            } else if let Some(x11) = win.x11_surface() {
                info!(
                    window_id = win_id.0,
                    x11_id = x11.window_id(),
                    "Sending WM_DELETE_WINDOW close from context menu"
                );
                let _ = x11.close();
            }
        }
    }

    /// Resolves owning OS PID for `win_id` via Wayland `Credentials` or X11 PID (`RUSTYOS-064`).
    #[must_use]
    pub fn window_client_pid(&self, win_id: WindowId) -> Option<u32> {
        let win = self.smithay_window_for_id(win_id)?;
        if let Some(toplevel) = win.toplevel()
            && let Some(client) = toplevel.wl_surface().client()
            && let Ok(creds) = client.get_credentials(&self.display_handle)
        {
            return u32::try_from(creds.pid).ok().filter(|&pid| pid > 1);
        }
        if let Some(x11) = win.x11_surface() {
            return x11.pid().filter(|&pid| pid > 1);
        }
        None
    }

    fn force_quit_window_by_id(&mut self, win_id: WindowId) {
        let self_pid = std::process::id();
        let Some(pid) = self.window_client_pid(win_id).filter(|&p| p != self_pid) else {
            self.close_window_by_id(win_id);
            return;
        };
        let mut unique = BTreeSet::new();
        unique.insert(pid);
        self.kill_pids_gracefully_forced(&unique);
    }

    fn quit_dock_app(&mut self, hit: DockHit, force: bool) {
        if hit == DockHit::PinnedSettings {
            self.close_settings();
        }
        let matching = self.matching_windows_for_dock_app(hit);
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

    fn kill_pids_gracefully_forced(&mut self, pids: &BTreeSet<u32>) {
        let self_pid = std::process::id();
        let mut killed_any = false;
        for &pid in pids {
            if pid <= 1 || pid == self_pid {
                continue;
            }
            if Command::new("kill")
                .args(["-KILL", &pid.to_string()])
                .spawn()
                .is_ok()
            {
                killed_any = true;
            }
        }
        if killed_any {
            self.show_osd("Force Quit Sent", None, false);
        }
    }

    fn spawn_empty_trash_worker(&mut self) {
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

    /// Dispatches a mouse click at `global_pos` when `self.context_menu` is open (`RUSTYOS-064`).
    ///
    /// Returns `Some(true)` if consumed inside/dismissing by LKM, or `Some(false)` if an outside
    /// right-click closed the menu so the caller can reopen a new menu under the pointer.
    pub fn handle_context_menu_pointer_click(
        &mut self,
        button: u32,
        global_pos: Point<f64, Logical>,
        serial: Serial,
    ) -> Option<bool> {
        let hit = {
            let ctx = self.context_menu.as_ref()?;
            let entry = self.output_map.get(ctx.output_id)?;
            let geo = entry.logical_geometry();
            let screen_size = geo.size.to_physical(1);
            let local_x = global_pos.x - f64::from(geo.loc.x);
            let local_y = global_pos.y - f64::from(geo.loc.y);
            crate::shell_render::hit_test_context_menu(
                screen_size,
                ctx,
                entry.is_primary,
                local_x,
                local_y,
            )
        };
        match hit {
            crate::shell_render::ContextMenuHit::Action(action) => {
                self.execute_context_menu_action(action, serial);
                Some(true)
            }
            crate::shell_render::ContextMenuHit::ExpandSubmenu(parent_idx) => {
                if let Some(ref mut ctx) = self.context_menu {
                    ctx.open_submenu_parent = Some(parent_idx);
                    ctx.selected_row = Some(parent_idx);
                    ctx.submenu_selected_row = None;
                    self.shell_redraw_needed = true;
                }
                Some(true)
            }
            crate::shell_render::ContextMenuHit::Inside => Some(true),
            crate::shell_render::ContextMenuHit::Outside => {
                self.close_context_menu();
                // Allow right-click (`273`) outside to fall through and immediately reopen on the new target.
                Some(button != 273)
            }
        }
    }

    /// Updates hovered row/submenu state for an open [`ContextMenuState`] (`RUSTYOS-064`).
    pub fn update_context_menu_hover_at(&mut self, global_pos: Point<f64, Logical>) {
        let Some(ref mut ctx) = self.context_menu else {
            return;
        };
        let Some(entry) = self.output_map.get(ctx.output_id) else {
            self.context_menu = None;
            return;
        };
        let geo = entry.logical_geometry();
        let screen_size = geo.size.to_physical(1);
        let local_x = global_pos.x - f64::from(geo.loc.x);
        let local_y = global_pos.y - f64::from(geo.loc.y);
        if crate::shell_render::update_context_menu_hover(
            screen_size,
            ctx,
            entry.is_primary,
            local_x,
            local_y,
        ) {
            self.shell_redraw_needed = true;
        }
    }

    /// Rebuilds the active context menu if `Keysym` executed an action, or updates hover state.
    pub fn handle_context_menu_key_event(
        &mut self,
        sym: smithay::input::keyboard::Keysym,
        serial: Serial,
    ) {
        let Some(ref mut ctx) = self.context_menu else {
            return;
        };
        match crate::shell_render::handle_context_menu_key(ctx, sym) {
            crate::shell_render::ContextMenuKeyOutcome::None => {}
            crate::shell_render::ContextMenuKeyOutcome::Redraw => {
                self.shell_redraw_needed = true;
            }
            crate::shell_render::ContextMenuKeyOutcome::Close => self.close_context_menu(),
            crate::shell_render::ContextMenuKeyOutcome::Execute(action) => {
                self.execute_context_menu_action(action, serial);
            }
        }
    }

    /// Opens the Dock `Downloads` menu directly in tests without depending on real pointer coordinates.
    #[cfg(test)]
    pub(crate) fn test_refresh_downloads_sort_and_entries(
        &mut self,
        downloads_dir: &Path,
        mode: DownloadsSortMode,
    ) -> Vec<ContextMenuEntry> {
        self.user_config.downloads_sort = mode;
        let files = read_downloads_entries(downloads_dir, mode, MAX_DOWNLOADS_MENU_ITEMS);
        Self::build_downloads_entries_from_list(files, mode)
    }
}
