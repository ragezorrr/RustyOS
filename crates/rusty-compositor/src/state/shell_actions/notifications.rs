//! Notification center, D-Bus notifications, tray updates, OSD banners, and screenshot HUD actions.

use std::time::Duration;

use rusty_ipc::Urgency;
use tracing::warn;

use super::super::{CompositorNotification, OsdBanner, RustyCompositorState};

impl RustyCompositorState {
    /// Pushes a notification into the compositor HUD queue using current monotonic uptime (`RUSTYOS-024`).
    pub fn push_notification(&mut self, urgency: Urgency, title: String, body: String) -> u64 {
        let now = self.start_time.elapsed();
        self.push_notification_at(urgency, title, body, now)
    }

    /// Pushes a notification at explicit monotonic timestamp `now` (`RUSTYOS-024`).
    pub fn push_notification_at(
        &mut self,
        urgency: Urgency,
        title: String,
        body: String,
        now: Duration,
    ) -> u64 {
        const MAX_QUEUE_LEN: usize = 16;
        const MAX_HISTORY_LEN: usize = 32;
        self.next_notification_id = self.next_notification_id.saturating_add(1);
        let ttl = match urgency {
            Urgency::Low => Duration::from_secs(4),
            Urgency::Normal => Duration::from_secs(6),
            Urgency::Critical => Duration::from_secs(12),
        };
        let id = self.next_notification_id;
        let item = CompositorNotification {
            id,
            dbus_id: None,
            app_key: None,
            app_display_name: None,
            urgency,
            title,
            body,
            avatar: None,
            created_at: now,
            ttl,
        };
        self.notifications.push(item.clone());
        self.notification_history.push(item);
        if self.notifications.len() > MAX_QUEUE_LEN {
            let excess = self.notifications.len() - MAX_QUEUE_LEN;
            self.notifications.drain(0..excess);
        }
        if self.notification_history.len() > MAX_HISTORY_LEN {
            let excess = self.notification_history.len() - MAX_HISTORY_LEN;
            self.notification_history.drain(0..excess);
        }
        self.sync_notification_count();
        id
    }

    /// Updates the active `XWayland` `DISPLAY` number (`:N`) on state and launcher (`RUSTYOS-026`).
    pub fn set_xwayland_display(&mut self, display_number: Option<u32>) {
        self.xwayland_display = display_number;
        self.launcher.x11_display = display_number.map(|num| format!(":{num}"));
        if self.sync_systemd_activation_env && display_number.is_some() {
            crate::drm_backend::sync_drm_activation_environment(&self.socket_name, display_number);
        }
    }

    /// Inserts or replaces a notification received from the D-Bus session server (`RUSTYOS-026`).
    pub fn push_dbus_notification(
        &mut self,
        dbus_id: u32,
        replaces_id: u32,
        urgency: Urgency,
        title: String,
        body: String,
        expire_after: Option<Duration>,
    ) {
        self.push_dbus_notification_for_app(
            dbus_id,
            replaces_id,
            urgency,
            title,
            body,
            expire_after,
            None,
            None,
            None,
        );
    }

