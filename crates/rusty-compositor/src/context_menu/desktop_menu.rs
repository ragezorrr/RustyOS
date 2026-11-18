//! Desktop right-click context menu builders and desktop canvas operations (`RUSTYOS-064`, `RUSTYOS-081`, `RUSTYOS-086`).

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use tracing::warn;

use crate::shell_render::{ContextMenuAction, ContextMenuEntry, DesktopCanvasItem};
use crate::surfaces::UiGlyphKind;

/// Resolves standard XDG Desktop directory (`~/Desktop` or localized) (`RUSTYOS-064`).
#[allow(clippy::unnecessary_wraps)]
#[must_use]
pub fn resolve_desktop_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_DESKTOP_DIR")
        && !dir.is_empty()
    {
        return Some(PathBuf::from(dir));
    }
    let detected = rusty_services::file_manager::UserFileLocations::detect().desktop;
    if detected.is_dir() {
        return Some(detected);
    }
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        let localized_ru = home.join("Рабочий стол");
        if localized_ru.is_dir() {
            return Some(localized_ru);
        }
        let en = home.join("Desktop");
        return Some(en);
    }
    Some(detected)
}

/// Creates a uniquely named `New Folder` (or `Новая папка`) inside `desktop_dir` (`RUSTYOS-064`).
///
/// # Errors
///
/// Returns an [`std::io::Error`] if the directory cannot be created or candidate names are exhausted.
pub fn create_unique_desktop_folder(desktop_dir: &Path) -> std::io::Result<PathBuf> {
    fs::create_dir_all(desktop_dir)?;
    let is_ru = desktop_dir.to_string_lossy().contains("Рабочий стол");
    let base_name = if is_ru {
        "Новая папка"
    } else {
        "New Folder"
    };
    for idx in 1..=999_u32 {
        let name = if idx == 1 {
            base_name.to_owned()
        } else {
            format!("{base_name} {idx}")
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

/// Scans desktop canvas items with saved grid slot persistence (`RUSTYOS-081`).
#[must_use]
pub fn scan_desktop_canvas_items(desktop_dir: &Path) -> Vec<DesktopCanvasItem> {
    let mut items = Vec::new();
    let Ok(read_dir) = fs::read_dir(desktop_dir) else {
        return items;
    };
    for entry in read_dir.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let is_dir = entry.file_type().is_ok_and(|ft| ft.is_dir());
        let mut display_name = name.clone();
        let mut icon_name = None;

        if !is_dir
            && name.ends_with(".desktop")
            && let Ok(content) = fs::read_to_string(&path)
            && let Some(app) = crate::launcher::parse_desktop_entry(&content, &name)
        {
            display_name = app.name;
            icon_name = app.icon;
        }

        items.push(DesktopCanvasItem {
            name: display_name,
            path,
            is_dir,
            icon_name,
            slot: (usize::MAX, usize::MAX),
        });
    }

    items.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });

    let saved_slots = crate::config::default_user_config_path()
        .and_then(|cfg| cfg.parent().map(|dir| dir.join("desktop_grid.toml")))
        .map(|path| crate::shell_render::load_desktop_grid_positions(&path))
        .unwrap_or_default();

    let mut occupied: HashSet<(usize, usize)> = HashSet::new();

    // 1. Assign saved positions first
    for item in &mut items {
        let key = item
            .path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default();
        if let Some(&slot) = saved_slots.get(&key)
            && !occupied.contains(&slot)
        {
            item.slot = slot;
            occupied.insert(slot);
        }
    }

    // 2. Allocate free grid cells for any new items flowing down columns
    let mut next_col = 0;
    let mut next_row = 0;
    for item in &mut items {
        if item.slot == (usize::MAX, usize::MAX) {
            while occupied.contains(&(next_col, next_row)) {
                next_row += 1;
                if next_row >= 16 {
                    next_row = 0;
                    next_col += 1;
                }
            }
            item.slot = (next_col, next_row);
            occupied.insert((next_col, next_row));
        }
    }

    items
}

