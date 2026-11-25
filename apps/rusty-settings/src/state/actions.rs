//! Row, service, device, system, input, notification, battery, and display actions for `SettingsAppState`.

use rusty_services::{COMMON_TIMEZONES, PowerProfile};

#[allow(clippy::wildcard_imports)]
use super::config_io::*;
#[allow(clippy::wildcard_imports)]
use super::*;

impl SettingsAppState {
    fn try_apply_wifi_service_action(&mut self, action: RowControlId) -> bool {
        match action {
            RowControlId::WifiEnabledToggle => {
                let next = !self.network.wifi_enabled;
                self.network.wifi_enabled = next;
                self.pending_service_cmds
                    .push(ServiceCommand::SetWifiEnabled(next));
            }
            RowControlId::WifiConnectIndex(idx) => {
                let Some(ap) = self.network.wifi_networks.get(idx).cloned() else {
                    return true;
                };
                if ap.connected {
                    self.network_status = format!("Disconnecting from {}...", ap.ssid);
                    self.pending_service_cmds
                        .push(ServiceCommand::DisconnectWifi(ap.ssid));
                } else if ap.secure && !ap.known {
                    self.open_wifi_password_modal(ap.ssid, None);
                } else {
                    self.network_status = format!("Connecting to {}...", ap.ssid);
                    self.pending_service_cmds
                        .push(ServiceCommand::ConnectWifi(ap.ssid));
                }
            }
            RowControlId::WifiInfoIndex(idx) => {
                let Some(ap) = self.network.wifi_networks.get(idx).cloned() else {
                    return true;
                };
                if ap.known || ap.connected {
                    self.open_wifi_info_modal(&ap);
                } else if ap.secure {
                    self.open_wifi_password_modal(ap.ssid, None);
                } else {
                    self.network_status = format!("Connecting to {}...", ap.ssid);
                    self.pending_service_cmds
                        .push(ServiceCommand::ConnectWifi(ap.ssid));
                }
            }
            RowControlId::WifiDisconnectActive => {
                if let Some(ssid) = self.network.wifi_ssid.clone() {
                    self.network_status = format!("Disconnecting from {ssid}...");
                    self.pending_service_cmds
                        .push(ServiceCommand::DisconnectWifi(ssid));
                }
            }
            RowControlId::NetworkRefresh => {
                self.network_status = String::from("Scanning nearby Wi-Fi networks...");
                self.pending_service_cmds.push(ServiceCommand::RescanWifi);
            }
            _ => return false,
        }
        true
    }

    fn try_apply_timedate_service_action(&mut self, action: RowControlId) -> bool {
        match action {
            RowControlId::DateTimeNtpToggle => {
                let next = !self.timedate.ntp_enabled;
                self.timedate.ntp_enabled = next;
                self.timedate_hint = String::from(if next {
                    "Enabling NTP synchronization..."
                } else {
                    "Disabling NTP synchronization..."
                });
                self.pending_service_cmds
                    .push(ServiceCommand::SetNtpEnabled(next));
            }
            RowControlId::DateTimeTimezoneOpen => self.open_timezone_modal(),
            RowControlId::DateTimeTimezoneCycle => {
                let idx = COMMON_TIMEZONES
                    .iter()
                    .position(|z| z.eq_ignore_ascii_case(&self.timedate.timezone))
                    .map_or(0, |i| (i + 1) % COMMON_TIMEZONES.len());
                let next = String::from(COMMON_TIMEZONES[idx]);
                self.timedate.timezone.clone_from(&next);
                self.timedate_hint = format!("Setting timezone to {next}...");
                self.pending_service_cmds
                    .push(ServiceCommand::SetTimezone(next));
            }
            RowControlId::DateTimeManualOpen => {
                if self.timedate.ntp_enabled {
                    self.timedate_hint =
                        String::from("Turn off Automatic Date & Time before setting time manually");
                } else {
                    self.open_manual_datetime_modal();
                }
            }
            _ => return false,
        }
        true
    }