    /// Inserts or replaces a D-Bus notification optionally tagged with a canonical Dock `app_key` (`RUSTYOS-162`).
    #[allow(clippy::too_many_arguments)]
    pub fn push_dbus_notification_for_app(
        &mut self,
        dbus_id: u32,
        replaces_id: u32,
        urgency: Urgency,
        title: String,
        body: String,
        expire_after: Option<Duration>,
        app_key: Option<String>,
        app_display_name: Option<String>,
        avatar: Option<crate::state::NotificationAvatar>,
    ) {
        const MAX_QUEUE_LEN: usize = 12;
        const MAX_HISTORY_LEN: usize = 32;
        let now = self.start_time.elapsed();
        let default_ttl = match urgency {
            Urgency::Low => Duration::from_secs(4),
            Urgency::Normal => Duration::from_secs(6),
            Urgency::Critical => Duration::from_secs(12),
        };
        let ttl = expire_after.unwrap_or(default_ttl);

        if replaces_id > 0
            && let Some(existing) = self
                .notifications
                .iter_mut()
                .find(|n| n.dbus_id == Some(replaces_id))
        {
            existing.dbus_id = Some(dbus_id);
            if app_key.is_some() {
                existing.app_key.clone_from(&app_key);
            }
            if app_display_name.is_some() {
                existing.app_display_name.clone_from(&app_display_name);
            }
            existing.urgency = urgency;
            existing.title.clone_from(&title);
            existing.body.clone_from(&body);
            if avatar.is_some() {
                existing.avatar.clone_from(&avatar);
            }
            existing.created_at = now;
            existing.ttl = ttl;
            if let Some(hist) = self
                .notification_history
                .iter_mut()
                .find(|n| n.dbus_id == Some(replaces_id))
            {
                hist.dbus_id = Some(dbus_id);
                if app_key.is_some() {
                    hist.app_key = app_key;
                }
                if app_display_name.is_some() {
                    hist.app_display_name = app_display_name;
                }
                hist.urgency = urgency;
                hist.title = title;
                hist.body = body;
                if avatar.is_some() {
                    hist.avatar = avatar;
                }
                hist.created_at = now;
                hist.ttl = ttl;
            }
            self.sync_notification_count();
            return;
        }

        let suppress_toast = !self.user_config.notifications_allow
            || app_key
                .as_deref()
                .is_some_and(|k| self.user_config.is_notification_app_muted(k));
        if !suppress_toast && let Some(ref key) = app_key {
            self.increment_notification_badge(key);
        }

        self.next_notification_id = self.next_notification_id.saturating_add(1);
        let id = self.next_notification_id;
        let item = CompositorNotification {
            id,
            dbus_id: Some(dbus_id),
            app_key,
            app_display_name,
            urgency,
            title,
            body,
            avatar,
            created_at: now,
            ttl,
        };
        if !suppress_toast {
            self.notifications.push(item.clone());
        }
        self.notification_history.push(item);
        if self.notifications.len() > MAX_QUEUE_LEN {
            let excess = self.notifications.len() - MAX_QUEUE_LEN;
            self.notifications.drain(0..excess);
        }
        if self.notification_history.len() > MAX_HISTORY_LEN {
            let excess = self.notification_history.len() - MAX_HISTORY_LEN;
            self.notification_history.drain(0..excess);
        }
        self.sync_notification_count();
    }

    /// Non-blockingly drains queued notifications and `Unity.LauncherEntry` badge updates from D-Bus (`RUSTYOS-026` / `RUSTYOS-162`).
    pub fn drain_dbus_notifications(&mut self) {
        for cmd in self.dbus_notifications.drain_commands() {
            match cmd {
                crate::dbus_notifications::DbusNotificationCommand::Notify {
                    id,
                    replaces_id,
                    urgency,
                    title,
                    body,
                    expire_after,
                    app_key,
                    app_display_name,
                    avatar,
                } => {
                    self.push_dbus_notification_for_app(
                        id,
                        replaces_id,
                        urgency,
                        title,
                        body,
                        expire_after,
                        app_key,
                        app_display_name,
                        avatar,
                    );
                    self.shell_redraw_needed = true;
                }
                crate::dbus_notifications::DbusNotificationCommand::Close { id } => {
                    let closed_app = self
                        .notifications
                        .iter()
                        .chain(self.notification_history.iter())
                        .find(|n| n.dbus_id == Some(id))
                        .and_then(|n| n.app_key.clone());
                    self.notifications.retain(|n| n.dbus_id != Some(id));
                    self.notification_history.retain(|n| n.dbus_id != Some(id));
                    if let Some(key) = closed_app {
                        self.decrement_notification_badge(&key);
                    }
                    self.sync_notification_count();
                    self.shell_redraw_needed = true;
                }
                crate::dbus_notifications::DbusNotificationCommand::LauncherBadgeUpdate {
                    app_key,
                    count,
                    visible,
                } => {
                    self.set_unity_launcher_badge(&app_key, count, visible);
                }
            }
        }
    }

    /// Non-blockingly drains system tray updates from the background SNI watcher (`RUSTYOS-032`).
    pub fn drain_tray_updates(&mut self) {
        if let Some(items) = self.tray.drain_latest() {
            self.tray_items = items;
        }
    }

    /// Removes notifications whose TTL has elapsed as of current uptime (`RUSTYOS-024`).
    pub fn expire_notifications(&mut self) -> bool {
        let now = self.start_time.elapsed();
        self.expire_notifications_at(now)
    }

