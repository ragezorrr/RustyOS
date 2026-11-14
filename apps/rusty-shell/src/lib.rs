//! `RustyOS` independent desktop shell (`RUSTYOS-010`, ADR-0005).
//!
//! Implements the macOS-inspired desktop shell state and versioned IPC client
//! (`TopBar`, `Dock`, `AppLauncher`, `NotificationCenter`, `QuickSettings`,
//! `WorkspaceOverview`, `LockScreen`, and `WallpaperConfig`) running in its own process
//! so shell restarts or crashes never terminate `rusty-compositor`.

use std::fmt::Write as _;
use std::path::Path;
use std::time::Duration;

use rusty_animation::{AnimatedValue, Easing, MotionTokens};
use rusty_ipc::{
    CompositorEvent, DesktopStateSnapshot, IpcClient, IpcError, NotificationSummary, ShellRequest,
    Urgency, WindowSummary,
};
use rusty_services::ServiceRegistry;

pub mod wayland;

/// Canonical design tokens from `DESIGN.md` (Section 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignTokens {
    /// Primary desktop background color (`#0F0A19`).
    pub color_background: &'static str,
    /// Elevated panel/dock/card surface color (`#161023`).
    pub color_surface: &'static str,
    /// Primary interactive accent color (`#A56EFF`).
    pub color_accent: &'static str,
    /// Primary foreground text color (`#F4F0FA`).
    pub color_text: &'static str,
    /// Secondary muted text color (`#A69BB5`).
    pub color_muted: &'static str,
    /// Danger/critical state color (`#E56B75`).
    pub color_danger: &'static str,
    /// Small corner radius (`6 px`).
    pub radius_small: u16,
    /// Medium corner radius (`10 px`).
    pub radius_medium: u16,
    /// Large corner radius (`16 px`).
    pub radius_large: u16,
    /// Pill corner radius (`999 px`).
    pub radius_pill: u16,
    /// Motion tokens shared with `rusty-animation`.
    pub motion: MotionTokens,
}

impl Default for DesignTokens {
    fn default() -> Self {
        Self {
            color_background: "#0F0A19",
            color_surface: "#161023",
            color_accent: "#A56EFF",
            color_text: "#F4F0FA",
            color_muted: "#A69BB5",
            color_danger: "#E56B75",
            radius_small: 6,
            radius_medium: 10,
            radius_large: 16,
            radius_pill: 999,
            motion: MotionTokens::default(),
        }
    }
}

/// Top panel / system status bar state (`TopBar`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopBarState {
    /// Active workspace (`1..=9`).
    pub active_workspace: u8,
    /// Current layout mode (`"floating"` or `"master-stack"`).
    pub layout_mode: String,
    /// Current clock text (e.g. `"10:35"`).
    pub clock_label: String,
    /// Battery percentage (`0..=100`).
    pub battery_percent: u8,
    /// True if AC power is connected (`RUSTYOS-052`).
    pub ac_online: bool,
    /// True if battery is actively charging (`RUSTYOS-052`).
    pub battery_charging: bool,
    /// Network connectivity label.
    pub network_label: String,
    /// Live Wi-Fi signal percentage (`0..=100`, `RUSTYOS-052`).
    pub wifi_signal_percent: Option<u8>,
    /// Median frame interval reported by the compositor (`us`).
    pub frame_interval_p50_us: u64,
    /// 95th percentile input latency reported by the compositor (`us`).
    pub input_latency_p95_us: u64,
    /// Active/unread notification count from compositor HUD (`RUSTYOS-024`).
    pub notification_count: u32,
}

impl Default for TopBarState {
    fn default() -> Self {
        Self {
            active_workspace: 1,
            layout_mode: "floating".to_owned(),
            clock_label: "Wed Apr 1  9:41 AM".to_owned(),
            battery_percent: 100,
            ac_online: false,
            battery_charging: false,
            network_label: "Wi-Fi Connected".to_owned(),
            wifi_signal_percent: None,
            frame_interval_p50_us: 16_667,
            input_latency_p95_us: 0,
            notification_count: 0,
        }
    }
}