    pub(super) fn try_apply_service_action(&mut self, action: RowControlId) -> bool {
        if self.try_apply_wifi_service_action(action)
            || self.try_apply_timedate_service_action(action)
        {
            return true;
        }
        match action {
            RowControlId::VolumeDown => {
                let target = self.volume_percent.saturating_sub(5);
                self.volume_percent = target;
                self.pending_service_cmds
                    .push(ServiceCommand::SetVolume(target));
            }
            RowControlId::VolumeUp => {
                let target = self.volume_percent.saturating_add(5).min(100);
                self.volume_percent = target;
                self.pending_service_cmds
                    .push(ServiceCommand::SetVolume(target));
            }
            RowControlId::ToggleMute => {
                self.muted = !self.muted;
                self.pending_service_cmds.push(ServiceCommand::ToggleMute);
            }
            RowControlId::BluetoothPoweredToggle => {
                let next = !self.bluetooth.status.powered;
                self.bluetooth.status.powered = next;
                if !next {
                    self.bluetooth.discovering = false;
                }
                self.bluetooth_status_hint = String::from(if next {
                    "Turning on..."
                } else {
                    "Turning off..."
                });
                self.pending_service_cmds
                    .push(ServiceCommand::SetBluetoothPowered(next));
            }
            RowControlId::BluetoothScanToggle => {
                let next = !self.bluetooth.discovering;
                self.bluetooth.discovering = next;
                self.bluetooth_status_hint = String::from(if next {
                    "Scanning for devices..."
                } else {
                    "Stopping scan..."
                });
                self.pending_service_cmds
                    .push(ServiceCommand::SetBluetoothDiscovering(next));
            }
            RowControlId::BluetoothActionIndex(idx) => {
                self.trigger_bluetooth_device_action(idx);
            }
            RowControlId::PowerProfileCycle => {
                let next = match self.power.active_profile {
                    Some(PowerProfile::Balanced) | None => PowerProfile::Performance,
                    Some(PowerProfile::Performance) => PowerProfile::LowPower,
                    Some(PowerProfile::LowPower) => PowerProfile::Balanced,
                };
                self.power.active_profile = Some(next);
                self.pending_service_cmds
                    .push(ServiceCommand::SetPowerProfile(next));
            }
            _ => return false,
        }
        self.mark_dirty(DamageScope::Page);
        true
    }

    pub(crate) fn apply_row_action(&mut self, action: RowControlId) {
        if self.try_apply_service_action(action) {
            return;
        }
        match action {
            RowControlId::ToggleReduceTransparency => {
                self.reduce_transparency = !self.reduce_transparency;
                let v = bool_str(self.reduce_transparency);
                let _ = update_config_entries(&[("ui", "reduce_transparency", v)]);
            }
            RowControlId::ToggleReducedMotion => {
                self.reduced_motion = !self.reduced_motion;
                let v = bool_str(self.reduced_motion);
                let _ = update_config_entries(&[("ui", "reduced_motion", v)]);
            }
            RowControlId::GlassOpacityDown => {
                self.glass_opacity = (self.glass_opacity - 0.1).clamp(0.35, 1.0);
                let v = format!("{:.2}", self.glass_opacity);
                let _ = update_config_entries(&[("ui", "glass_opacity", &v)]);
            }
            RowControlId::GlassOpacityUp => {
                self.glass_opacity = (self.glass_opacity + 0.1).clamp(0.35, 1.0);
                let v = format!("{:.2}", self.glass_opacity);
                let _ = update_config_entries(&[("ui", "glass_opacity", &v)]);
            }
            RowControlId::ToggleWallpaper => match self.custom_wallpaper.as_deref() {
                None | Some("rustyos-dynamic") => {
                    let next = String::from("rustyos-light");
                    self.custom_wallpaper = Some(next.clone());
                    let _ = update_config_entries(&[("ui", "wallpaper", &next)]);
                }
                Some("rustyos-light") => {
                    let next = String::from("rustyos-dark");
                    self.custom_wallpaper = Some(next.clone());
                    let _ = update_config_entries(&[("ui", "wallpaper", &next)]);
                }
                _ => {
                    self.custom_wallpaper = None;
                    let _ = update_config_entries(&[("ui", "wallpaper", "")]);
                }
            },
            other => self.apply_device_action(other),
        }
        self.last_config_mtime = current_config_mtime();
        notify_compositor_reload();
        self.mark_dirty(DamageScope::Page);
    }