/// Builds root Desktop context menu entries (`RUSTYOS-064`, `RUSTYOS-107`).
#[must_use]
pub fn build_desktop_context_entries(tiling_active: bool) -> Vec<ContextMenuEntry> {
    vec![
        ContextMenuEntry::item(
            "New Folder",
            UiGlyphKind::Plus,
            ContextMenuAction::DesktopNewFolder,
        )
        .with_hint("⇧⌘N"),
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
        .with_hint("⌥⏎"),
        ContextMenuEntry::item(
            "Mission Control",
            UiGlyphKind::Grid,
            ContextMenuAction::DesktopMissionControl,
        )
        .with_hint("⌃▲"),
        ContextMenuEntry::item(
            "Tile Windows",
            UiGlyphKind::LayoutSplit,
            ContextMenuAction::DesktopToggleLayoutMode,
        )
        .with_hint("⌥L")
        .with_checked(tiling_active),
    ]
}

/// Builds context menu entries for an individual desktop canvas file/directory item (`RUSTYOS-081`).
#[must_use]
pub fn build_desktop_item_context_entries(item: &DesktopCanvasItem) -> Vec<ContextMenuEntry> {
    vec![
        ContextMenuEntry::item(
            "Open",
            UiGlyphKind::Folder,
            ContextMenuAction::DesktopItemOpen(item.path.clone()),
        ),
        ContextMenuEntry::Separator,
        ContextMenuEntry::item(
            "Move to Trash",
            UiGlyphKind::Close,
            ContextMenuAction::DesktopItemTrash(item.path.clone()),
        ),
    ]
}

impl crate::state::RustyCompositorState {
    /// Commits the active desktop in-place rename if the new name is valid (`RUSTYOS-083`).
    pub fn commit_desktop_inline_rename(&mut self) {
        let Some(rename) = self.desktop_inline_rename.take() else {
            return;
        };
        let new_name = rename.buffer.trim();
        if new_name.is_empty() || new_name.contains('/') || new_name.contains('\0') {
            self.show_osd("Invalid file name", None, false);
            self.shell_redraw_needed = true;
            return;
        }
        let Some(parent) = rename.target_path.parent() else {
            return;
        };
        let new_path = parent.join(new_name);
        if new_path == rename.target_path {
            self.shell_redraw_needed = true;
            return;
        }
        if new_path.exists() {
            self.show_osd("File already exists", None, false);
            self.shell_redraw_needed = true;
            return;
        }
        if let Err(err) = fs::rename(&rename.target_path, &new_path) {
            warn!(?err, "Failed to rename desktop file");
            self.show_osd("Rename failed", None, false);
        } else {
            self.selected_desktop_items.remove(&rename.target_path);
            self.selected_desktop_items.insert(new_path);
            self.resync_desktop_items();
            self.save_desktop_grid_layout();
        }
        self.shell_redraw_needed = true;
    }

    /// Moves all currently selected desktop items to Trash (`RUSTYOS-083`).
    pub fn move_selected_desktop_items_to_trash(&mut self) {
        let Some(trash_root) = crate::context_menu::dock_menu::resolve_trash_root() else {
            self.show_osd("Trash unavailable", None, false);
            return;
        };
        let mut count = 0;
        let paths: Vec<PathBuf> = self.selected_desktop_items.iter().cloned().collect();
        for path in paths {
            if rusty_services::file_manager::move_to_trash(&path, &trash_root).is_ok() {
                count += 1;
                self.selected_desktop_items.remove(&path);
            }
        }
        if count > 0 {
            self.show_osd(format!("Moved to Trash: {count}"), None, false);
            self.resync_desktop_items();
            self.save_desktop_grid_layout();
            self.update_trash_full_status();
        }
        self.shell_redraw_needed = true;
    }
}