/// Entry in the macOS-inspired bottom Dock / task switcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockItem {
    /// Application identifier.
    pub app_id: String,
    /// Display title.
    pub title: String,
    /// Command used to launch the app.
    pub exec: String,
    /// Whether the app is pinned to the dock.
    pub pinned: bool,
    /// Associated live compositor window IDs.
    pub window_ids: Vec<u64>,
    /// Whether one of the app's windows currently holds focus.
    pub focused: bool,
}

/// Application launcher catalog and search filter (`AppLauncher`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppLauncher {
    /// Whether the launcher overlay is currently visible.
    pub open: bool,
    /// Current search query entered by the user.
    pub query: String,
    /// Registered desktop applications.
    pub catalog: Vec<LauncherEntry>,
}

/// Single application entry in [`AppLauncher`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherEntry {
    /// Application identifier.
    pub id: String,
    /// Human-readable application name.
    pub name: String,
    /// Keywords for search matching.
    pub keywords: Vec<String>,
    /// Launch command.
    pub exec: String,
}

impl Default for AppLauncher {
    fn default() -> Self {
        Self {
            open: false,
            query: String::new(),
            catalog: vec![
                LauncherEntry {
                    id: "org.rustyos.terminal".to_owned(),
                    name: "Terminal".to_owned(),
                    keywords: vec!["shell".to_owned(), "console".to_owned(), "cli".to_owned()],
                    exec: "foot".to_owned(),
                },
                LauncherEntry {
                    id: "org.rustyos.files".to_owned(),
                    name: "Files".to_owned(),
                    keywords: vec!["folder".to_owned(), "browser".to_owned()],
                    exec: "rusty-files".to_owned(),
                },
                LauncherEntry {
                    id: "org.rustyos.gears".to_owned(),
                    name: "ES2 Gears Demo".to_owned(),
                    keywords: vec!["opengl".to_owned(), "benchmark".to_owned()],
                    exec: "es2gears_wayland".to_owned(),
                },
                LauncherEntry {
                    id: "org.rustyos.settings".to_owned(),
                    name: "System Settings".to_owned(),
                    keywords: vec!["preferences".to_owned(), "display".to_owned()],
                    exec: "rusty-settings".to_owned(),
                },
            ],
        }
    }
}

impl AppLauncher {
    /// Filters catalog entries matching `query` (case-insensitive substring on name or keywords).
    #[must_use]
    pub fn filtered_entries(&self) -> Vec<&LauncherEntry> {
        let needle = self.query.trim().to_ascii_lowercase();
        if needle.is_empty() {
            return self.catalog.iter().collect();
        }
        self.catalog
            .iter()
            .filter(|entry| {
                entry.name.to_ascii_lowercase().contains(&needle)
                    || entry
                        .keywords
                        .iter()
                        .any(|kw| kw.to_ascii_lowercase().contains(&needle))
            })
            .collect()
    }
}

/// Single notification banner in [`NotificationCenter`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationItem {
    /// Monotonic notification ID.
    pub id: u64,
    /// Urgency level.
    pub urgency: Urgency,
    /// Title string.
    pub title: String,
    /// Body text.
    pub body: String,
    /// Monotonic timestamp when the notification was created.
    pub created_at: Duration,
    /// Auto-dismiss time-to-live (`ttl`).
    pub ttl: Duration,
}

/// Notification queue with monotonic auto-expiration (`NotificationCenter`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NotificationCenter {
    next_id: u64,
    items: Vec<NotificationItem>,
}

impl NotificationCenter {
    /// Pushes a new notification into the queue.
    pub fn push(&mut self, urgency: Urgency, title: String, body: String, now: Duration) -> u64 {
        self.next_id = self.next_id.saturating_add(1);
        let ttl = match urgency {
            Urgency::Low => Duration::from_secs(3),
            Urgency::Normal => Duration::from_secs(5),
            Urgency::Critical => Duration::from_secs(15),
        };
        let id = self.next_id;
        self.items.push(NotificationItem {
            id,
            urgency,
            title,
            body,
            created_at: now,
            ttl,
        });
        id
    }