    fn apply_device_action(&mut self, action: RowControlId) {
        match action {
            RowControlId::DockPositionSet(idx) => {
                match idx {
                    0 => "left",
                    2 => "right",
                    _ => "bottom",
                }
                .clone_into(&mut self.dock_position);
                let _ = update_config_entries(&[("dock", "position", &self.dock_position)]);
            }
            RowControlId::DockSizeDown => {
                self.dock_icon_size = (self.dock_icon_size - 4).clamp(36, 68);
                let v = self.dock_icon_size.to_string();
                let _ = update_config_entries(&[("dock", "icon_size", &v)]);
            }
            RowControlId::DockSizeUp => {
                self.dock_icon_size = (self.dock_icon_size + 4).clamp(36, 68);
                let v = self.dock_icon_size.to_string();
                let _ = update_config_entries(&[("dock", "icon_size", &v)]);
            }
            RowControlId::DockVisibilityCycle => {
                let next = match self.dock_visibility.as_str() {
                    "always_show" => "smart_hide",
                    "smart_hide" => "always_hide",
                    _ => "always_show",
                };
                next.clone_into(&mut self.dock_visibility);
                self.dock_auto_hide = next == "always_hide";
                let auto_v = bool_str(self.dock_auto_hide);
                let _ = update_config_entries(&[
                    ("dock", "visibility", &self.dock_visibility),
                    ("dock", "auto_hide", auto_v),
                ]);
            }
            RowControlId::DockIndicatorsToggle => {
                self.dock_show_indicators = !self.dock_show_indicators;
                let v = bool_str(self.dock_show_indicators);
                let _ = update_config_entries(&[("dock", "show_indicators", v)]);
            }
            RowControlId::DockRecentToggle => {
                self.dock_show_recent = !self.dock_show_recent;
                let v = bool_str(self.dock_show_recent);
                let _ = update_config_entries(&[("dock", "show_recent", v)]);
            }
            RowControlId::DockMinimizeIntoAppToggle => {
                self.dock_minimize_into_app = !self.dock_minimize_into_app;
                let v = bool_str(self.dock_minimize_into_app);
                let _ = update_config_entries(&[("dock", "minimize_into_app", v)]);
            }
            RowControlId::DockMinimizeEffectCycle => {
                if self.dock_minimize_effect == "genie" {
                    "scale"
                } else {
                    "genie"
                }
                .clone_into(&mut self.dock_minimize_effect);
                let _ = update_config_entries(&[(
                    "dock",
                    "minimize_effect",
                    &self.dock_minimize_effect,
                )]);
            }
            other => self.apply_system_action(other),
        }
    }