    /// Removes notifications and transient OSD banners whose TTL has elapsed at `now` (`RUSTYOS-024`/`031`).
    pub fn expire_notifications_at(&mut self, now: Duration) -> bool {
        let mut expired_dbus_ids = Vec::new();
        let prev_len = self.notifications.len();
        self.notifications.retain(|n| {
            let keep = now.saturating_sub(n.created_at) < n.ttl;
            if !keep && let Some(dbus_id) = n.dbus_id {
                expired_dbus_ids.push(dbus_id);
            }
            keep
        });
        for dbus_id in expired_dbus_ids {
            self.dbus_notifications.emit_closed(dbus_id, 1);
        }
        self.sync_notification_count();
        let mut changed = self.notifications.len() != prev_len;
        if self.osd.as_ref().is_some_and(|osd| now >= osd.expires_at) {
            self.osd = None;
            changed = true;
        }
        changed
    }

    /// Displays a transient glass OSD indicator pill above the Dock (`RUSTYOS-031`).
    pub fn show_osd(&mut self, label: impl Into<String>, value_percent: Option<u8>, muted: bool) {
        let expires_at = self.start_time.elapsed() + Duration::from_millis(1600);
        self.osd = Some(OsdBanner {
            label: label.into(),
            value_percent,
            muted,
            expires_at,
        });
    }

    /// Polls background screenshot encoding/saving results without blocking (`RUSTYOS-031`).
    pub fn drain_screenshot_results(&mut self) {
        while let Some(res) = self.screenshot_results.try_pop() {
            match res {
                Ok(path) => {
                    let filename = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("screenshot.png")
                        .to_owned();
                    let _ = self.push_notification(
                        Urgency::Low,
                        "Screenshot Saved".to_owned(),
                        format!("{filename} (copied to clipboard)"),
                    );
                    self.show_osd("Screenshot Saved", None, false);
                }
                Err(err) => {
                    warn!(error = %err, "Failed to save screenshot");
                    let _ = self.push_notification(
                        Urgency::Critical,
                        "Screenshot Failed".to_owned(),
                        err,
                    );
                    self.show_osd("Screenshot Failed", None, false);
                }
            }
        }
    }

    /// Activates the default D-Bus action (`"default"`) on a clicked notification toast and dismisses it.
    pub fn activate_notification(&mut self, id: u64) -> bool {
        let dbus_id = self
            .notifications
            .iter()
            .chain(self.notification_history.iter())
            .find(|n| n.id == id)
            .and_then(|n| n.dbus_id);
        if let Some(did) = dbus_id {
            self.dbus_notifications.emit_action_invoked(did, "default");
        }
        self.dismiss_notification(id)
    }

    /// Dismisses a single active notification by ID (`RUSTYOS-024`, `RUSTYOS-109`).
    pub fn dismiss_notification(&mut self, id: u64) -> bool {
        let found = self
            .notifications
            .iter()
            .chain(self.notification_history.iter())
            .find(|n| n.id == id)
            .map(|n| (n.dbus_id, n.app_key.clone()));
        let dbus_id = found.as_ref().and_then(|(did, _)| *did);
        let app_key = found.and_then(|(_, key)| key);
        let prev_len = self.notifications.len();
        let prev_hist = self.notification_history.len();
        self.notifications.retain(|n| n.id != id);
        self.notification_history.retain(|n| n.id != id);
        if let Some(did) = dbus_id {
            self.dbus_notifications.emit_closed(did, 2);
        }
        if let Some(key) = app_key {
            self.decrement_notification_badge(&key);
        }
        self.sync_notification_count();
        self.notifications.len() < prev_len || self.notification_history.len() < prev_hist
    }

    /// Clears all active notifications from the HUD queue and Notification Center (`RUSTYOS-024`, `RUSTYOS-109`).
    pub fn clear_notifications(&mut self) {
        for n in &self.notifications {
            if let Some(did) = n.dbus_id {
                self.dbus_notifications.emit_closed(did, 2);
            }
        }
        self.notifications.clear();
        self.notification_history.clear();
        self.dock_unread_notif_badges.clear();
        self.sync_notification_count();
    }

    /// Pushes a live workspace/telemetry status notification (`Alt+N`, `RUSTYOS-024`).
    pub fn trigger_demo_notification(&mut self) -> u64 {
        let next_idx = self.next_notification_id % 3;
        let urgency = match next_idx {
            0 => Urgency::Normal,
            1 => Urgency::Low,
            _ => Urgency::Critical,
        };
        let ws = self.wm.active_workspace_id().0;
        let win_count = self.wm.visible_windows().len();
        let tel = self.telemetry.snapshot();
        let (title, body) = match urgency {
            Urgency::Normal => (
                format!("Workspace {ws} Status"),
                format!("{win_count} windows • {}", tel.network.status_label),
            ),
            Urgency::Low => (
                "System Telemetry".to_owned(),
                format!(
                    "Vol {}%{} • {}",
                    tel.audio.volume_percent,
                    if tel.audio.muted { " (Muted)" } else { "" },
                    tel.clock_hhmm
                ),
            ),
            Urgency::Critical => (
                "Attention Required".to_owned(),
                "Click toast or Alt+Shift+N to dismiss".to_owned(),
            ),
        };
        self.push_notification(urgency, title, body)
    }