    /// Expires notifications whose `ttl` has elapsed at monotonic timestamp `now`.
    pub fn expire(&mut self, now: Duration) {
        self.items
            .retain(|item| now.saturating_sub(item.created_at) < item.ttl);
    }

    /// Returns active notifications.
    #[must_use]
    pub fn active(&self) -> &[NotificationItem] {
        &self.items
    }

    /// Dismisses a notification by ID (`RUSTYOS-024`).
    pub fn dismiss(&mut self, id: u64) -> bool {
        let prev = self.items.len();
        self.items.retain(|item| item.id != id);
        self.items.len() < prev
    }

    /// Clears all notifications (`RUSTYOS-024`).
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Synchronizes local notification queue from a compositor [`DesktopStateSnapshot`] (`RUSTYOS-024`).
    pub fn sync_from_snapshot(&mut self, summaries: &[NotificationSummary], now: Duration) {
        let mut synced = Vec::with_capacity(summaries.len());
        for summary in summaries {
            let ttl = match summary.urgency {
                Urgency::Low => Duration::from_secs(4),
                Urgency::Normal => Duration::from_secs(6),
                Urgency::Critical => Duration::from_secs(12),
            };
            let created_at = self
                .items
                .iter()
                .find(|existing| existing.id == summary.id)
                .map_or(now, |existing| existing.created_at);
            self.next_id = self.next_id.max(summary.id);
            synced.push(NotificationItem {
                id: summary.id,
                urgency: summary.urgency,
                title: summary.title.clone(),
                body: summary.body.clone(),
                created_at,
                ttl,
            });
        }
        self.items = synced;
    }
}

/// Quick settings control panel state (`QuickSettings`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct QuickSettingsState {
    /// Whether the quick settings popover is open.
    pub open: bool,
    /// Output audio volume (`0..=100`).
    pub volume_percent: u8,
    /// Display brightness (`0..=100`).
    pub brightness_percent: u8,
    /// Wi-Fi toggle state.
    pub wifi_enabled: bool,
    /// Bluetooth toggle state.
    pub bluetooth_enabled: bool,
    /// Accessibility reduced-motion toggle state.
    pub reduced_motion: bool,
    /// Do Not Disturb mode.
    pub do_not_disturb: bool,
}

impl Default for QuickSettingsState {
    fn default() -> Self {
        Self {
            open: false,
            volume_percent: 75,
            brightness_percent: 85,
            wifi_enabled: true,
            bluetooth_enabled: true,
            reduced_motion: false,
            do_not_disturb: false,
        }
    }
}

/// Wallpaper configuration (`WallpaperConfig`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WallpaperConfig {
    /// Solid or gradient primary color (`#0F0A19`).
    pub primary_color: String,
    /// Secondary gradient accent (`#161023`).
    pub secondary_color: String,
    /// Optional image asset path.
    pub image_path: Option<String>,
}

impl Default for WallpaperConfig {
    fn default() -> Self {
        Self {
            primary_color: "#0F0A19".to_owned(),
            secondary_color: "#161023".to_owned(),
            image_path: None,
        }
    }
}

/// Complete state of the independent `rusty-shell` process.
#[derive(Debug, Clone)]
pub struct DesktopShell {
    /// Shared design tokens.
    pub tokens: DesignTokens,
    /// Top system status bar.
    pub top_bar: TopBarState,
    /// Bottom application dock and task switcher.
    pub dock: Vec<DockItem>,
    /// Application launcher.
    pub launcher: AppLauncher,
    /// Notification center.
    pub notifications: NotificationCenter,
    /// Quick settings popover.
    pub quick_settings: QuickSettingsState,
    /// Whether workspace overview is open.
    pub overview_active: bool,
    /// Animated overview progress (`0.0` closed -> `1.0` open).
    pub overview_progress: AnimatedValue,
    /// Whether the session lock screen is engaged.
    pub screen_locked: bool,
    /// Desktop wallpaper configuration.
    pub wallpaper: WallpaperConfig,
    /// System service adapters registry.
    pub services: ServiceRegistry,
}