    #[allow(clippy::too_many_lines)]
    fn apply_system_action(&mut self, action: RowControlId) {
        match action {
            RowControlId::MenubarWorkspacesToggle => {
                self.menubar_workspaces = !self.menubar_workspaces;
                let v = bool_str(self.menubar_workspaces);
                let _ = update_config_entries(&[("ui", "menubar_show_workspaces", v)]);
            }
            RowControlId::MenubarTrayToggle => {
                self.menubar_tray = !self.menubar_tray;
                let v = bool_str(self.menubar_tray);
                let _ = update_config_entries(&[("ui", "menubar_show_tray", v)]);
            }
            RowControlId::MenubarInputToggle => {
                self.menubar_input = !self.menubar_input;
                let v = bool_str(self.menubar_input);
                let _ = update_config_entries(&[("ui", "menubar_show_input_source", v)]);
            }
            RowControlId::MenubarVolumeToggle => {
                self.menubar_volume = !self.menubar_volume;
                let v = bool_str(self.menubar_volume);
                let _ = update_config_entries(&[
                    ("ui", "menubar_show_volume", v),
                    ("top_bar", "show_sound", v),
                ]);
            }
            RowControlId::MenubarWifiToggle => {
                self.menubar_wifi = !self.menubar_wifi;
                let v = bool_str(self.menubar_wifi);
                let _ = update_config_entries(&[("top_bar", "show_wifi", v)]);
            }
            RowControlId::MenubarBatteryToggle => {
                self.menubar_battery = !self.menubar_battery;
                let v = bool_str(self.menubar_battery);
                let _ = update_config_entries(&[("top_bar", "show_battery", v)]);
            }
            RowControlId::MenubarBatteryPercentageToggle => {
                self.menubar_battery_percentage = !self.menubar_battery_percentage;
                let v = bool_str(self.menubar_battery_percentage);
                let _ = update_config_entries(&[("top_bar", "show_battery_percentage", v)]);
            }
            RowControlId::MenubarBluetoothToggle => {
                self.menubar_bluetooth = !self.menubar_bluetooth;
                let v = bool_str(self.menubar_bluetooth);
                let _ = update_config_entries(&[("top_bar", "show_bluetooth", v)]);
            }
            RowControlId::MenubarAdaptiveContrastToggle => {
                self.menubar_adaptive_contrast = !self.menubar_adaptive_contrast;
                let v = bool_str(self.menubar_adaptive_contrast);
                let _ = update_config_entries(&[("top_bar", "adaptive_contrast", v)]);
            }
            RowControlId::Clock24hToggle => {
                self.clock_24h = !self.clock_24h;
                let v = bool_str(self.clock_24h);
                let _ =
                    update_config_entries(&[("ui", "clock_24h", v), ("top_bar", "clock_24h", v)]);
            }
            RowControlId::ClockLocaleCycle => {
                let next = match self.clock_locale.as_str() {
                    "en" => "ru",
                    "ru" => "uk",
                    _ => "en",
                };
                next.clone_into(&mut self.clock_locale);
                let _ = update_config_entries(&[("top_bar", "clock_locale", next)]);
            }
            RowControlId::UpdatesEnabledToggle => {
                self.updates_enabled = !self.updates_enabled;
                let v = bool_str(self.updates_enabled);
                let _ = update_config_entries(&[("updates", "enabled", v)]);
            }
            RowControlId::UpdatesCheckNow => {
                self.check_updates_now();
            }
            RowControlId::UpdatesInstallNow => {
                self.start_update_transaction();
            }
            RowControlId::DisplayOutputCycle => {
                if !self.display_outputs.is_empty() {
                    self.selected_display_idx =
                        (self.selected_display_idx + 1) % self.display_outputs.len();
                }
            }
            RowControlId::ScaleDown => {
                self.set_display_scale(self.display_scale - 0.25);
            }
            RowControlId::ScaleUp => {
                self.set_display_scale(self.display_scale + 0.25);
            }
            RowControlId::PreferMaxRefreshToggle => {
                self.prefer_max_refresh = !self.prefer_max_refresh;
                let v = bool_str(self.prefer_max_refresh);
                let _ = update_config_entries(&[("display", "prefer_max_refresh", v)]);
            }
            RowControlId::VrrModeCycle => {
                match self.vrr_mode.as_str() {
                    "off" => "fullscreen",
                    "fullscreen" => "always",
                    _ => "off",
                }
                .clone_into(&mut self.vrr_mode);
                let _ = update_config_entries(&[("display", "vrr_mode", &self.vrr_mode)]);
            }
            RowControlId::RotationCycle => {
                match self.display_rotation.as_str() {
                    "0" => "90",
                    "90" => "180",
                    "180" => "270",
                    _ => "0",
                }
                .clone_into(&mut self.display_rotation);
                let section = self.active_display_config_section();
                let _ = update_config_entries(&[(&section, "rotation", &self.display_rotation)]);
            }
            other => self.apply_input_action(other),
        }
    }