    pub(in crate::state) fn sync_notification_count(&mut self) {
        self.notification_count = u32::try_from(self.notifications.len()).unwrap_or(u32::MAX);
    }

    /// Opens the interactive `ScreenshotHud` overlay (`RUSTYOS-133`).
    pub fn open_screenshot_hud(&mut self) {
        self.close_overview();
        self.close_dock_stack();
        self.close_context_menu();
        self.close_launcher();
        self.close_quick_settings();
        self.close_updates_popover();
        self.close_wifi_popover();
        self.close_notification_center();
        self.close_top_bar_menu();
        self.screenshot_hud = Some(crate::shell_render::ScreenshotHudState::default());
        self.shell_redraw_needed = true;
    }

    /// Closes the interactive `ScreenshotHud` overlay (`RUSTYOS-133`).
    pub fn close_screenshot_hud(&mut self) {
        if self.screenshot_hud.take().is_some() {
            self.shell_redraw_needed = true;
        }
    }

    /// Executes the active `ScreenshotHud` capture and closes the HUD so it is excluded from the PNG (`RUSTYOS-133`).
    pub fn trigger_screenshot_hud_capture(&mut self) {
        let Some(hud) = self.screenshot_hud.take() else {
            return;
        };
        let req = match hud.mode {
            crate::shell_render::ScreenshotMode::FullScreen => {
                if hud.copy_only {
                    crate::screenshot::ScreenshotRequest::FullScreenCopyOnly
                } else {
                    crate::screenshot::ScreenshotRequest::FullScreen
                }
            }
            crate::shell_render::ScreenshotMode::Window => {
                if let Some((_, rect)) = hud.hovered_window {
                    crate::screenshot::ScreenshotRequest::Region {
                        rect,
                        copy_only: hud.copy_only,
                    }
                } else {
                    crate::screenshot::ScreenshotRequest::FocusedWindow
                }
            }
            crate::shell_render::ScreenshotMode::Region => {
                if let Some(rect) = hud.active_region_rect() {
                    crate::screenshot::ScreenshotRequest::Region {
                        rect,
                        copy_only: hud.copy_only,
                    }
                } else if hud.copy_only {
                    crate::screenshot::ScreenshotRequest::FullScreenCopyOnly
                } else {
                    crate::screenshot::ScreenshotRequest::FullScreen
                }
            }
        };
        self.pending_screenshot = Some(req);
        self.shell_redraw_needed = true;
    }

    /// Updates hover/drag state while `ScreenshotHud` is active (`RUSTYOS-133`).
    #[allow(clippy::cast_possible_truncation)]
    pub fn handle_screenshot_hud_motion(
        &mut self,
        loc: smithay::utils::Point<f64, smithay::utils::Logical>,
    ) {
        let Some(hud) = self.screenshot_hud.as_mut() else {
            return;
        };
        let output_phys = self
            .space
            .output_geometry(&self.output)
            .map_or_else(|| smithay::utils::Size::from((1280, 800)), |geo| geo.size)
            .to_physical(1);
        let px = loc.x.round() as i32;
        let py = loc.y.round() as i32;
        let next_hover = crate::shell_render::hit_test_screenshot_hud(
            output_phys,
            smithay::utils::Point::from((px, py)),
        );
        if hud.hover_hit != next_hover {
            hud.hover_hit = next_hover;
            self.shell_redraw_needed = true;
        }
        if hud.mode == crate::shell_render::ScreenshotMode::Region && hud.drag_start.is_some() {
            hud.drag_current = Some((px, py));
            self.shell_redraw_needed = true;
        } else if hud.mode == crate::shell_render::ScreenshotMode::Window {
            let hovered = self
                .wm
                .visible_windows()
                .into_iter()
                .rev()
                .find(|w| {
                    !self.is_window_minimized(w.id)
                        && px >= w.geometry.x
                        && px < w.geometry.x + w.geometry.width
                        && py >= w.geometry.y
                        && py < w.geometry.y + w.geometry.height
                })
                .map(|w| (w.id, w.geometry));
            if let Some(hud) = self.screenshot_hud.as_mut()
                && hud.hovered_window != hovered
            {
                hud.hovered_window = hovered;
                self.shell_redraw_needed = true;
            }
        }
    }