impl Default for DesktopShell {
    fn default() -> Self {
        Self {
            tokens: DesignTokens::default(),
            top_bar: TopBarState::default(),
            dock: vec![
                DockItem {
                    app_id: "org.rustyos.terminal".to_owned(),
                    title: "Terminal".to_owned(),
                    exec: "foot".to_owned(),
                    pinned: true,
                    window_ids: Vec::new(),
                    focused: false,
                },
                DockItem {
                    app_id: "org.rustyos.gears".to_owned(),
                    title: "ES2 Gears".to_owned(),
                    exec: "es2gears_wayland".to_owned(),
                    pinned: true,
                    window_ids: Vec::new(),
                    focused: false,
                },
            ],
            launcher: AppLauncher::default(),
            notifications: NotificationCenter::default(),
            quick_settings: QuickSettingsState::default(),
            overview_active: false,
            overview_progress: AnimatedValue::new_settled(0.0),
            screen_locked: false,
            wallpaper: WallpaperConfig::default(),
            services: ServiceRegistry::new(),
        }
    }
}

impl DesktopShell {
    /// Applies a [`DesktopStateSnapshot`] received from `rusty-compositor` over IPC.
    pub fn apply_compositor_snapshot(&mut self, snapshot: &DesktopStateSnapshot, now: Duration) {
        self.top_bar.active_workspace = snapshot.active_workspace;
        self.top_bar.layout_mode.clone_from(&snapshot.layout_mode);
        self.top_bar.frame_interval_p50_us = snapshot.frame_interval_p50_us;
        self.top_bar.input_latency_p95_us = snapshot.input_latency_p95_us;
        self.top_bar.notification_count = snapshot.notification_count;

        self.tokens.motion.reduced_motion = snapshot.reduced_motion;
        self.quick_settings.reduced_motion = snapshot.reduced_motion;
        self.launcher.open = snapshot.launcher_active;
        self.launcher.query.clone_from(&snapshot.launcher_query);
        self.screen_locked = snapshot.screen_locked;
        self.notifications
            .sync_from_snapshot(&snapshot.notifications, now);

        if self.overview_active != snapshot.overview_active {
            self.overview_active = snapshot.overview_active;
            let target = if snapshot.overview_active { 1.0 } else { 0.0 };
            self.overview_progress.retarget(
                target,
                now,
                self.tokens.motion.long_duration,
                Easing::EaseOutCubic,
                self.tokens.motion.reduced_motion,
            );
        }

        self.sync_dock_windows(&snapshot.windows);
    }

    fn sync_dock_windows(&mut self, windows: &[WindowSummary]) {
        for item in &mut self.dock {
            item.window_ids.clear();
            item.focused = false;
        }
        self.dock.retain(|item| item.pinned);

        for win in windows {
            if let Some(existing) = self.dock.get_mut(1) {
                existing.window_ids.push(win.id);
                if win.focused {
                    existing.focused = true;
                }
            } else {
                self.dock.push(DockItem {
                    app_id: format!("window-{}", win.id),
                    title: format!("Window #{}", win.id),
                    exec: String::new(),
                    pinned: false,
                    window_ids: vec![win.id],
                    focused: win.focused,
                });
            }
        }
    }

    /// Advances shell animations, updates system service metrics, and expires old notifications.
    pub fn tick(&mut self, now: Duration) {
        let _ = self.overview_progress.tick(now);
        self.notifications.expire(now);

        // Update system hardware status in TopBar and QuickSettings
        let pwr = self.services.power.query_status();
        self.top_bar.battery_percent = pwr.percentage;
        self.top_bar.ac_online = pwr.ac_online;
        self.top_bar.battery_charging = pwr.state == rusty_services::BatteryState::Charging;

        let net = self.services.network.query_status();
        self.top_bar.network_label = net.status_label;
        self.top_bar.wifi_signal_percent = net.wifi_signal_percent;
        self.quick_settings.wifi_enabled = net.connected;

        let aud = self.services.audio.query_status();
        self.quick_settings.volume_percent = aud.volume_percent;
    }