    fn apply_input_action(&mut self, action: RowControlId) {
        match action {
            RowControlId::KeyboardLayoutCycle => {
                match self.keyboard_layout.as_str() {
                    "us" => "us,ru",
                    "us,ru" => "us,ua",
                    _ => "us",
                }
                .clone_into(&mut self.keyboard_layout);
                let _ = update_config_entries(&[("keyboard", "layout", &self.keyboard_layout)]);
            }
            RowControlId::KeyboardOptionCycle => {
                match self.keyboard_options.as_str() {
                    "grp:alt_shift_toggle" => "grp:win_space_toggle",
                    "grp:win_space_toggle" => "grp:caps_toggle",
                    _ => "grp:alt_shift_toggle",
                }
                .clone_into(&mut self.keyboard_options);
                let _ = update_config_entries(&[("keyboard", "options", &self.keyboard_options)]);
            }
            RowControlId::RepeatRateDown => {
                self.repeat_rate = (self.repeat_rate - 5).clamp(1, 120);
                let v = self.repeat_rate.to_string();
                let _ = update_config_entries(&[("keyboard", "repeat_rate", &v)]);
            }
            RowControlId::RepeatRateUp => {
                self.repeat_rate = (self.repeat_rate + 5).clamp(1, 120);
                let v = self.repeat_rate.to_string();
                let _ = update_config_entries(&[("keyboard", "repeat_rate", &v)]);
            }
            RowControlId::RepeatDelayDown => {
                self.repeat_delay = (self.repeat_delay - 25).clamp(50, 2000);
                let v = self.repeat_delay.to_string();
                let _ = update_config_entries(&[("keyboard", "repeat_delay", &v)]);
            }
            RowControlId::RepeatDelayUp => {
                self.repeat_delay = (self.repeat_delay + 25).clamp(50, 2000);
                let v = self.repeat_delay.to_string();
                let _ = update_config_entries(&[("keyboard", "repeat_delay", &v)]);
            }
            RowControlId::LockPasswordToggle => {
                self.require_password_on_lock = !self.require_password_on_lock;
                let v = bool_str(self.require_password_on_lock);
                let _ = update_config_entries(&[("security", "require_password_on_lock", v)]);
            }
            RowControlId::LockSleepToggle => {
                self.lock_on_sleep = !self.lock_on_sleep;
                let v = bool_str(self.lock_on_sleep);
                let _ = update_config_entries(&[("security", "lock_on_sleep", v)]);
            }
            RowControlId::PolkitToggle => {
                self.enable_polkit_agent = !self.enable_polkit_agent;
                let v = bool_str(self.enable_polkit_agent);
                let _ = update_config_entries(&[("security", "enable_polkit_agent", v)]);
            }
            RowControlId::CursorThemeCycle => {
                match self.cursor_theme.as_str() {
                    "RustyOS" => "Adwaita",
                    "Adwaita" => "Bibata-Modern-Classic",
                    _ => "RustyOS",
                }
                .clone_into(&mut self.cursor_theme);
                let _ = update_config_entries(&[("ui", "cursor_theme", &self.cursor_theme)]);
            }
            RowControlId::CursorSizeDown => {
                self.cursor_size = self.cursor_size.saturating_sub(4).clamp(16, 64);
                let v = self.cursor_size.to_string();
                let _ = update_config_entries(&[("ui", "cursor_size", &v)]);
            }
            RowControlId::CursorSizeUp => {
                self.cursor_size = (self.cursor_size + 4).clamp(16, 64);
                let v = self.cursor_size.to_string();
                let _ = update_config_entries(&[("ui", "cursor_size", &v)]);
            }
            other => self.apply_notification_action(other),
        }
    }

