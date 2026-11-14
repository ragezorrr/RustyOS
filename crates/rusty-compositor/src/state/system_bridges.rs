//! Non-blocking bridges for PAM lock auth, systemd-logind, Polkit, and XDG Portals.

use std::time::{Duration, Instant};

use rusty_wm::{OutputId, Rect, WindowId};
use smithay::utils::{Logical, Point, Serial};
use tracing::{info, warn};

use super::{
    ActiveScreencastSession, PendingPickColorState, PendingPortalCapture, PolkitPromptState,
    PortalPromptState, PortalSourceCandidate, ResolvedPortalTarget, RustyCompositorState,
    ScreencastCaptureTarget,
};

impl RustyCompositorState {
    /// Starts non-blocking background system integration threads shared across Winit and DRM (`RUSTYOS-056`).
    pub fn start_background_services(&mut self) {
        self.telemetry = crate::telemetry::SystemMetricsHandle::start_background();
        self.dbus_notifications =
            crate::dbus_notifications::DbusNotificationHandle::start_background();
        self.tray = crate::tray::TrayWatcherHandle::start_background();
        self.auth = rusty_services::AsyncAuthHandle::start_background();
        if self.user_config.lock_on_sleep {
            self.logind = rusty_services::LogindMonitorHandle::start_background();
        }
        if self.user_config.enable_polkit_agent {
            self.polkit = rusty_services::PolkitAgentHandle::start_background();
        }
        self.portal = rusty_services::PortalHandle::start_background();
        self.sync_portal_snapshot();
    }

    /// Returns `true` and clears the pending shell redraw flag when async security events updated UI state.
    pub fn take_shell_redraw_needed(&mut self) -> bool {
        std::mem::take(&mut self.shell_redraw_needed)
    }

    /// Resolves the active account username selected on the Lock Screen (`RUSTYOS-056`).
    #[must_use]
    pub fn selected_lock_username(&self) -> String {
        let users = self.telemetry.snapshot().users;
        if let Some(user) = users.get(self.lock_selected_user)
            && !user.username.trim().is_empty()
        {
            return user.username.clone();
        }
        if let Ok(user) = std::env::var("USER")
            && rusty_services::is_valid_unix_username(user.trim())
        {
            return user.trim().to_owned();
        }
        "user".to_owned()
    }

    /// Submits the Lock Screen password asynchronously to `unix_chkpwd` (or unlocks directly if disabled).
    pub fn submit_lock_password(&mut self, serial: Serial) {
        if !self.screen_locked {
            return;
        }
        if !self.user_config.require_password_on_lock {
            self.set_screen_locked(false, serial);
            return;
        }
        if self.lock_auth_checking {
            return;
        }
        let req_id = self.lock_next_request_id;
        self.lock_next_request_id = self.lock_next_request_id.saturating_add(1);
        let username = self.selected_lock_username();
        let secret = self.lock_password.take();
        if self.auth.submit_check(req_id, username, secret) {
            self.lock_auth_checking = true;
            self.lock_pending_request_id = Some(req_id);
            self.lock_auth_error = None;
        } else {
            self.lock_auth_error = Some("Authentication service unavailable".to_owned());
        }
        self.shell_redraw_needed = true;
    }

    /// Submits typed credentials from the active `PolicyKit` dialog (`RUSTYOS-056`).
    pub fn submit_polkit_password(&mut self) {
        let Some(prompt) = self.polkit_prompt.as_mut() else {
            return;
        };
        if prompt.checking {
            return;
        }
        let cookie = prompt.cookie.clone();
        prompt.checking = true;
        prompt.error_message = None;
        let secret = self.polkit_password.take();
        self.polkit.submit_password(cookie, secret);
        self.shell_redraw_needed = true;
    }

