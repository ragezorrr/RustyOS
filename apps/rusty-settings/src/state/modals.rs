//! Modal dialog lifecycle, validation, confirmation, and text input for `SettingsAppState`.

use rusty_services::{
    TimeDateService, is_valid_iana_timezone, match_timezone_suggestions,
    parse_manual_datetime_input,
};

#[allow(clippy::wildcard_imports)]
use super::config_io::*;
#[allow(clippy::wildcard_imports)]
use super::*;

impl SettingsAppState {
    /// Applies a directly captured key combo inside `SettingsModal::ShortcutEdit` (`RUSTYOS-170`).
    pub(crate) fn apply_captured_shortcut(&mut self, combo: &str) -> bool {
        let Some(mut modal) = self.active_modal.take() else {
            return false;
        };
        let is_shortcut = matches!(modal, SettingsModal::ShortcutEdit { .. });
        if is_shortcut {
            if let Some(tf) = &mut modal.sheet_mut().input {
                tf.buffer.set_text(combo);
            }
            self.refresh_shortcut_modal_validation(&mut modal);
            self.mark_dirty(DamageScope::Full);
        }
        self.active_modal = Some(modal);
        is_shortcut
    }

    pub(super) fn refresh_shortcut_modal_validation(&self, modal: &mut SettingsModal) {
        let SettingsModal::ShortcutEdit {
            index,
            conflict_index,
            validated_combo,
            sheet,
        } = modal
        else {
            return;
        };
        let raw = sheet.input.as_ref().map_or("", |tf| tf.buffer.as_str());
        match validate_shortcut_input(raw) {
            Ok(canon) => {
                let conflict = self
                    .shortcuts
                    .iter()
                    .enumerate()
                    .find(|(i, s)| *i != *index && s.eq_ignore_ascii_case(&canon))
                    .map(|(i, _)| i);
                *conflict_index = conflict;
                *validated_combo = Some(canon.clone());
                if let Some(ci) = conflict {
                    sheet.message = format!(
                        "Conflicts with \"{}\" · Save swaps both",
                        SHORTCUT_DEFS[ci].label
                    );
                } else {
                    sheet.message = format!(
                        "Valid: {canon} · Default: {}",
                        SHORTCUT_DEFS[*index].default_combo
                    );
                }
            }
            Err(err) => {
                *conflict_index = None;
                *validated_combo = None;
                sheet.message = err.to_owned();
            }
        }
    }

    pub(crate) fn open_wifi_password_modal(&mut self, ssid: String, error: Option<String>) {
        let message =
            error.unwrap_or_else(|| format!("Enter the WPA2/WPA3 password for \"{ssid}\"."));
        let mut sheet = rusty_ui::DialogSheet::prompt(
            format!("Join \"{ssid}\""),
            message,
            "Join",
            "Password",
            "",
        );
        sheet.width = 430;
        if let Some(tf) = sheet.input.as_mut() {
            tf.masked = true;
            tf.buffer.clear_selection();
        }
        self.active_modal = Some(SettingsModal::WifiPassword {
            ssid,
            show_password: false,
            sheet,
        });
        self.mark_dirty(DamageScope::Full);
    }

    pub(super) fn open_wifi_info_modal(&mut self, ap: &rusty_services::WifiAccessPoint) {
        let state_label = if ap.connected { "Connected" } else { "Saved" };
        let sec_label = if ap.secure { "WPA2/WPA3" } else { "Open" };
        let msg = format!(
            "{state_label} · {sec_label} · Signal {}%",
            ap.signal_percent
        );
        let mut sheet =
            rusty_ui::DialogSheet::confirm(ap.ssid.clone(), msg, "Forget").with_destructive(true);
        sheet.cancel_label = Some(String::from("Close"));
        sheet.width = 400;
        self.active_modal = Some(SettingsModal::WifiNetworkInfo {
            ssid: ap.ssid.clone(),
            connected: ap.connected,
            sheet,
        });
        self.mark_dirty(DamageScope::Full);
    }

    pub(crate) fn open_timezone_modal(&mut self) {
        if self.available_timezones.is_empty() {
            self.available_timezones = TimeDateService::new().list_timezones();
        }
        let current = self.timedate.timezone.clone();
        let mut sheet = rusty_ui::DialogSheet::prompt(
            "Set Time Zone",
            format!("Current: {current} · Enter IANA zone (e.g. Europe/Kyiv)"),
            "Apply",
            "Europe/Kyiv",
            current,
        );
        sheet.width = 450;
        let mut modal = SettingsModal::TimezoneEdit { sheet };
        self.refresh_modal_validation(&mut modal);
        self.active_modal = Some(modal);
        self.mark_dirty(DamageScope::Full);
    }