    fn apply_notification_action(&mut self, action: RowControlId) {
        match action {
            RowControlId::NotifAllowToggle => {
                self.notif_allow = !self.notif_allow;
                let v = bool_str(self.notif_allow);
                let _ = update_config_entries(&[("notifications", "allow", v)]);
            }
            RowControlId::NotifShowPreviewsCycle => {
                let (next_label, toml_val) = match self.notif_show_previews.as_str() {
                    "When Unlocked" => ("Never", "never"),
                    "Never" => ("Always", "always"),
                    _ => ("When Unlocked", "when_unlocked"),
                };
                next_label.clone_into(&mut self.notif_show_previews);
                let _ = update_config_entries(&[("notifications", "show_previews", toml_val)]);
            }
            RowControlId::NotifWhenSleepingToggle => {
                self.notif_when_sleeping = !self.notif_when_sleeping;
                let v = bool_str(self.notif_when_sleeping);
                let _ = update_config_entries(&[("notifications", "when_sleeping", v)]);
            }
            RowControlId::NotifWhenLockedToggle => {
                self.notif_when_locked = !self.notif_when_locked;
                let v = bool_str(self.notif_when_locked);
                let _ = update_config_entries(&[("notifications", "when_locked", v)]);
            }
            RowControlId::NotifMirroringCycle => {
                self.notif_mirroring_off = !self.notif_mirroring_off;
                let v = bool_str(self.notif_mirroring_off);
                let _ = update_config_entries(&[("notifications", "mirroring_off", v)]);
            }
            RowControlId::NotifAppToggle(idx) => {
                if let Some(enabled) = self.notif_app_enabled.get_mut(idx) {
                    *enabled = !*enabled;
                    let muted: Vec<String> = NOTIF_APP_KEYS
                        .iter()
                        .enumerate()
                        .filter(|&(i, _)| !self.notif_app_enabled[i])
                        .map(|(_, k)| (*k).to_owned())
                        .collect();
                    let arr = format_toml_string_array(&muted);
                    let _ = update_config_entries(&[("notifications", "muted_apps", &arr)]);
                }
            }
            RowControlId::PowerDisplaySleepCycle => {
                self.power_display_sleep_minutes = match self.power_display_sleep_minutes {
                    2 => 5,
                    5 => 10,
                    10 => 15,
                    15 => 30,
                    30 => 60,
                    60 => 0,
                    _ => 2,
                };
                let v = self.power_display_sleep_minutes.to_string();
                let _ = update_config_entries(&[("power", "display_sleep_minutes", &v)]);
            }
            RowControlId::PowerAutoLockCycle => {
                self.power_auto_lock_minutes = match self.power_auto_lock_minutes {
                    0 => 1,
                    1 => 5,
                    5 => 15,
                    15 => 30,
                    _ => 0,
                };
                let v = self.power_auto_lock_minutes.to_string();
                let _ = update_config_entries(&[("power", "auto_lock_minutes", &v)]);
            }
            RowControlId::PowerDimOnBatteryToggle => {
                self.power_dim_on_battery = !self.power_dim_on_battery;
                let v = bool_str(self.power_dim_on_battery);
                let _ = update_config_entries(&[("power", "dim_on_battery", v)]);
            }
            RowControlId::ShortcutsResetDefaults => {
                self.shortcuts = std::array::from_fn(|i| SHORTCUT_DEFS[i].default_combo.to_owned());
                let pairs: Vec<(&str, &str, &str)> = SHORTCUT_DEFS
                    .iter()
                    .map(|d| ("shortcuts", d.key, d.default_combo))
                    .collect();
                let _ = update_config_entries(&pairs);
            }
            RowControlId::BluetoothInfoIndex(idx) => self.open_bluetooth_info_modal(idx),
            RowControlId::PowerOptionsOpen | RowControlId::BatteryDetailsOpen => {
                self.open_battery_details_modal();
            }
            RowControlId::AboutMoreInfoOpen => self.open_about_more_info_modal(),
            RowControlId::ShortcutEditIndex(idx) => self.open_shortcut_edit_modal(idx),
            _ => {}
        }
    }

    pub(crate) fn select_battery_tab(&mut self, index: usize) {
        let elapsed = self.epoch.elapsed();
        if self.battery_tab.select(index, elapsed, self.reduced_motion) {
            self.battery_tab_animating = !self.reduced_motion;
            self.mark_dirty(DamageScope::Page);
        }
    }