    /// Handles pointer press/release while `ScreenshotHud` is active (`RUSTYOS-133`).
    #[allow(clippy::cast_possible_truncation)]
    pub fn handle_screenshot_hud_button(&mut self, pressed: bool) {
        let output_phys = self
            .space
            .output_geometry(&self.output)
            .map_or_else(|| smithay::utils::Size::from((1280, 800)), |geo| geo.size)
            .to_physical(1);
        let px = self.pointer_location.x.round() as i32;
        let py = self.pointer_location.y.round() as i32;
        if pressed {
            if let Some(hit) = crate::shell_render::hit_test_screenshot_hud(
                output_phys,
                smithay::utils::Point::from((px, py)),
            ) {
                match hit {
                    crate::shell_render::ScreenshotHudHit::ModeRegion => {
                        if let Some(hud) = self.screenshot_hud.as_mut() {
                            hud.mode = crate::shell_render::ScreenshotMode::Region;
                            self.shell_redraw_needed = true;
                        }
                    }
                    crate::shell_render::ScreenshotHudHit::ModeWindow => {
                        if let Some(hud) = self.screenshot_hud.as_mut() {
                            hud.mode = crate::shell_render::ScreenshotMode::Window;
                            self.shell_redraw_needed = true;
                        }
                    }
                    crate::shell_render::ScreenshotHudHit::ModeFullScreen => {
                        if let Some(hud) = self.screenshot_hud.as_mut() {
                            hud.mode = crate::shell_render::ScreenshotMode::FullScreen;
                            self.shell_redraw_needed = true;
                        }
                    }
                    crate::shell_render::ScreenshotHudHit::ToggleCopyOnly => {
                        if let Some(hud) = self.screenshot_hud.as_mut() {
                            hud.copy_only = !hud.copy_only;
                            self.shell_redraw_needed = true;
                        }
                    }
                    crate::shell_render::ScreenshotHudHit::Capture => {
                        self.trigger_screenshot_hud_capture();
                    }
                    crate::shell_render::ScreenshotHudHit::Cancel => {
                        self.close_screenshot_hud();
                    }
                }
                return;
            }
            let mode = self.screenshot_hud.as_ref().map(|h| h.mode);
            match mode {
                Some(crate::shell_render::ScreenshotMode::Region) => {
                    if let Some(hud) = self.screenshot_hud.as_mut() {
                        hud.drag_start = Some((px, py));
                        hud.drag_current = Some((px, py));
                        hud.selection = None;
                        self.shell_redraw_needed = true;
                    }
                }
                Some(
                    crate::shell_render::ScreenshotMode::Window
                    | crate::shell_render::ScreenshotMode::FullScreen,
                ) => {
                    self.trigger_screenshot_hud_capture();
                }
                None => {}
            }
        } else if let Some(hud) = self.screenshot_hud.as_mut()
            && hud.mode == crate::shell_render::ScreenshotMode::Region
            && let Some(start) = hud.drag_start.take()
        {
            hud.drag_current = None;
            hud.selection = crate::shell_render::normalize_selection_rect(start, (px, py));
            self.shell_redraw_needed = true;
        }
    }

    /// Handles keyboard input while `ScreenshotHud` is active (`RUSTYOS-133`).
    pub fn handle_screenshot_hud_key(&mut self, raw_sym: u32) {
        use smithay::input::keyboard::Keysym;
        if raw_sym == Keysym::Escape.raw() {
            self.close_screenshot_hud();
        } else if raw_sym == Keysym::Return.raw()
            || raw_sym == Keysym::KP_Enter.raw()
            || raw_sym == Keysym::space.raw()
        {
            self.trigger_screenshot_hud_capture();
        } else if let Some(hud) = self.screenshot_hud.as_mut() {
            if raw_sym == Keysym::_1.raw() {
                hud.mode = crate::shell_render::ScreenshotMode::Region;
                self.shell_redraw_needed = true;
            } else if raw_sym == Keysym::_2.raw() {
                hud.mode = crate::shell_render::ScreenshotMode::Window;
                self.shell_redraw_needed = true;
            } else if raw_sym == Keysym::_3.raw() {
                hud.mode = crate::shell_render::ScreenshotMode::FullScreen;
                self.shell_redraw_needed = true;
            }
        }
    }
}