    pub(crate) fn open_manual_datetime_modal(&mut self) {
        let now_seed = current_local_iso_datetime_seed();
        let mut sheet = rusty_ui::DialogSheet::prompt(
            "Set Date & Time",
            "Format: YYYY-MM-DD HH:MM (local time)",
            "Set",
            "2026-11-23 15:30",
            now_seed,
        );
        sheet.width = 430;
        let mut modal = SettingsModal::ManualDateTime { sheet };
        self.refresh_modal_validation(&mut modal);
        self.active_modal = Some(modal);
        self.mark_dirty(DamageScope::Full);
    }

    fn refresh_modal_validation(&self, modal: &mut SettingsModal) {
        match modal {
            SettingsModal::ShortcutEdit { .. } => self.refresh_shortcut_modal_validation(modal),
            SettingsModal::TimezoneEdit { sheet } => {
                let raw = sheet.input.as_ref().map_or("", |tf| tf.buffer.as_str());
                let trimmed = raw.trim();
                let suggestions = match_timezone_suggestions(trimmed, &self.available_timezones, 3);
                if trimmed.is_empty() {
                    sheet.message = format!("Presets: {}", suggestions.join(", "));
                } else if is_valid_iana_timezone(trimmed)
                    && self.available_timezones.iter().any(|z| z == trimmed)
                {
                    sheet.message = format!("Valid IANA timezone: {trimmed}");
                } else if !suggestions.is_empty() {
                    sheet.message = format!("Matches: {}", suggestions.join(", "));
                } else {
                    sheet.message = String::from("No matching IANA timezone (e.g. Europe/Kyiv)");
                }
            }
            SettingsModal::ManualDateTime { sheet } => {
                let raw = sheet.input.as_ref().map_or("", |tf| tf.buffer.as_str());
                sheet.message = match parse_manual_datetime_input(raw) {
                    Ok(canon) => format!("Valid local time: {canon}"),
                    Err(err) => err,
                };
            }
            SettingsModal::BluetoothDeviceInfo { .. }
            | SettingsModal::WifiPassword { .. }
            | SettingsModal::WifiNetworkInfo { .. }
            | SettingsModal::InfoSheet { .. } => {}
        }
    }

    pub(crate) fn toggle_wifi_password_visibility(&mut self) {
        if let Some(SettingsModal::WifiPassword {
            show_password,
            sheet,
            ..
        }) = &mut self.active_modal
        {
            *show_password = !*show_password;
            if let Some(tf) = sheet.input.as_mut() {
                tf.masked = !*show_password;
            }
            self.mark_dirty(DamageScope::Full);
        }
    }

    pub(crate) fn dismiss_active_modal(&mut self) {
        if self.active_modal.take().is_some() {
            self.mark_dirty(DamageScope::Full);
        }
    }

    pub(crate) fn confirm_active_modal(&mut self) {
        let Some(modal) = self.active_modal.take() else {
            return;
        };
        match modal {
            SettingsModal::BluetoothDeviceInfo {
                address,
                paired,
                connected,
                ..
            } => {
                let _ = connected;
                let cmd = if paired {
                    ServiceCommand::RemoveBluetooth(address)
                } else {
                    ServiceCommand::PairBluetooth(address)
                };
                self.pending_service_cmds.push(cmd);
            }
            SettingsModal::WifiPassword {
                ssid,
                show_password,
                mut sheet,
            } => {
                let password = sheet
                    .input
                    .as_ref()
                    .map_or_default(|tf| tf.buffer.as_str().to_owned());
                if password.is_empty() {
                    sheet.message = String::from("Please enter the Wi-Fi password.");
                    self.active_modal = Some(SettingsModal::WifiPassword {
                        ssid,
                        show_password,
                        sheet,
                    });
                    self.mark_dirty(DamageScope::Full);
                    return;
                }
                self.network_status = format!("Connecting to {ssid}...");
                self.pending_service_cmds
                    .push(ServiceCommand::ConnectWifiWithPassword { ssid, password });
            }
            SettingsModal::WifiNetworkInfo {
                ssid, connected, ..
            } => {
                let _ = connected;
                self.network_status = format!("Forgetting {ssid}...");
                self.pending_service_cmds
                    .push(ServiceCommand::ForgetWifi(ssid));
            }
            SettingsModal::InfoSheet { .. } => {}
            SettingsModal::ShortcutEdit {
                index,
                conflict_index,
                validated_combo,
                sheet,
            } => {
                let Some(canon) = validated_combo else {
                    let mut restored = SettingsModal::ShortcutEdit {
                        index,
                        conflict_index,
                        validated_combo: None,
                        sheet,
                    };
                    self.refresh_shortcut_modal_validation(&mut restored);
                    self.active_modal = Some(restored);
                    self.mark_dirty(DamageScope::Full);
                    return;
                };
                let old_combo = self.shortcuts[index].clone();
                self.shortcuts[index].clone_from(&canon);
                if let Some(ci) = conflict_index {
                    self.shortcuts[ci].clone_from(&old_combo);
                    let _ = update_config_entries(&[
                        ("shortcuts", SHORTCUT_DEFS[index].key, &canon),
                        ("shortcuts", SHORTCUT_DEFS[ci].key, &old_combo),
                    ]);
                } else {
                    let _ =
                        update_config_entries(&[("shortcuts", SHORTCUT_DEFS[index].key, &canon)]);
                }
                self.last_config_mtime = current_config_mtime();
                notify_compositor_reload();
            }
            SettingsModal::TimezoneEdit { sheet } => self.confirm_timezone_modal(sheet),
            SettingsModal::ManualDateTime { sheet } => self.confirm_manual_datetime_modal(sheet),
        }
        self.mark_dirty(DamageScope::Full);
    }