    /// Cancels the active `PolicyKit` authentication prompt and notifies the D-Bus caller (`RUSTYOS-056`).
    pub fn cancel_polkit_prompt(&mut self, serial: Serial) {
        let Some(prompt) = self.polkit_prompt.take() else {
            return;
        };
        self.polkit_password.clear();
        self.polkit.cancel(prompt.cookie);
        if !self.is_session_locked() {
            self.sync_space_with_wm(serial);
        }
        self.shell_redraw_needed = true;
    }

    /// Non-blockingly drains PAM auth, `systemd-logind`, `PolicyKit`, and XDG Portal queues (`RUSTYOS-056`/`057`).
    pub fn drain_system_integration_events(&mut self, serial: Serial) -> bool {
        let a = self.drain_lock_auth_results(serial);
        let l = self.drain_logind_events(serial);
        let p = self.drain_polkit_events(serial);
        let q = self.drain_portal_events(serial);
        let changed = a || l || p || q;
        if changed {
            self.shell_redraw_needed = true;
        }
        changed
    }

    /// Synchronizes current outputs, visible windows, and lock status into [`rusty_services::PortalHandle`] (`RUSTYOS-057`).
    pub fn sync_portal_snapshot(&self) {
        let outputs = self
            .output_map
            .entries()
            .iter()
            .map(|entry| {
                let geo = entry.logical_geometry();
                rusty_services::PortalOutputInfo {
                    id: u64::from(entry.id.0),
                    name: entry.name.clone(),
                    x: entry.location.x,
                    y: entry.location.y,
                    width: geo.size.w.max(1),
                    height: geo.size.h.max(1),
                    refresh_hz: entry.refresh_hz(),
                    is_primary: entry.is_primary,
                }
            })
            .collect();
        let focused = self.wm.focused_window();
        let windows = self
            .wm
            .visible_windows()
            .into_iter()
            .rev()
            .map(|win| rusty_services::PortalWindowInfo {
                id: win.id.0,
                title: self.window_title(win.id),
                app_id: self
                    .window_app_id(win.id)
                    .unwrap_or_else(|| "application".to_owned()),
                x: win.geometry.x,
                y: win.geometry.y,
                width: win.geometry.width.max(1),
                height: win.geometry.height.max(1),
                focused: focused == Some(win.id),
            })
            .collect();
        self.portal
            .update_snapshot(rusty_services::PortalDesktopSnapshot {
                outputs,
                windows,
                screen_locked: self.is_session_locked(),
            });
    }

    /// Immediately terminates active portal prompts, pick-color captures, and PipeWire screencasts on session lock (`SECURITY.md`).
    pub(crate) fn abort_portal_activity_for_lock(&mut self) {
        if let Some(prompt) = self.portal_prompt.take() {
            self.portal
                .send_reply(rusty_services::PortalReply::PromptDecision {
                    request_id: prompt.request_id,
                    accepted: false,
                    selection: None,
                });
        }
        if let Some(pick) = self.portal_pick_color.take() {
            self.portal
                .send_reply(rusty_services::PortalReply::PickColorResult {
                    request_id: pick.request_id,
                    result: Err("session locked".to_owned()),
                });
        }
        if let Some(pending) = self.pending_portal_capture.take() {
            match pending {
                PendingPortalCapture::Screenshot { request_id, .. } => {
                    self.portal
                        .send_reply(rusty_services::PortalReply::ScreenshotResult {
                            request_id,
                            result: Err("session locked".to_owned()),
                        });
                }
                PendingPortalCapture::PickColorSample { request_id, .. } => {
                    self.portal
                        .send_reply(rusty_services::PortalReply::PickColorResult {
                            request_id,
                            result: Err("session locked".to_owned()),
                        });
                }
            }
        }
        let stopped = std::mem::take(&mut self.active_screencasts);
        for session in stopped {
            self.portal.notify_session_closed(&session.session_handle);
        }
        self.sync_portal_snapshot();
    }