    pub(crate) fn select_low_power_mode(&mut self, index: usize) {
        let mode_str = match index {
            1 => "battery",
            2 => "always",
            3 => "ac",
            _ => "never",
        };
        self.low_power_dropdown.selected = index.min(3);
        self.low_power_dropdown.open = false;
        self.power_low_power_mode = String::from(mode_str);
        let _ = update_config_entries(&[("power", "low_power_mode", mode_str)]);
        self.last_config_mtime = current_config_mtime();
        notify_compositor_reload();

        let desired = match mode_str {
            "always" => PowerProfile::LowPower,
            "battery" if self.power.status.has_battery && !self.power.status.ac_online => {
                PowerProfile::LowPower
            }
            "ac" if self.power.status.ac_online || !self.power.status.has_battery => {
                PowerProfile::LowPower
            }
            _ => PowerProfile::Balanced,
        };
        if !self.power.available_profiles.is_empty() {
            self.power.active_profile = Some(desired);
            self.pending_service_cmds
                .push(ServiceCommand::SetPowerProfile(desired));
        }
        self.mark_dirty(DamageScope::Page);
    }

    fn trigger_bluetooth_device_action(&mut self, idx: usize) {
        let Some(dev) = self.bluetooth.devices.get(idx).cloned() else {
            return;
        };
        if dev.connected {
            self.bluetooth_status_hint = format!("Disconnecting {}...", dev.name);
            self.pending_service_cmds
                .push(ServiceCommand::DisconnectBluetooth(dev.address));
        } else if dev.paired {
            self.bluetooth_status_hint = format!("Connecting {}...", dev.name);
            self.pending_service_cmds
                .push(ServiceCommand::ConnectBluetooth(dev.address));
        } else {
            self.bluetooth_status_hint = format!("Pairing {}...", dev.name);
            self.pending_service_cmds
                .push(ServiceCommand::PairBluetooth(dev.address));
        }
    }

    fn open_bluetooth_info_modal(&mut self, idx: usize) {
        let Some(dev) = self.bluetooth.devices.get(idx).cloned() else {
            return;
        };
        let mut msg = format!("MAC: {}", dev.address);
        if dev.paired {
            msg.push_str(" · Paired");
        }
        if dev.connected {
            msg.push_str(" · Connected");
        }
        if let Some(pct) = dev.battery_percent {
            use std::fmt::Write as _;
            let _ = write!(msg, " · Battery {pct}%");
        }
        let mut sheet = rusty_ui::DialogSheet::confirm(
            dev.name,
            msg,
            if dev.paired { "Forget" } else { "Pair" },
        )
        .with_destructive(dev.paired);
        sheet.cancel_label = Some(String::from("Close"));
        sheet.width = 400;
        self.active_modal = Some(SettingsModal::BluetoothDeviceInfo {
            address: dev.address,
            paired: dev.paired,
            connected: dev.connected,
            sheet,
        });
        self.mark_dirty(DamageScope::Full);
    }

    fn open_battery_details_modal(&mut self) {
        let cond = self.power.condition.label();
        let cap = self
            .power
            .health_percent
            .map_or_else(|| String::from("N/A"), |p| format!("{p}%"));
        let cycles = self
            .power
            .cycle_count
            .map_or_else(|| String::from("N/A"), |c| c.to_string());
        let msg = format!(
            "Condition: {cond} · Max Capacity: {cap} · Cycles: {cycles} · Sleep {} min",
            self.power_display_sleep_minutes
        );
        let mut sheet = rusty_ui::DialogSheet::confirm("Battery & Power Options", msg, "Done");
        sheet.cancel_label = None;
        sheet.width = 440;
        self.active_modal = Some(SettingsModal::InfoSheet { sheet });
        self.mark_dirty(DamageScope::Full);
    }

