//! Context menu action execution and window/process dispatching (`RUSTYOS-064`, `RUSTYOS-086`).

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;

use rusty_wm::{WindowId, WmAction, WorkspaceId};
use smithay::reexports::wayland_server::Resource;
use smithay::utils::Serial;
use tracing::{info, warn};

use crate::shell_render::{ContextMenuAction, ContextMenuEntry, DockHit};

/// Dynamically swaps `Quit` <-> `Force Quit` when `Alt` is pressed or released (`RUSTYOS-064`).
pub fn toggle_quit_force_in_entries(entries: &mut [ContextMenuEntry], alt_held: bool) -> bool {
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
                "Close".clone_into(label);
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

impl crate::state::RustyCompositorState {
    /// Executes a selected context menu action (`RUSTYOS-064`, `RUSTYOS-086`).
    pub fn execute_context_menu_action(&mut self, action: ContextMenuAction, serial: Serial) {
        let keep_open = matches!(
            action,
            ContextMenuAction::DownloadsSetSort(_)
                | ContextMenuAction::DownloadsSetDisplay(_)
                | ContextMenuAction::DownloadsSetView(_)
        );
        match action {
            ContextMenuAction::DesktopNewFolder => self.create_desktop_folder_action(),
            ContextMenuAction::DesktopOpenFiles => {
                let desktop = crate::context_menu::desktop_menu::resolve_desktop_dir();
                if !self.launch_rusty_files_dir(desktop.as_deref()) {
                    self.show_osd("rusty-files unavailable", None, false);
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
                self.launch_dock_app_new_instance("terminal", false);
            }
            ContextMenuAction::DesktopMissionControl | ContextMenuAction::DockShowAllWindows => {
                self.overview_active = false;
                self.overview_filter_app = None;
                self.toggle_overview();
            }
            ContextMenuAction::DesktopToggleLayoutMode => {
                self.apply_wm_action(WmAction::ToggleLayoutMode, serial);
            }
            ContextMenuAction::DesktopItemOpen(path) => {
                let is_dir = path.is_dir();
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_owned();
                let item = crate::shell_render::DesktopCanvasItem {
                    name,
                    path: path.clone(),
                    is_dir,
                    icon_name: None,
                    slot: (0, 0),
                };
                self.open_desktop_canvas_item(&item);
            }
            ContextMenuAction::DesktopItemTrash(path) => {
                if let Some(trash_dir) = crate::context_menu::dock_menu::resolve_trash_root() {
                    let _ = rusty_services::file_manager::move_to_trash(&path, &trash_dir);
                    self.resync_desktop_items();
                    self.show_osd("Moved to Trash", None, false);
                }
            }
            ContextMenuAction::OverviewToggle => self.toggle_overview(),
            ContextMenuAction::SwitchWorkspace(ws) => {
                self.apply_wm_action(
                    WmAction::SwitchWorkspace(WorkspaceId::new_clamped(ws)),
                    serial,
                );
            }
            ContextMenuAction::DockFocusWindow(win_id)
            | ContextMenuAction::WindowFocusOrRestore(win_id) => {
                if self.is_window_minimized(win_id) {
                    self.restore_window(win_id, serial);
                } else {
                    self.wm.focus_and_raise(win_id);
                    self.sync_space_with_wm(serial);
                }
            }
            ContextMenuAction::OpenSubmenu => return,
            other => self.execute_dock_or_window_context_action(other, serial),
        }
        if !keep_open {
            self.close_context_menu();
        }
    }

    pub(crate) fn execute_dock_or_window_context_action(
        &mut self,
        action: ContextMenuAction,
        serial: Serial,
    ) {
        match action {
            ContextMenuAction::DockLaunchApp(hit) => {
                if hit == DockHit::PinnedSettings {
                    self.open_settings();
                } else if let Some(app_key) = self.dock_key_for_hit(hit) {
                    self.launch_dock_app_new_instance(&app_key, false);
                }
            }
            ContextMenuAction::DockToggleKeepInDock(hit) => {
                if let Some(app_key) = self.dock_key_for_hit(hit) {
                    self.toggle_app_keep_in_dock(&app_key);
                }
            }
            ContextMenuAction::DockShowAppWindows(hit) => {
                if let Some(app_key) = self.dock_key_for_hit(hit) {
                    self.toggle_overview_for_app(&app_key);
                }
            }
            ContextMenuAction::DockToggleOpenAtLogin(hit) => self.toggle_dock_app_autostart(hit),
            ContextMenuAction::DockQuitApp(hit) => self.quit_dock_app(hit, false),
            ContextMenuAction::DockForceQuitApp(hit) => self.quit_dock_app(hit, true),
            ContextMenuAction::WindowMinimize(win_id) => self.minimize_window(win_id, serial),
            ContextMenuAction::WindowTileLeft(win_id) => {
                if self.is_window_minimized(win_id) {
                    self.restore_window(win_id, serial);
                }
                self.wm.focus_and_raise(win_id);
                self.apply_wm_action(
                    WmAction::SnapFocused(rusty_wm::SnapTarget::LeftHalf),
                    serial,
                );
            }
            ContextMenuAction::WindowTileRight(win_id) => {
                if self.is_window_minimized(win_id) {
                    self.restore_window(win_id, serial);
                }
                self.wm.focus_and_raise(win_id);
                self.apply_wm_action(
                    WmAction::SnapFocused(rusty_wm::SnapTarget::RightHalf),
                    serial,
                );
            }
            ContextMenuAction::WindowToggleMaximize(win_id) => {
                if self.is_window_minimized(win_id) {
                    self.restore_window(win_id, serial);
                }
                self.wm.focus_and_raise(win_id);
                self.apply_wm_action(WmAction::ToggleMaximizeFocused, serial);
            }
            ContextMenuAction::WindowMoveToNextOutput(win_id) => {
                if self.is_window_minimized(win_id) {
                    self.restore_window(win_id, serial);
                }
                self.wm.focus_and_raise(win_id);
                self.apply_wm_action(WmAction::MoveFocusedToNextOutput, serial);
            }
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
                let downloads = crate::context_menu::dock_menu::resolve_downloads_dir();
                if !self.launch_rusty_files_dir(downloads.as_deref()) {
                    self.toggle_launcher();
                }
            }
            ContextMenuAction::DownloadsOpenEntry(path) => self.open_downloads_entry(&path),
            ContextMenuAction::DownloadsSetSort(mode) => {
                self.user_config.downloads_sort = mode;
                self.sync_and_save_user_config();
                self.refresh_downloads_context_menu();
            }
            ContextMenuAction::DownloadsSetDisplay(mode) => {
                self.user_config.dock_downloads_display = mode;
                self.sync_and_save_user_config();
                self.refresh_downloads_context_menu();
            }
            ContextMenuAction::DownloadsSetView(mode) => {
                self.user_config.dock_downloads_view = mode;
                self.sync_and_save_user_config();
                self.refresh_downloads_context_menu();
            }
            ContextMenuAction::TrashOpen => self.handle_dock_hit(DockHit::PinnedTrash, serial),
            ContextMenuAction::TrashEmpty => self.spawn_empty_trash_worker(),
            _ => {}
        }
    }

    pub(crate) fn open_downloads_entry(&mut self, path: &Path) {
        if path.is_dir() {
            let _ = self.launch_rusty_files_dir(Some(path));
        } else {
            self.launch_path_with_xdg_open(path);
        }
    }

    pub(crate) fn launch_path_with_xdg_open(&mut self, path: &Path) {
        let Some(path_str) = path.to_str() else {
            return;
        };
        let ipc_path = rusty_ipc::default_ipc_socket_path(&self.socket_name);
        let cmd = format!(
            "xdg-open {}",
            crate::context_menu::dock_menu::shell_quote_single(path_str)
        );
        let _ = self
            .launcher
            .launch_command(&cmd, &self.socket_name, &ipc_path);
    }

    pub(crate) fn open_desktop_canvas_item(
        &mut self,
        item: &crate::shell_render::DesktopCanvasItem,
    ) {
        if item.is_dir {
            let _ = self.launch_rusty_files_dir(Some(&item.path));
        } else if item.path.extension().is_some_and(|e| e == "desktop")
            && let Ok(content) = fs::read_to_string(&item.path)
            && let Some(app) = crate::launcher::parse_desktop_entry(&content, &item.name)
        {
            let ipc_path = rusty_ipc::default_ipc_socket_path(&self.socket_name);
            let cmd = crate::launcher::clean_desktop_exec(&app.exec);
            let _ = self
                .launcher
                .launch_command(&cmd, &self.socket_name, &ipc_path);
        } else {
            self.launch_path_with_xdg_open(&item.path);
        }
    }

    pub(crate) fn create_desktop_folder_action(&mut self) {
        let Some(desktop_dir) = crate::context_menu::desktop_menu::resolve_desktop_dir() else {
            self.show_osd("Desktop path unavailable", None, false);
            return;
        };
        match crate::context_menu::desktop_menu::create_unique_desktop_folder(&desktop_dir) {
            Ok(created) => {
                let folder_name = created
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("New Folder");
                self.show_osd(format!("Created: {folder_name}"), None, false);
                self.resync_desktop_items();
                self.selected_desktop_items.clear();
                self.selected_desktop_items.insert(created);
                self.save_desktop_grid_layout();
            }
            Err(err) => {
                warn!(?err, "Failed to create Desktop folder");
                self.show_osd("Folder creation failed", None, false);
            }
        }
    }

    pub(crate) fn close_window_by_id(&mut self, win_id: WindowId) {
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

    pub(crate) fn force_quit_window_by_id(&mut self, win_id: WindowId) {
        let self_pid = std::process::id();
        let Some(pid) = self.window_client_pid(win_id).filter(|&p| p != self_pid) else {
            self.close_window_by_id(win_id);
            return;
        };
        let mut unique = BTreeSet::new();
        unique.insert(pid);
        self.kill_pids_gracefully_forced(&unique);
    }

    pub(crate) fn kill_pids_gracefully_forced(&mut self, pids: &BTreeSet<u32>) {
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
}