    /// Moves the highlighted candidate row inside the active XDG Portal consent dialog.
    pub fn move_portal_selection(&mut self, delta: isize) {
        let Some(prompt) = self.portal_prompt.as_mut() else {
            return;
        };
        let len = prompt.candidates.len();
        if len == 0 {
            return;
        }
        let next = if delta < 0 {
            if prompt.selected == 0 {
                len - 1
            } else {
                prompt.selected - 1
            }
        } else {
            (prompt.selected + 1) % len
        };
        prompt.selected = next;
        self.shell_redraw_needed = true;
    }

    /// Toggles embedded software cursor inclusion when permitted by the active `ScreenCast` prompt.
    pub fn toggle_portal_include_cursor(&mut self) {
        let Some(prompt) = self.portal_prompt.as_mut() else {
            return;
        };
        if prompt.is_screencast && prompt.cursor_mode_editable {
            prompt.include_cursor = !prompt.include_cursor;
            self.shell_redraw_needed = true;
        }
    }

    /// Confirms the highlighted capture source in the active XDG Portal consent dialog (`RUSTYOS-057`).
    pub fn accept_portal_prompt(&mut self, serial: Serial) {
        let Some(prompt) = self.portal_prompt.take() else {
            return;
        };
        let selected = prompt
            .candidates
            .get(prompt.selected)
            .or_else(|| prompt.candidates.first());
        let selection = selected.map(|cand| {
            let output_id = cand.output_id.map(|id| u64::from(id.0));
            let window_id = cand.window_id.map(|id| id.0);
            if prompt.is_screencast {
                let source_type = if cand.is_window {
                    rusty_services::SCREENCAST_SOURCE_WINDOW
                } else {
                    rusty_services::SCREENCAST_SOURCE_MONITOR
                };
                let cursor_mode = if prompt.include_cursor {
                    rusty_services::SCREENCAST_CURSOR_EMBEDDED
                } else {
                    rusty_services::SCREENCAST_CURSOR_HIDDEN
                };
                rusty_services::PortalSelection::ScreenCast {
                    source_type,
                    output_id,
                    window_id,
                    cursor_mode,
                    persist_mode: prompt.persist_mode,
                }
            } else {
                let target = if cand.is_window {
                    rusty_services::SCREENSHOT_TARGET_WINDOW
                } else {
                    rusty_services::SCREENSHOT_TARGET_SCREEN
                };
                rusty_services::PortalSelection::Screenshot {
                    target,
                    output_id,
                    window_id,
                }
            }
        });
        let accepted = selection.is_some();
        self.portal
            .send_reply(rusty_services::PortalReply::PromptDecision {
                request_id: prompt.request_id,
                accepted,
                selection,
            });
        if !self.is_session_locked() {
            self.sync_space_with_wm(serial);
        }
        self.shell_redraw_needed = true;
    }

    /// Denies/cancels the active XDG Portal consent modal (`RUSTYOS-057`).
    pub fn cancel_portal_prompt(&mut self, serial: Serial) {
        let Some(prompt) = self.portal_prompt.take() else {
            return;
        };
        self.portal
            .send_reply(rusty_services::PortalReply::PromptDecision {
                request_id: prompt.request_id,
                accepted: false,
                selection: None,
            });
        if !self.is_session_locked() {
            self.sync_space_with_wm(serial);
        }
        self.shell_redraw_needed = true;
    }

    /// Cancels an active interactive `PickColor` sampling banner (`Esc`).
    pub fn cancel_portal_pick_color(&mut self) {
        let Some(pick) = self.portal_pick_color.take() else {
            return;
        };
        self.portal
            .send_reply(rusty_services::PortalReply::PickColorResult {
                request_id: pick.request_id,
                result: Err("cancelled".to_owned()),
            });
        self.shell_redraw_needed = true;
    }

    /// Queues pixel readback at `pointer_global` when the user clicks during `PickColor`.
    pub fn trigger_pick_color_sample(&mut self, pointer_global: Point<f64, Logical>) {
        let Some(pick) = self.portal_pick_color.take() else {
            return;
        };
        self.pending_portal_capture = Some(PendingPortalCapture::PickColorSample {
            request_id: pick.request_id,
            pointer_global,
        });
        self.shell_redraw_needed = true;
    }