    fn confirm_timezone_modal(&mut self, mut sheet: rusty_ui::DialogSheet) {
        let raw = sheet
            .input
            .as_ref()
            .map_or_default(|tf| tf.buffer.as_str().trim().to_owned());
        let resolved = if is_valid_iana_timezone(&raw)
            && (self.available_timezones.is_empty()
                || self.available_timezones.iter().any(|z| z == &raw))
        {
            Some(raw)
        } else {
            let matches = match_timezone_suggestions(&raw, &self.available_timezones, 2);
            (matches.len() == 1).then(|| matches[0].clone())
        };
        let Some(tz) = resolved else {
            sheet.message = String::from("Enter an exact IANA timezone (e.g. Europe/Kyiv)");
            self.active_modal = Some(SettingsModal::TimezoneEdit { sheet });
            return;
        };
        self.timedate.timezone.clone_from(&tz);
        self.timedate_hint = format!("Setting timezone to {tz}...");
        self.pending_service_cmds
            .push(ServiceCommand::SetTimezone(tz));
    }

    fn confirm_manual_datetime_modal(&mut self, mut sheet: rusty_ui::DialogSheet) {
        let raw = sheet
            .input
            .as_ref()
            .map_or_default(|tf| tf.buffer.as_str().trim().to_owned());
        match parse_manual_datetime_input(&raw) {
            Ok(canonical) => {
                self.timedate_hint = format!("Setting time to {canonical}...");
                self.pending_service_cmds
                    .push(ServiceCommand::SetManualDateTime(canonical));
            }
            Err(err) => {
                sheet.message = err;
                self.active_modal = Some(SettingsModal::ManualDateTime { sheet });
            }
        }
    }

    pub(crate) fn handle_modal_text_key(
        &mut self,
        key: rusty_ui::TextInputKey,
        mods: rusty_ui::InputModifiers,
        fonts: &mut rusty_ui::UiFontEngine,
        clipboard: &mut dyn rusty_ui::ClipboardSink,
    ) -> bool {
        if key == rusty_ui::TextInputKey::Escape {
            self.dismiss_active_modal();
            return true;
        }
        let Some(mut modal) = self.active_modal.take() else {
            return false;
        };
        let elapsed = self.epoch.elapsed();
        let ev = if let Some(tf) = modal.sheet_mut().input.as_mut() {
            tf.handle_key(key, mods, elapsed, fonts, clipboard)
        } else if key == rusty_ui::TextInputKey::Enter {
            rusty_ui::TextInputEvent::Submitted
        } else {
            rusty_ui::TextInputEvent::Ignored
        };

        match ev {
            rusty_ui::TextInputEvent::Submitted => {
                self.active_modal = Some(modal);
                self.confirm_active_modal();
                return true;
            }
            rusty_ui::TextInputEvent::Cancelled => {
                self.mark_dirty(DamageScope::Full);
                return true;
            }
            rusty_ui::TextInputEvent::Changed => {
                self.refresh_modal_validation(&mut modal);
            }
            rusty_ui::TextInputEvent::CaretMoved | rusty_ui::TextInputEvent::Ignored => {}
        }
        self.active_modal = Some(modal);
        self.mark_dirty(DamageScope::Full);
        true
    }

    #[allow(dead_code)]
    pub(crate) fn handle_modal_paste(
        &mut self,
        text: &str,
        fonts: &mut rusty_ui::UiFontEngine,
    ) -> bool {
        if text.is_empty() {
            return false;
        }
        let Some(mut modal) = self.active_modal.take() else {
            return false;
        };
        if let Some(tf) = modal.sheet_mut().input.as_mut() {
            let mut mem = rusty_ui::MemoryClipboard::new();
            rusty_ui::ClipboardSink::set_text(&mut mem, text);
            let _ = tf.handle_key(
                rusty_ui::TextInputKey::Char('v'),
                rusty_ui::InputModifiers {
                    ctrl: true,
                    shift: false,
                    alt: false,
                },
                self.epoch.elapsed(),
                fonts,
                &mut mem,
            );
            self.refresh_modal_validation(&mut modal);
            self.active_modal = Some(modal);
            self.mark_dirty(DamageScope::Full);
            return true;
        }
        self.active_modal = Some(modal);
        false
    }
}