    /// Renders a structured text HUD summary of all Phase 6 shell surfaces.
    #[must_use]
    pub fn render_hud_summary(&self) -> String {
        let mut out = String::with_capacity(512);
        let _ = writeln!(
            out,
            "[TopBar] ws={} layout={} net='{}' bat={}% frame_p50={}us input_p95={}us",
            self.top_bar.active_workspace,
            self.top_bar.layout_mode,
            self.top_bar.network_label,
            self.top_bar.battery_percent,
            self.top_bar.frame_interval_p50_us,
            self.top_bar.input_latency_p95_us
        );
        let dock_summary: Vec<String> = self
            .dock
            .iter()
            .map(|d| {
                format!(
                    "{}(wins={},focused={})",
                    d.title,
                    d.window_ids.len(),
                    d.focused
                )
            })
            .collect();
        let _ = writeln!(out, "[Dock] {}", dock_summary.join(" | "));
        let _ = writeln!(
            out,
            "[QuickSettings] vol={}% bright={}% wifi={} bt={} reduced_motion={}",
            self.quick_settings.volume_percent,
            self.quick_settings.brightness_percent,
            self.quick_settings.wifi_enabled,
            self.quick_settings.bluetooth_enabled,
            self.quick_settings.reduced_motion
        );
        let _ = writeln!(
            out,
            "[Overview] active={} progress={:.2} | [LockScreen] locked={} | [Notifications] active={}",
            self.overview_active,
            self.overview_progress.current(),
            self.screen_locked,
            self.notifications.active().len()
        );
        out
    }

    /// Connects to a running `rusty-compositor` over `socket_path`, executes `commands`,
    /// synchronizes the final [`DesktopStateSnapshot`], and returns the resulting event.
    ///
    /// # Errors
    ///
    /// Returns [`IpcError`] if the connection, handshake, or command exchange fails.
    pub fn sync_with_compositor(
        &mut self,
        socket_path: &Path,
        commands: &[ShellRequest],
        now: Duration,
    ) -> Result<DesktopStateSnapshot, IpcError> {
        let mut client = IpcClient::connect(socket_path, "rusty-shell")?;
        let mut latest_snapshot = DesktopStateSnapshot::default();

        if commands.is_empty() {
            if let CompositorEvent::StateSnapshot(snap) =
                client.send_request(&ShellRequest::GetStateSnapshot)?
            {
                self.apply_compositor_snapshot(&snap, now);
                latest_snapshot = snap;
            }
            return Ok(latest_snapshot);
        }

        for cmd in commands {
            match client.send_request(cmd)? {
                CompositorEvent::StateSnapshot(snap) => {
                    self.apply_compositor_snapshot(&snap, now);
                    latest_snapshot = snap;
                }
                CompositorEvent::Error { code, message } => {
                    return Err(IpcError::InvalidFrame(format!("{code}: {message}")));
                }
                CompositorEvent::HandshakeAck { .. } => {}
            }
        }
        Ok(latest_snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusty_ipc::IpcServer;
    use std::path::PathBuf;

    #[test]
    fn launcher_filtering_and_notification_expiration_work_deterministically() {
        let mut shell = DesktopShell::default();
        assert_eq!(shell.launcher.filtered_entries().len(), 4);

        shell.launcher.query = "gear".to_owned();
        let matches = shell.launcher.filtered_entries();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].id, "org.rustyos.gears");

        // Push normal (5s TTL) and low (3s TTL) notifications.
        shell.notifications.push(
            Urgency::Low,
            "Wi-Fi".to_owned(),
            "Connected".to_owned(),
            Duration::from_secs(1),
        );
        shell.notifications.push(
            Urgency::Normal,
            "Update".to_owned(),
            "Ready".to_owned(),
            Duration::from_secs(1),
        );
        assert_eq!(shell.notifications.active().len(), 2);

        // At t = 5s, low urgency notification (created at 1s, TTL 3s) expires.
        shell.tick(Duration::from_secs(5));
        assert_eq!(shell.notifications.active().len(), 1);
        assert_eq!(shell.notifications.active()[0].title, "Update");
    }