    /// Returns `true` when at least one active PipeWire screencast needs a captured frame now.
    #[must_use]
    pub fn has_due_screencast_frame(&self, now: Instant) -> bool {
        if self.is_session_locked() {
            return false;
        }
        self.active_screencasts.iter().any(|sess| {
            sess.stream.is_alive()
                && (!sess.pushed_initial_frame
                    || (sess.stream.is_consumer_active()
                        && now.duration_since(sess.last_pushed_at)
                            >= Duration::from_secs_f64(1.0 / f64::from(sess.fps.clamp(15, 120)))))
        })
    }

    /// Resolves capture geometry, scale, output-local crop, and stream dimensions for a portal target.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub(crate) fn resolve_portal_capture_target(
        &self,
        target: ScreencastCaptureTarget,
    ) -> ResolvedPortalTarget {
        if let ScreencastCaptureTarget::Window(win_id) = target
            && let Some(win) = self.wm.window(win_id)
        {
            let center = Point::<f64, Logical>::from((
                f64::from(win.geometry.x) + f64::from(win.geometry.width) * 0.5,
                f64::from(win.geometry.y) + f64::from(win.geometry.height) * 0.5,
            ));
            let entry = self
                .output_map
                .output_under(center)
                .and_then(|oid| self.output_map.get(oid))
                .unwrap_or_else(|| self.output_map.primary());
            let phys = entry.transformed_physical_size();
            let scale = entry.scale;
            let local_crop = Rect::new(
                win.geometry.x - entry.location.x,
                win.geometry.y - entry.location.y,
                win.geometry.width.max(1),
                win.geometry.height.max(1),
            );
            let stream_w =
                ((f64::from(local_crop.width) * scale).round() as i32).clamp(1, phys.w.max(1));
            let stream_h =
                ((f64::from(local_crop.height) * scale).round() as i32).clamp(1, phys.h.max(1));
            return ResolvedPortalTarget {
                output_id: entry.id,
                capture_size: phys,
                output_scale: scale,
                crop: Some(local_crop),
                origin: Point::from((win.geometry.x, win.geometry.y)),
                mapping_id: format!("window:{}", win_id.0),
                refresh_hz: entry.refresh_hz().clamp(30, 120),
                stream_size: (
                    u32::try_from(stream_w).unwrap_or(1),
                    u32::try_from(stream_h).unwrap_or(1),
                ),
            };
        }

        let entry = match target {
            ScreencastCaptureTarget::Output(oid) => self
                .output_map
                .get(oid)
                .unwrap_or_else(|| self.output_map.primary()),
            ScreencastCaptureTarget::Window(_) => self.output_map.primary(),
        };
        let phys = entry.transformed_physical_size();
        let stream_w = u32::try_from(phys.w.max(1)).unwrap_or(1);
        let stream_h = u32::try_from(phys.h.max(1)).unwrap_or(1);
        ResolvedPortalTarget {
            output_id: entry.id,
            capture_size: phys,
            output_scale: entry.scale,
            crop: None,
            origin: entry.location,
            mapping_id: format!("output:{}", entry.name),
            refresh_hz: entry.refresh_hz().clamp(30, 120),
            stream_size: (stream_w, stream_h),
        }
    }

    fn build_portal_candidates(
        &self,
        kind: &rusty_services::PortalPromptKind,
    ) -> (Vec<PortalSourceCandidate>, usize) {
        let mut candidates = Vec::new();
        let (allow_outputs, allow_windows, restored, prefer_window) = match kind {
            rusty_services::PortalPromptKind::Screenshot { requested_target } => (
                true,
                true,
                None,
                (*requested_target & rusty_services::SCREENSHOT_TARGET_WINDOW) != 0,
            ),
            rusty_services::PortalPromptKind::ScreenCast {
                allowed_types,
                restored_target,
                ..
            } => (
                (*allowed_types & rusty_services::SCREENCAST_SOURCE_MONITOR) != 0,
                (*allowed_types & rusty_services::SCREENCAST_SOURCE_WINDOW) != 0,
                restored_target.as_deref(),
                false,
            ),
        };

        if allow_outputs {
            for out in self.output_map.entries() {
                let geo = out.logical_geometry();
                let role = if out.is_primary { "Primary" } else { "Display" };
                candidates.push(PortalSourceCandidate {
                    label: format!("Monitor: {}", out.name),
                    detail: format!(
                        "{}x{} @ {} Hz ({role})",
                        geo.size.w,
                        geo.size.h,
                        out.refresh_hz()
                    ),
                    is_window: false,
                    output_id: Some(out.id),
                    window_id: None,
                });
            }
        }
        if allow_windows {
            for win in self.wm.visible_windows().into_iter().rev() {
                let title = self.window_title(win.id);
                let app = self
                    .window_app_id(win.id)
                    .unwrap_or_else(|| "Window".to_owned());
                candidates.push(PortalSourceCandidate {
                    label: title,
                    detail: format!("{app} ({}x{})", win.geometry.width, win.geometry.height),
                    is_window: true,
                    output_id: None,
                    window_id: Some(win.id),
                });
            }
        }
        if candidates.is_empty() {
            let primary = self.output_map.primary();
            let geo = primary.logical_geometry();
            candidates.push(PortalSourceCandidate {
                label: format!("Monitor: {}", primary.name),
                detail: format!(
                    "{}x{} @ {} Hz",
                    geo.size.w,
                    geo.size.h,
                    primary.refresh_hz()
                ),
                is_window: false,
                output_id: Some(primary.id),
                window_id: None,
            });
        }

        let default_idx = restored
            .and_then(|tok| {
                if let Some(name) = tok.strip_prefix("output:") {
                    return candidates.iter().position(|c| {
                        c.output_id
                            .and_then(|oid| self.output_map.get(oid))
                            .is_some_and(|o| o.name == name)
                    });
                }
                if let Some(raw_id) = tok.strip_prefix("window:")
                    && let Ok(id) = raw_id.parse::<u64>()
                {
                    return candidates
                        .iter()
                        .position(|c| c.window_id == Some(WindowId(id)));
                }
                None
            })
            .or_else(|| {
                prefer_window
                    .then(|| candidates.iter().position(|c| c.is_window))
                    .flatten()
            })
            .unwrap_or(0);
        (candidates, default_idx)
    }

    fn handle_start_screencast(
        &mut self,
        request_id: u64,
        session_handle: String,
        app_id: &str,
        selection: &rusty_services::PortalSelection,
    ) {
        if self.is_session_locked() {
            self.portal
                .send_reply(rusty_services::PortalReply::ScreenCastResult {
                    request_id,
                    session_handle,
                    result: Err("session locked".to_owned()),
                });
            return;
        }
        let rusty_services::PortalSelection::ScreenCast {
            source_type,
            output_id,
            window_id,
            cursor_mode,
            persist_mode: _,
        } = *selection
        else {
            self.portal
                .send_reply(rusty_services::PortalReply::ScreenCastResult {
                    request_id,
                    session_handle,
                    result: Err("unexpected portal selection variant".to_owned()),
                });
            return;
        };

        let target = if let Some(raw_win) = window_id
            && self.wm.window(WindowId(raw_win)).is_some()
        {
            ScreencastCaptureTarget::Window(WindowId(raw_win))
        } else if let Some(raw_out) = output_id
            && let Ok(id) = u32::try_from(raw_out)
            && self.output_map.get(OutputId(id)).is_some()
        {
            ScreencastCaptureTarget::Output(OutputId(id))
        } else {
            ScreencastCaptureTarget::Output(self.output_map.primary().id)
        };

        let resolved = self.resolve_portal_capture_target(target);
        let (width, height) = resolved.stream_size;
        let fps = resolved.refresh_hz;
        let config = rusty_services::PipeWireStreamConfig {
            width,
            height,
            fps,
            app_name: app_id.to_owned(),
            node_description: format!("RustyOS ScreenCast ({})", resolved.mapping_id),
        };
        let stream = match rusty_services::PipeWireStreamHandle::spawn(&config) {
            Ok(handle) => handle,
            Err(err) => {
                self.portal
                    .send_reply(rusty_services::PortalReply::ScreenCastResult {
                        request_id,
                        session_handle,
                        result: Err(err),
                    });
                return;
            }
        };

        let desc = rusty_services::PortalStreamDescriptor {
            node_id: stream.node_id(),
            pipewire_serial: stream.pipewire_serial(),
            x: resolved.origin.x,
            y: resolved.origin.y,
            width: i32::try_from(width).unwrap_or(i32::MAX),
            height: i32::try_from(height).unwrap_or(i32::MAX),
            source_type,
            mapping_id: resolved.mapping_id,
        };

        self.active_screencasts
            .retain(|s| s.session_handle != session_handle);
        self.active_screencasts.push(ActiveScreencastSession {
            session_handle: session_handle.clone(),
            target,
            include_cursor: (cursor_mode & rusty_services::SCREENCAST_CURSOR_EMBEDDED) != 0,
            stream,
            fps,
            last_pushed_at: Instant::now(),
            pushed_initial_frame: false,
        });
        self.portal
            .send_reply(rusty_services::PortalReply::ScreenCastResult {
                request_id,
                session_handle,
                result: Ok(vec![desc]),
            });
    }

    fn prune_dead_screencasts(&mut self) -> bool {
        let mut changed = false;
        let mut i = 0;
        while i < self.active_screencasts.len() {
            let target_valid = match self.active_screencasts[i].target {
                ScreencastCaptureTarget::Output(oid) => self.output_map.get(oid).is_some(),
                ScreencastCaptureTarget::Window(wid) => self.wm.window(wid).is_some(),
            };
            if self.active_screencasts[i].stream.is_alive() && target_valid {
                i += 1;
            } else {
                let removed = self.active_screencasts.swap_remove(i);
                self.portal.notify_session_closed(&removed.session_handle);
                changed = true;
            }
        }
        changed
    }

    fn drain_portal_events(&mut self, serial: Serial) -> bool {
        self.sync_portal_snapshot();
        let mut changed = self.prune_dead_screencasts();

        for event in self.portal.drain_events() {
            changed = true;
            match event {
                rusty_services::PortalEvent::ShowPrompt(req) => {
                    self.handle_portal_show_prompt(req, serial);
                }
                rusty_services::PortalEvent::CaptureScreenshot {
                    request_id,
                    app_id: _,
                    target,
                    output_id,
                    window_id,
                } => {
                    if self.is_session_locked() {
                        self.portal
                            .send_reply(rusty_services::PortalReply::ScreenshotResult {
                                request_id,
                                result: Err("session locked".to_owned()),
                            });
                        continue;
                    }
                    self.pending_portal_capture = Some(PendingPortalCapture::Screenshot {
                        request_id,
                        target,
                        output_id: output_id.and_then(|v| u32::try_from(v).ok()).map(OutputId),
                        window_id: window_id.map(WindowId),
                    });
                }
                rusty_services::PortalEvent::PickColor { request_id, app_id } => {
                    if self.is_session_locked() {
                        self.portal
                            .send_reply(rusty_services::PortalReply::PickColorResult {
                                request_id,
                                result: Err("session locked".to_owned()),
                            });
                        continue;
                    }
                    if let Some(prev) = self.portal_pick_color.take() {
                        self.portal
                            .send_reply(rusty_services::PortalReply::PickColorResult {
                                request_id: prev.request_id,
                                result: Err("superseded".to_owned()),
                            });
                    }
                    self.portal_pick_color = Some(PendingPickColorState { request_id, app_id });
                }
                rusty_services::PortalEvent::StartScreenCast {
                    request_id,
                    session_handle,
                    app_id,
                    selection,
                } => {
                    self.handle_start_screencast(request_id, session_handle, &app_id, &selection);
                }
                rusty_services::PortalEvent::StopSession { session_handle } => {
                    if self
                        .portal_prompt
                        .as_ref()
                        .and_then(|p| p.session_handle.as_deref())
                        == Some(session_handle.as_str())
                        && let Some(p) = self.portal_prompt.take()
                    {
                        self.portal
                            .send_reply(rusty_services::PortalReply::PromptDecision {
                                request_id: p.request_id,
                                accepted: false,
                                selection: None,
                            });
                    }
                    self.active_screencasts
                        .retain(|s| s.session_handle != session_handle);
                }
                rusty_services::PortalEvent::CancelRequest { request_id } => {
                    if self
                        .portal_prompt
                        .as_ref()
                        .is_some_and(|p| p.request_id == request_id)
                    {
                        self.portal_prompt = None;
                    }
                    if self
                        .portal_pick_color
                        .as_ref()
                        .is_some_and(|p| p.request_id == request_id)
                    {
                        self.portal_pick_color = None;
                    }
                    if matches!(
                        self.pending_portal_capture,
                        Some(PendingPortalCapture::Screenshot { request_id: id, .. }
                            | PendingPortalCapture::PickColorSample { request_id: id, .. }) if id == request_id
                    ) {
                        self.pending_portal_capture = None;
                    }
                }
            }
        }
        changed
    }

    fn handle_portal_show_prompt(
        &mut self,
        req: rusty_services::PortalPromptRequest,
        serial: Serial,
    ) {
        if self.is_session_locked() {
            self.portal
                .send_reply(rusty_services::PortalReply::PromptDecision {
                    request_id: req.request_id,
                    accepted: false,
                    selection: None,
                });
            return;
        }
        if let Some(prev) = self.portal_prompt.take() {
            self.portal
                .send_reply(rusty_services::PortalReply::PromptDecision {
                    request_id: prev.request_id,
                    accepted: false,
                    selection: None,
                });
        }
        self.window_switcher = None;
        self.close_launcher();
        self.close_quick_settings();
        self.close_top_bar_menu();
        self.close_overview();
        self.deactivate_focused_shortcut_inhibitor();
        let keyboard = self.keyboard.clone();
        keyboard.set_focus(self, None, serial);

        let (candidates, selected) = self.build_portal_candidates(&req.kind);
        let (
            is_screencast,
            session_handle,
            requested_target,
            allowed_types,
            include_cursor,
            cursor_mode_editable,
            persist_mode,
        ) = match req.kind {
            rusty_services::PortalPromptKind::Screenshot { requested_target } => {
                (false, None, requested_target, 0, false, false, 0)
            }
            rusty_services::PortalPromptKind::ScreenCast {
                session_handle,
                allowed_types,
                multiple: _,
                cursor_mode,
                persist_mode,
                restored_target: _,
            } => (
                true,
                Some(session_handle),
                0,
                allowed_types,
                (cursor_mode & rusty_services::SCREENCAST_CURSOR_EMBEDDED) != 0,
                cursor_mode != rusty_services::SCREENCAST_CURSOR_HIDDEN || (allowed_types != 0),
                persist_mode,
            ),
        };
        self.close_launcher();
        self.close_quick_settings();
        self.close_overview();
        self.close_settings();
        self.close_top_bar_menu();
        self.portal_prompt = Some(PortalPromptState {
            request_id: req.request_id,
            app_id: req.app_id,
            is_screencast,
            session_handle,
            requested_target,
            allowed_types,
            include_cursor,
            cursor_mode_editable,
            persist_mode,
            candidates,
            selected,
        });
    }

    fn drain_lock_auth_results(&mut self, serial: Serial) -> bool {
        let mut changed = false;
        for res in self.auth.drain_results() {
            if !self.screen_locked || self.lock_pending_request_id != Some(res.request_id) {
                continue;
            }
            self.lock_auth_checking = false;
            self.lock_pending_request_id = None;
            changed = true;
            match res.outcome {
                rusty_services::AuthOutcome::Verified => {
                    self.set_screen_locked(false, serial);
                    info!("Verified session Lock Screen credentials via PAM helper");
                }
                rusty_services::AuthOutcome::InvalidCredentials => {
                    self.lock_password.clear();
                    self.lock_auth_error = Some("Incorrect password".to_owned());
                    warn!("Lock Screen password check rejected");
                }
                rusty_services::AuthOutcome::Unavailable(msg) => {
                    self.lock_password.clear();
                    self.lock_auth_error = Some(msg);
                    warn!("Lock Screen PAM helper unavailable");
                }
            }
        }
        changed
    }

    fn drain_logind_events(&mut self, serial: Serial) -> bool {
        let mut changed = false;
        for event in self.logind.drain_events() {
            match event {
                rusty_services::LogindEvent::PrepareForSleep {
                    sleep_starting: true,
                } => {
                    if self.user_config.lock_on_sleep && !self.is_session_locked() {
                        self.set_screen_locked(true, serial);
                        info!("Locked desktop session on systemd-logind PrepareForSleep");
                    }
                    self.logind.ack_prepare_for_sleep();
                    changed = true;
                }
                rusty_services::LogindEvent::PrepareForSleep {
                    sleep_starting: false,
                } => {
                    self.telemetry.request_refresh();
                    changed = true;
                }
                rusty_services::LogindEvent::SessionLock => {
                    if !self.is_session_locked() {
                        self.set_screen_locked(true, serial);
                        info!("Locked desktop session on systemd-logind Session.Lock");
                        changed = true;
                    }
                }
                rusty_services::LogindEvent::SessionUnlock => {
                    if self.screen_locked {
                        self.set_screen_locked(false, serial);
                        info!("Unlocked desktop session on systemd-logind Session.Unlock");
                        changed = true;
                    }
                }
            }
        }
        changed
    }

    fn drain_polkit_events(&mut self, serial: Serial) -> bool {
        let mut changed = false;
        for event in self.polkit.drain_events() {
            changed = true;
            match event {
                rusty_services::PolkitEvent::BeginPrompt(req) => {
                    self.window_switcher = None;
                    self.close_launcher();
                    self.close_quick_settings();
                    self.close_overview();
                    self.close_settings();
                    self.close_top_bar_menu();
                    self.polkit_password.clear();
                    self.deactivate_focused_shortcut_inhibitor();
                    if !self.is_session_locked() {
                        let keyboard = self.keyboard.clone();
                        keyboard.set_focus(self, None, serial);
                    }
                    self.polkit_prompt = Some(PolkitPromptState {
                        cookie: req.cookie,
                        action_id: req.action_id,
                        message: req.message,
                        identity_user: req.username,
                        prompt_label: req.prompt_label,
                        echo_on: req.echo_on,
                        checking: false,
                        error_message: None,
                        info_message: None,
                    });
                }
                rusty_services::PolkitEvent::PromptError { cookie, message } => {
                    if let Some(active) = self.polkit_prompt.as_mut()
                        && active.cookie == cookie
                    {
                        active.error_message = Some(message);
                        active.checking = false;
                        self.polkit_password.clear();
                    }
                }
                rusty_services::PolkitEvent::ClosePrompt { cookie } => {
                    if self
                        .polkit_prompt
                        .as_ref()
                        .is_some_and(|active| active.cookie == cookie)
                    {
                        self.polkit_prompt = None;
                        self.polkit_password.clear();
                        if !self.is_session_locked() {
                            self.sync_space_with_wm(serial);
                        }
                    }
                }
            }
        }
        changed
    }
}