    fn open_about_more_info_modal(&mut self) {
        let msg = format!(
            "{} · {} ({}) · {}",
            self.about_info.os_name,
            self.about_info.kernel,
            self.about_info.architecture,
            self.about_info.cpu
        );
        let mut sheet = rusty_ui::DialogSheet::confirm("About This System", msg, "Done");
        sheet.cancel_label = None;
        sheet.width = 460;
        self.active_modal = Some(SettingsModal::InfoSheet { sheet });
        self.mark_dirty(DamageScope::Full);
    }

    fn open_shortcut_edit_modal(&mut self, index: usize) {
        let Some(def) = SHORTCUT_DEFS.get(index) else {
            return;
        };
        let current = self.shortcuts[index].clone();
        let mut sheet = rusty_ui::DialogSheet::prompt(
            format!("Shortcut: {}", def.label),
            format!(
                "Default: {} · Press shortcut or type Super/Alt/Ctrl/Shift + Key",
                def.default_combo
            ),
            "Save",
            "e.g. Super+Space",
            current.clone(),
        );
        sheet.width = 420;
        let mut modal = SettingsModal::ShortcutEdit {
            index,
            conflict_index: None,
            validated_combo: Some(current),
            sheet,
        };
        self.refresh_shortcut_modal_validation(&mut modal);
        self.active_modal = Some(modal);
        self.mark_dirty(DamageScope::Full);
    }

    /// Updates connected Wayland outputs for per-output `Display` settings (`RUSTYOS-170`).
    pub(crate) fn sync_wayland_outputs(&mut self, names: Vec<String>) {
        let mut next = vec![String::from("All Displays")];
        for name in names {
            let trimmed = name.trim();
            if !trimmed.is_empty() && !next.iter().any(|existing| existing == trimmed) {
                next.push(trimmed.to_owned());
            }
        }
        if self.display_outputs != next {
            self.display_outputs = next;
            if self.selected_display_idx >= self.display_outputs.len() {
                self.selected_display_idx = 0;
            }
            if self.current_page == SettingsPage::Display {
                self.mark_dirty(DamageScope::Page);
            }
        }
    }

    #[must_use]
    pub(crate) fn active_display_label(&self) -> String {
        self.display_outputs
            .get(self.selected_display_idx)
            .cloned()
            .unwrap_or_else(|| String::from("All Displays"))
    }

    #[must_use]
    pub(crate) fn active_display_config_section(&self) -> String {
        if self.selected_display_idx == 0 {
            String::from("display")
        } else if let Some(name) = self.display_outputs.get(self.selected_display_idx) {
            format!("display.{name}")
        } else {
            String::from("display")
        }
    }

    pub(crate) fn set_display_scale(&mut self, scale: f64) {
        let quantized = ((scale.clamp(1.0, 3.0) * 4.0).round() / 4.0).clamp(1.0, 3.0);
        if (self.display_scale - quantized).abs() < 1e-4 {
            return;
        }
        self.display_scale = quantized;
        let v = format!("{:.2}", self.display_scale);
        let section = self.active_display_config_section();
        let _ = update_config_entries(&[(&section, "scale", &v)]);
        self.mark_dirty(DamageScope::Page);
    }

    pub(crate) fn set_volume_percent(&mut self, pct: u8) {
        let clamped = pct.min(100);
        if self.volume_percent == clamped {
            return;
        }
        self.volume_percent = clamped;
        self.pending_service_cmds
            .push(ServiceCommand::SetVolume(clamped));
        self.mark_dirty(DamageScope::Page);
    }

    pub(crate) fn set_glass_opacity(&mut self, pct: u8) {
        let clamped = pct.clamp(35, 100);
        let next = f32::from(clamped) / 100.0;
        if (self.glass_opacity - next).abs() < 1e-3 {
            return;
        }
        self.glass_opacity = next;
        let v = format!("{:.2}", self.glass_opacity);
        let _ = update_config_entries(&[("ui", "glass_opacity", &v)]);
        self.last_config_mtime = current_config_mtime();
        notify_compositor_reload();
        self.mark_dirty(DamageScope::Page);
    }
}