    #[test]
    fn shell_lifecycle_restart_and_resync_over_ipc_without_restarting_server()
    -> Result<(), IpcError> {
        let sock = PathBuf::from(format!(
            "/tmp/rustyos-shell-lifecycle-{}.sock",
            std::process::id()
        ));
        let mut server = IpcServer::bind(sock.clone())?;

        // Simulated compositor state across multiple shell restarts.
        let mut comp_state = DesktopStateSnapshot {
            active_workspace: 1,
            layout_mode: "floating".to_owned(),
            reduced_motion: false,
            overview_active: false,
            launcher_active: false,
            launcher_query: String::new(),
            screen_locked: false,
            notification_count: 1,
            frame_interval_p50_us: 16_700,
            input_latency_p95_us: 6_400,
            windows: vec![WindowSummary {
                id: 10,
                workspace: 1,
                focused: true,
                maximized: false,
            }],
            notifications: vec![NotificationSummary {
                id: 1,
                urgency: Urgency::Normal,
                title: "Welcome".to_owned(),
                body: "RustyOS Ready".to_owned(),
            }],
        };

        let server_thread = std::thread::spawn(move || {
            let start = std::time::Instant::now();
            while start.elapsed() < Duration::from_millis(400) {
                server.poll_requests(|req| match req {
                    ShellRequest::Handshake {
                        protocol_version, ..
                    } => CompositorEvent::HandshakeAck {
                        protocol_version,
                        server_version: "0.1.0".to_owned(),
                    },
                    ShellRequest::SwitchWorkspace { workspace } => {
                        comp_state.active_workspace = workspace;
                        CompositorEvent::StateSnapshot(comp_state.clone())
                    }
                    ShellRequest::ToggleOverview => {
                        comp_state.overview_active = !comp_state.overview_active;
                        CompositorEvent::StateSnapshot(comp_state.clone())
                    }
                    ShellRequest::SetReducedMotion { enabled } => {
                        comp_state.reduced_motion = enabled;
                        CompositorEvent::StateSnapshot(comp_state.clone())
                    }
                    _ => CompositorEvent::StateSnapshot(comp_state.clone()),
                });
                std::thread::sleep(Duration::from_millis(5));
            }
        });

        // Shell Instance #1 connects, switches workspace to 5 and opens overview, then terminates (dropped).
        {
            let mut shell_1 = DesktopShell::default();
            let snap = shell_1.sync_with_compositor(
                &sock,
                &[
                    ShellRequest::SwitchWorkspace { workspace: 5 },
                    ShellRequest::ToggleOverview,
                ],
                Duration::from_millis(10),
            )?;
            assert_eq!(snap.active_workspace, 5);
            assert!(shell_1.overview_active);
        }

        // Shell Instance #2 (restarted shell process) connects to the same compositor and recovers state.
        {
            let mut shell_2 = DesktopShell::default();
            let snap = shell_2.sync_with_compositor(&sock, &[], Duration::from_millis(50))?;
            assert_eq!(snap.active_workspace, 5);
            assert!(shell_2.overview_active);
            assert_eq!(shell_2.top_bar.active_workspace, 5);
            let hud = shell_2.render_hud_summary();
            assert!(hud.contains("ws=5"));
            assert!(hud.contains("active=true"));
            assert!(hud.contains("[Notifications] active=1"));
            assert_eq!(shell_2.top_bar.notification_count, 1);
        }

        let _ = server_thread.join();
        Ok(())
    }
}
