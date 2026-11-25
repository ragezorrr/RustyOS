//! TOML config persistence, IPC live-reload, shortcut validation, and hardware detection helpers.

use std::env;
use std::fs;
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant, SystemTime};

use rusty_services::{
    AudioService, BluetoothService, NetworkService, PowerService, TimeDateService,
};

#[allow(clippy::wildcard_imports)]
use super::*;

impl SettingsAppState {
    pub(crate) fn poll_external_config_if_due(&mut self, now: Instant) -> bool {
        if let Some(last) = self.last_config_poll
            && now.saturating_duration_since(last) < Duration::from_millis(200)
        {
            return false;
        }
        self.last_config_poll = Some(now);
        self.poll_external_config()
    }

    pub(crate) fn poll_external_config(&mut self) -> bool {
        if self.theme_locked {
            return false;
        }
        let mtime = current_config_mtime();
        if mtime.is_some() && mtime != self.last_config_mtime {
            self.last_config_mtime = mtime;
            self.load_from_disk();
            self.mark_dirty(DamageScope::Full);
            return true;
        }
        false
    }

    pub(crate) fn load_from_disk(&mut self) {
        let Some(path) = config_toml_path() else {
            return;
        };
        let Ok(content) = fs::read_to_string(&path) else {
            return;
        };
        let mut section = String::new();
        let mut auto_mode_seen = false;
        let mut explicit_dock_visibility: Option<String> = None;
        for raw in content.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                line[1..line.len() - 1].trim().clone_into(&mut section);
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let sec = section.trim();
            let key = k.trim();
            let val = unquote_toml(v.trim());
            if sec == "ui" && key == "theme_mode" && val.eq_ignore_ascii_case("auto") {
                auto_mode_seen = true;
            }
            if sec == "dock"
                && key == "visibility"
                && let Some(norm) = normalize_dock_visibility(val)
            {
                explicit_dock_visibility = Some(norm.to_owned());
            }
            self.apply_toml_kv(sec, key, val);
        }
        if auto_mode_seen {
            self.theme_choice = ThemeChoice::Auto;
        }
        if let Some(vis) = explicit_dock_visibility {
            self.dock_auto_hide = vis == "always_hide";
            self.dock_visibility = vis;
        }
    }

    pub(super) fn apply_toml_kv(&mut self, section: &str, key: &str, val: &str) {
        match (section, key) {
            ("ui", "theme") => {
                let dark = val.eq_ignore_ascii_case("dark");
                self.resolved_dark = dark;
                self.theme_choice = if dark {
                    ThemeChoice::Dark
                } else {
                    ThemeChoice::Light
                };
            }
            ("ui", "accent_index") => {
                if let Ok(idx) = val.parse::<usize>() {
                    self.accent_index = idx.min(ACCENT_COLORS.len() - 1);
                }
            }
            ("ui", "reduce_transparency") => self.reduce_transparency = parse_bool(val),
            ("ui", "reduced_motion") => self.reduced_motion = parse_bool(val),
            ("ui", "glass_opacity") => {
                if let Ok(v) = val.parse::<f32>() {
                    self.glass_opacity = v.clamp(0.35, 1.0);
                }
            }
            ("ui", "wallpaper") => {
                self.custom_wallpaper = (!val.is_empty()).then(|| val.to_owned());
            }
            ("ui", "menubar_show_workspaces") => self.menubar_workspaces = parse_bool(val),
            ("ui", "menubar_show_tray") => self.menubar_tray = parse_bool(val),
            ("ui", "menubar_show_input_source") => self.menubar_input = parse_bool(val),
            ("ui", "menubar_show_volume") | ("top_bar", "show_sound") => {
                self.menubar_volume = parse_bool(val);
            }
            ("top_bar", "show_wifi") => self.menubar_wifi = parse_bool(val),
            ("top_bar", "show_battery") => self.menubar_battery = parse_bool(val),
            ("top_bar", "show_battery_percentage") => {
                self.menubar_battery_percentage = parse_bool(val);
            }
            ("top_bar", "show_bluetooth") => self.menubar_bluetooth = parse_bool(val),
            ("top_bar", "adaptive_contrast") => self.menubar_adaptive_contrast = parse_bool(val),
            ("ui" | "top_bar", "clock_24h") => self.clock_24h = parse_bool(val),
            ("top_bar", "clock_locale") => {
                let norm = val.trim().trim_matches('"').to_ascii_lowercase();
                if matches!(norm.as_str(), "en" | "ru" | "uk") {
                    self.clock_locale = norm;
                }
            }
            ("ui", "cursor_theme") if !val.is_empty() => {
                val.trim()
                    .trim_matches('"')
                    .clone_into(&mut self.cursor_theme);
            }
            ("ui", "cursor_size") => {
                if let Ok(v) = val.trim().parse::<u32>() {
                    self.cursor_size = v.clamp(16, 64);
                }
            }
            ("updates", "enabled") => self.updates_enabled = parse_bool(val),
            ("notifications", "allow") => self.notif_allow = parse_bool(val),
            ("notifications", "show_previews") => {
                let norm = match val.trim().to_ascii_lowercase().as_str() {
                    "always" => "Always",
                    "never" | "off" => "Never",
                    _ => "When Unlocked",
                };
                norm.clone_into(&mut self.notif_show_previews);
            }
            ("notifications", "when_sleeping") => self.notif_when_sleeping = parse_bool(val),
            ("notifications", "when_locked") => self.notif_when_locked = parse_bool(val),
            ("notifications", "mirroring_off") => self.notif_mirroring_off = parse_bool(val),
            ("notifications", "muted_apps") => {
                let inner = val.trim().trim_start_matches('[').trim_end_matches(']');
                let muted: Vec<String> = inner
                    .split(',')
                    .map(|p| p.trim().trim_matches('"').to_ascii_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect();
                for (i, key) in NOTIF_APP_KEYS.iter().enumerate() {
                    self.notif_app_enabled[i] = !muted.iter().any(|m| m == key);
                }
            }
            _ => self.apply_toml_device_kv(section, key, val),
        }
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn apply_toml_device_kv(&mut self, section: &str, key: &str, val: &str) {
        match (section, key) {
            ("dock", "icon_size") => {
                if let Ok(v) = val.parse::<i32>() {
                    self.dock_icon_size = v.clamp(36, 68);
                }
            }
            ("dock", "position") => self.dock_position = val.to_ascii_lowercase(),
            ("dock", "minimize_effect") => self.dock_minimize_effect = val.to_ascii_lowercase(),
            ("dock", "minimize_into_app") => self.dock_minimize_into_app = parse_bool(val),
            ("dock", "visibility") => {
                if let Some(norm) = normalize_dock_visibility(val) {
                    norm.clone_into(&mut self.dock_visibility);
                    self.dock_auto_hide = norm == "always_hide";
                }
            }
            ("dock", "auto_hide") => {
                self.dock_auto_hide = parse_bool(val);
                let vis = if self.dock_auto_hide {
                    "always_hide"
                } else {
                    "always_show"
                };
                vis.clone_into(&mut self.dock_visibility);
            }
            ("dock", "show_indicators") => self.dock_show_indicators = parse_bool(val),
            ("dock", "show_recent") => self.dock_show_recent = parse_bool(val),
            ("dock", "magnification_enabled") => self.dock_magnification_enabled = parse_bool(val),
            ("dock", "magnification_max") => {
                if let Ok(v) = val.parse::<f64>()
                    && v.is_finite()
                {
                    self.dock_magnification_max = v.clamp(1.0, 2.0);
                }
            }
            ("dock", "animation_speed") => {
                if let Ok(v) = val.parse::<f64>()
                    && v.is_finite()
                {
                    self.dock_animation_speed = v.clamp(0.25, 3.0);
                }
            }
            ("dock", "show_tooltips") => self.dock_show_tooltips = parse_bool(val),
            ("display", "scale") => {
                if let Ok(v) = val.parse::<f64>() {
                    self.display_scale = v.clamp(1.0, 3.0);
                }
            }
            ("display", "prefer_max_refresh") => self.prefer_max_refresh = parse_bool(val),
            ("display", "vrr_mode") => self.vrr_mode = val.to_ascii_lowercase(),
            ("display", "rotation") => val.clone_into(&mut self.display_rotation),
            ("keyboard", "layout") => val.clone_into(&mut self.keyboard_layout),
            ("keyboard", "options") => val.clone_into(&mut self.keyboard_options),
            ("keyboard", "repeat_rate") => {
                if let Ok(v) = val.parse::<i32>() {
                    self.repeat_rate = v.clamp(1, 120);
                }
            }
            ("keyboard", "repeat_delay") => {
                if let Ok(v) = val.parse::<i32>() {
                    self.repeat_delay = v.clamp(50, 2000);
                }
            }
            ("security", "require_password_on_lock") => {
                self.require_password_on_lock = parse_bool(val);
            }
            ("security", "lock_on_sleep") => self.lock_on_sleep = parse_bool(val),
            ("security", "enable_polkit_agent") => self.enable_polkit_agent = parse_bool(val),
            ("power", "low_power_mode") => {
                let (idx, norm) = match val.trim().to_ascii_lowercase().as_str() {
                    "battery" | "only_on_battery" | "on_battery" => (1, "battery"),
                    "always" | "on" => (2, "always"),
                    "ac" | "adapter" | "only_on_ac" | "only_on_adapter" => (3, "ac"),
                    _ => (0, "never"),
                };
                self.power_low_power_mode = String::from(norm);
                self.low_power_dropdown.selected = idx;
            }
            ("power", "display_sleep_minutes") => {
                if let Ok(v) = val.parse::<u32>() {
                    self.power_display_sleep_minutes = v.min(180);
                }
            }
            ("power", "auto_lock_minutes") => {
                if let Ok(v) = val.parse::<u32>() {
                    self.power_auto_lock_minutes = v.min(120);
                }
            }
            ("power", "dim_on_battery") => self.power_dim_on_battery = parse_bool(val),
            ("network", "proxy_mode") => {
                let norm = match val.trim().to_ascii_lowercase().as_str() {
                    "manual" => "manual",
                    "auto" => "auto",
                    _ => "none",
                };
                self.network_proxy_mode = String::from(norm);
            }
            ("shortcuts", key) => {
                if let Some(idx) = SHORTCUT_DEFS.iter().position(|d| d.key == key)
                    && let Ok(canon) = validate_shortcut_input(val)
                {
                    self.shortcuts[idx] = canon;
                }
            }
            _ => {}
        }
    }
}

pub(super) const fn bool_str(b: bool) -> &'static str {
    if b { "true" } else { "false" }
}

pub(super) fn parse_bool(s: &str) -> bool {
    s.eq_ignore_ascii_case("true") || s == "1"
}

pub(super) fn normalize_dock_visibility(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "always_show" | "always-show" | "show" | "never" => Some("always_show"),
        "smart_hide" | "smart-hide" | "smart" | "intellihide" | "intell_hide"
        | "when_obstructed" | "when-obstructed" | "dodge" => Some("smart_hide"),
        "always_hide" | "always-hide" | "hide" | "always" => Some("always_hide"),
        _ => None,
    }
}

pub(crate) fn dock_visibility_label(raw: &str) -> &'static str {
    match normalize_dock_visibility(raw).unwrap_or("always_show") {
        "smart_hide" => "Smart Hide",
        "always_hide" => "Always Hide",
        _ => "Always Show",
    }
}

pub(super) fn unquote_toml(s: &str) -> &str {
    let trimmed = s.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    }
}

pub(super) fn config_toml_path() -> Option<PathBuf> {
    if let Ok(xdg) = env::var("XDG_CONFIG_HOME")
        && !xdg.trim().is_empty()
    {
        return Some(PathBuf::from(xdg).join("rustyos").join("config.toml"));
    }
    env::var("HOME")
        .ok()
        .filter(|h| !h.trim().is_empty())
        .map(|h| PathBuf::from(h).join(".config/rustyos/config.toml"))
}

pub(super) fn format_toml_value(val: &str) -> String {
    let trimmed = val.trim();
    if matches!(trimmed, "true" | "false")
        || trimmed.parse::<f64>().is_ok()
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
    {
        trimmed.to_owned()
    } else {
        let escaped = val
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t");
        format!("\"{escaped}\"")
    }
}

pub(super) fn format_toml_string_array(items: &[String]) -> String {
    let quoted: Vec<String> = items
        .iter()
        .map(|s| {
            let esc = s
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
                .replace('\r', "\\r");
            format!("\"{esc}\"")
        })
        .collect();
    format!("[{}]", quoted.join(", "))
}

pub(super) fn patch_toml_text(original: &str, section: &str, key: &str, raw_val: &str) -> String {
    let mut lines: Vec<String> = original.lines().map(ToOwned::to_owned).collect();
    let formatted_line = format!("{key} = {}", format_toml_value(raw_val));
    let mut in_section = false;
    let mut section_found = false;
    let mut section_end_idx = lines.len();

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let hdr = trimmed[1..trimmed.len() - 1].trim();
            if in_section {
                section_end_idx = i;
                break;
            }
            if hdr == section {
                in_section = true;
                section_found = true;
            }
            continue;
        }
        if in_section
            && let Some((lhs, _)) = trimmed.split_once('=')
            && lhs.trim() == key
        {
            if raw_val.is_empty() && key == "wallpaper" {
                lines.remove(i);
            } else {
                lines[i] = formatted_line;
            }
            return lines.join("\n") + "\n";
        }
    }

    if raw_val.is_empty() && key == "wallpaper" {
        return lines.join("\n") + "\n";
    }
    if section_found {
        lines.insert(section_end_idx, formatted_line);
    } else {
        if !lines.is_empty() && !lines.last().is_some_and(String::is_empty) {
            lines.push(String::new());
        }
        lines.push(format!("[{section}]"));
        lines.push(formatted_line);
    }
    lines.join("\n") + "\n"
}

pub(super) fn update_config_entries(updates: &[(&str, &str, &str)]) -> std::io::Result<()> {
    let Some(path) = config_toml_path() else {
        return Ok(());
    };
    rusty_services::with_config_file_lock(&path, || {
        let mut text = fs::read_to_string(&path)
            .unwrap_or_else(|_| String::from("# RustyOS compositor configuration\n\n[ui]\n"));
        for &(section, key, val) in updates {
            text = patch_toml_text(&text, section, key, val);
        }
        rusty_services::write_file_atomic(&path, text.as_bytes())
    })
}

pub(super) fn current_config_mtime() -> Option<SystemTime> {
    let path = config_toml_path()?;
    fs::metadata(path).ok()?.modified().ok()
}

pub(super) fn system_auto_prefers_dark() -> bool {
    let Ok(out) = Command::new("date").arg("+%H").output() else {
        return false;
    };
    if !out.status.success() {
        return false;
    }
    let hour = String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<u8>()
        .unwrap_or(12);
    !(7..19).contains(&hour)
}

pub(super) fn resolve_ipc_socket_path() -> Option<PathBuf> {
    if let Ok(sock_path) = env::var("RUSTYOS_IPC_SOCKET")
        && !sock_path.trim().is_empty()
    {
        return Some(PathBuf::from(sock_path));
    }
    let runtime_dir = env::var_os("XDG_RUNTIME_DIR")?;
    let wayland_display = env::var("WAYLAND_DISPLAY").ok()?;
    let candidate = PathBuf::from(runtime_dir).join(format!("rustyos-ipc-{wayland_display}.sock"));
    candidate.exists().then_some(candidate)
}

pub(super) fn notify_compositor_reload() {
    let Some(sock_path) = resolve_ipc_socket_path() else {
        return;
    };
    std::thread::spawn(move || {
        let Ok(mut stream) = UnixStream::connect(sock_path) else {
            return;
        };
        let timeout = Some(Duration::from_millis(120));
        let _ = stream.set_read_timeout(timeout);
        let _ = stream.set_write_timeout(timeout);
        let _ = stream.write_all(b"v1|RELOAD_CONFIG\n");
        let _ = stream.flush();
    });
}

#[allow(clippy::cast_possible_truncation)]
pub(super) fn step_scroll_axis(
    pos_i: &mut i32,
    pos_f: &mut f64,
    target: f64,
    alpha: f64,
    scope: DamageScope,
    changed_scope: &mut DamageScope,
) -> bool {
    let diff = target - *pos_f;
    if diff.abs() < 0.25 {
        *pos_f = target;
        let next_i = target.round() as i32;
        if next_i != *pos_i {
            *pos_i = next_i;
            *changed_scope = changed_scope.merge(scope);
        }
        return true;
    }
    *pos_f += diff * alpha;
    if (target - *pos_f).abs() < 0.25 {
        *pos_f = target;
        let next_i = target.round() as i32;
        if next_i != *pos_i {
            *pos_i = next_i;
            *changed_scope = changed_scope.merge(scope);
        }
        return true;
    }
    let next_i = pos_f.round() as i32;
    if next_i != *pos_i {
        *pos_i = next_i;
        *changed_scope = changed_scope.merge(scope);
    }
    false
}

pub(super) fn current_local_iso_datetime_seed() -> String {
    Command::new("date")
        .arg("+%Y-%m-%d %H:%M")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map_or_default(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
}

/// Queries live NetworkManager, BlueZ, power, timedated, and audio status (`RUSTYOS-112`, `RUSTYOS-131`, `RUSTYOS-160`).
#[must_use]
pub(crate) fn query_service_snapshot() -> ServiceSnapshot {
    let network = NetworkService::new().query_status();
    let bluetooth = BluetoothService::new().query_snapshot();
    let power = PowerService::new().query_details();
    let timedate = TimeDateService::new().query_status();
    let audio = AudioService::new().query_status();
    ServiceSnapshot {
        network,
        bluetooth,
        power,
        timedate,
        timedate_error: None,
        volume_percent: audio.volume_percent.clamp(0, 100),
        muted: audio.muted,
        wifi_action_result: None,
    }
}

/// Collapses redundant queued service commands before worker execution (`RUSTYOS-112`, `RUSTYOS-159`, `RUSTYOS-160`).
#[must_use]
pub(crate) fn coalesce_service_commands(cmds: Vec<ServiceCommand>) -> Vec<ServiceCommand> {
    let mut had_refresh = false;
    let mut rescan_wifi = false;
    let mut set_volume = None;
    let mut toggle_mute = false;
    let mut set_wifi = None;
    let mut wifi_op: Option<ServiceCommand> = None;
    let mut bt_power = None;
    let mut bt_discover = None;
    let mut bt_device_op: Option<ServiceCommand> = None;
    let mut power_profile = None;
    let mut set_ntp = None;
    let mut set_tz = None;
    let mut set_manual_time = None;

    for cmd in cmds {
        match cmd {
            ServiceCommand::Refresh => had_refresh = true,
            ServiceCommand::RescanWifi => rescan_wifi = true,
            ServiceCommand::SetVolume(v) => set_volume = Some(v),
            ServiceCommand::ToggleMute => toggle_mute = !toggle_mute,
            ServiceCommand::SetWifiEnabled(en) => {
                set_wifi = Some(en);
                if !en {
                    wifi_op = None;
                    rescan_wifi = false;
                }
            }
            op @ (ServiceCommand::ConnectWifi(_)
            | ServiceCommand::ConnectWifiWithPassword { .. }
            | ServiceCommand::DisconnectWifi(_)
            | ServiceCommand::ForgetWifi(_)) => wifi_op = Some(op),
            ServiceCommand::SetBluetoothPowered(en) => {
                bt_power = Some(en);
                if !en {
                    bt_discover = None;
                    bt_device_op = None;
                }
            }
            ServiceCommand::SetBluetoothDiscovering(en) => bt_discover = Some(en),
            op @ (ServiceCommand::ConnectBluetooth(_)
            | ServiceCommand::DisconnectBluetooth(_)
            | ServiceCommand::PairBluetooth(_)
            | ServiceCommand::RemoveBluetooth(_)) => bt_device_op = Some(op),
            ServiceCommand::SetPowerProfile(p) => power_profile = Some(p),
            ServiceCommand::SetNtpEnabled(en) => {
                set_ntp = Some(en);
                if en {
                    set_manual_time = None;
                }
            }
            ServiceCommand::SetTimezone(tz) => set_tz = Some(tz),
            ServiceCommand::SetManualDateTime(dt) => set_manual_time = Some(dt),
        }
    }

    let mut out = Vec::new();
    if let Some(en) = set_wifi {
        out.push(ServiceCommand::SetWifiEnabled(en));
    }
    if let Some(op) = wifi_op {
        out.push(op);
    }
    if rescan_wifi {
        out.push(ServiceCommand::RescanWifi);
    }
    if let Some(en) = bt_power {
        out.push(ServiceCommand::SetBluetoothPowered(en));
    }
    if let Some(op) = bt_device_op {
        out.push(op);
    }
    if let Some(en) = bt_discover {
        out.push(ServiceCommand::SetBluetoothDiscovering(en));
    }
    if let Some(p) = power_profile {
        out.push(ServiceCommand::SetPowerProfile(p));
    }
    if let Some(en) = set_ntp {
        out.push(ServiceCommand::SetNtpEnabled(en));
    }
    if let Some(tz) = set_tz {
        out.push(ServiceCommand::SetTimezone(tz));
    }
    if let Some(dt) = set_manual_time {
        out.push(ServiceCommand::SetManualDateTime(dt));
    }
    if let Some(v) = set_volume {
        out.push(ServiceCommand::SetVolume(v));
    }
    if toggle_mute {
        out.push(ServiceCommand::ToggleMute);
    }
    if out.is_empty() && had_refresh {
        out.push(ServiceCommand::Refresh);
    }
    out
}

/// Executes a coalesced batch of service commands and queries fresh snapshot (`RUSTYOS-112`, `RUSTYOS-131`, `RUSTYOS-159`, `RUSTYOS-160`).
#[must_use]
pub(crate) fn execute_coalesced_service_batch(cmds: Vec<ServiceCommand>) -> ServiceSnapshot {
    let bt = BluetoothService::new();
    let net = NetworkService::new();
    let td = TimeDateService::new();
    let mut wifi_action_result: Option<WifiActionOutcome> = None;
    let mut timedate_error: Option<String> = None;
    let mut rescanned_networks: Option<Vec<rusty_services::WifiAccessPoint>> = None;
    for cmd in coalesce_service_commands(cmds) {
        match cmd {
            ServiceCommand::Refresh => {}
            ServiceCommand::RescanWifi => {
                rescanned_networks = Some(net.rescan_wifi_networks());
            }
            ServiceCommand::SetVolume(v) => {
                let _ = AudioService::new().set_volume(v);
            }
            ServiceCommand::ToggleMute => {
                let _ = AudioService::new().toggle_mute();
            }
            ServiceCommand::SetWifiEnabled(en) => {
                let _ = net.set_wifi_enabled(en);
            }
            ServiceCommand::ConnectWifi(ssid) => {
                let result = net.connect_wifi_with_password(&ssid, None);
                wifi_action_result = Some(WifiActionOutcome {
                    ssid,
                    kind: WifiActionKind::Connect,
                    result,
                });
            }
            ServiceCommand::ConnectWifiWithPassword { ssid, password } => {
                let result = net.connect_wifi_with_password(&ssid, Some(&password));
                wifi_action_result = Some(WifiActionOutcome {
                    ssid,
                    kind: WifiActionKind::Connect,
                    result,
                });
            }
            ServiceCommand::DisconnectWifi(ssid) => {
                let result = net.disconnect_wifi(&ssid);
                wifi_action_result = Some(WifiActionOutcome {
                    ssid,
                    kind: WifiActionKind::Disconnect,
                    result,
                });
            }
            ServiceCommand::ForgetWifi(ssid) => {
                let result = net.forget_wifi(&ssid);
                wifi_action_result = Some(WifiActionOutcome {
                    ssid,
                    kind: WifiActionKind::Forget,
                    result,
                });
            }
            ServiceCommand::SetBluetoothPowered(en) => {
                let _ = bt.set_powered(en);
            }
            ServiceCommand::SetBluetoothDiscovering(en) => {
                let _ = bt.set_discovering(en);
            }
            ServiceCommand::ConnectBluetooth(addr) => {
                let _ = bt.connect_device(&addr);
            }
            ServiceCommand::DisconnectBluetooth(addr) => {
                let _ = bt.disconnect_device(&addr);
            }
            ServiceCommand::PairBluetooth(addr) => {
                let _ = bt.pair_device(&addr);
            }
            ServiceCommand::RemoveBluetooth(addr) => {
                let _ = bt.remove_device(&addr);
            }
            ServiceCommand::SetPowerProfile(profile) => {
                let _ = PowerService::new().set_power_profile(profile);
            }
            ServiceCommand::SetNtpEnabled(en) => {
                if let Err(err) = td.set_ntp(en) {
                    timedate_error = Some(err);
                }
            }
            ServiceCommand::SetTimezone(tz) => {
                if let Err(err) = td.set_timezone(&tz) {
                    timedate_error = Some(err);
                }
            }
            ServiceCommand::SetManualDateTime(dt) => {
                if let Err(err) = td.set_manual_datetime(&dt) {
                    timedate_error = Some(err);
                }
            }
        }
    }
    let mut snap = query_service_snapshot();
    if let Some(fresh) = rescanned_networks
        && !fresh.is_empty()
    {
        snap.network.wifi_networks = fresh;
    }
    snap.wifi_action_result = wifi_action_result;
    snap.timedate_error = timedate_error;
    snap
}

pub(super) fn default_search_controls() -> (
    rusty_ui::FocusScope,
    rusty_ui::WidgetId,
    rusty_ui::TextField,
) {
    let focus_search_id = rusty_ui::WidgetId::from_tag("settings.search");
    let mut focus = rusty_ui::FocusScope::new();
    focus.begin_frame();
    focus.register(focus_search_id);
    let search_field = rusty_ui::TextField::search(rusty_ui::Rect::new(12, 48, 204, 28), "Search");
    (focus, focus_search_id, search_field)
}

pub(super) fn default_battery_tab() -> rusty_ui::SegmentedTabs {
    rusty_ui::SegmentedTabs::new(
        rusty_ui::Rect::new(0, 0, 260, 28),
        vec![
            String::from("Usage & Supply"),
            String::from("Health & Power"),
        ],
        0,
    )
}

pub(super) fn default_low_power_dropdown() -> rusty_ui::DropdownSelect {
    rusty_ui::DropdownSelect::new(
        rusty_ui::Rect::new(0, 0, 176, 26),
        vec![
            String::from("Never"),
            String::from("Only on Battery"),
            String::from("Always"),
            String::from("Only on Power Adapter"),
        ],
        0,
    )
}

pub(super) fn validate_shortcut_input(raw: &str) -> Result<String, &'static str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Enter modifiers and key (e.g. Super+Space)");
    }
    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut logo = false;
    let mut key_name: Option<String> = None;

    for token in trimmed.split('+').map(str::trim).filter(|t| !t.is_empty()) {
        match token.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => ctrl = true,
            "alt" | "opt" | "option" => alt = true,
            "shift" => shift = true,
            "super" | "logo" | "meta" | "win" | "cmd" => logo = true,
            other => {
                if key_name.is_some() {
                    return Err("Specify only one primary key after modifiers");
                }
                key_name = Some(normalize_shortcut_key_token(other)?);
            }
        }
    }

    let key = key_name.ok_or("Missing primary key (e.g. Space, W, Left)")?;
    let is_fn_or_print = key == "Print"
        || (key.starts_with('F') && key[1..].parse::<u8>().is_ok_and(|n| (1..=12).contains(&n)));
    if !ctrl && !alt && !logo && !is_fn_or_print {
        return Err("Include Ctrl, Alt, or Super modifier");
    }
    if (ctrl && alt && key == "BackSpace")
        || (ctrl && alt && key.starts_with('F') && key[1..].parse::<u8>().is_ok_and(|n| n <= 12))
        || key == "Escape"
    {
        return Err("Reserved compositor/VT safety shortcut");
    }

    let mut parts = Vec::with_capacity(5);
    if ctrl {
        parts.push("Ctrl".to_owned());
    }
    if logo {
        parts.push("Super".to_owned());
    }
    if alt {
        parts.push("Alt".to_owned());
    }
    if shift {
        parts.push("Shift".to_owned());
    }
    parts.push(key);
    Ok(parts.join("+"))
}

pub(super) fn normalize_shortcut_key_token(lower: &str) -> Result<String, &'static str> {
    let out = match lower {
        "space" => String::from("Space"),
        "tab" => String::from("Tab"),
        "comma" | "," => String::from("Comma"),
        "period" | "." => String::from("Period"),
        "slash" | "/" => String::from("Slash"),
        "left" => String::from("Left"),
        "right" => String::from("Right"),
        "up" => String::from("Up"),
        "down" => String::from("Down"),
        "return" | "enter" => String::from("Return"),
        "backspace" => String::from("BackSpace"),
        "delete" | "del" => String::from("Delete"),
        "escape" | "esc" => String::from("Escape"),
        "print" => String::from("Print"),
        fn_key
            if fn_key.starts_with('f')
                && fn_key[1..]
                    .parse::<u8>()
                    .is_ok_and(|n| (1..=12).contains(&n)) =>
        {
            fn_key.to_ascii_uppercase()
        }
        single if single.len() == 1 => {
            let b = single.as_bytes()[0];
            if b.is_ascii_alphanumeric() {
                (b as char).to_ascii_uppercase().to_string()
            } else {
                return Err("Unsupported key token; use A-Z, 0-9, Space, Tab, Left/Right, F1-F12");
            }
        }
        _ => return Err("Unsupported key token; use A-Z, 0-9, Space, Tab, Comma, Left/Right"),
    };
    Ok(out)
}

#[must_use]
pub(crate) fn gather_system_about_info() -> SystemAboutInfo {
    let mut info = SystemAboutInfo::default();
    if let Ok(h) = fs::read_to_string("/proc/sys/kernel/hostname")
        && !h.trim().is_empty()
    {
        h.trim().clone_into(&mut info.hostname);
    }
    if let Ok(os_rel) = fs::read_to_string("/etc/os-release") {
        for line in os_rel.lines() {
            if let Some(rest) = line.strip_prefix("PRETTY_NAME=") {
                rest.trim().trim_matches('"').clone_into(&mut info.os_name);
                break;
            }
        }
    }
    let ostype =
        fs::read_to_string("/proc/sys/kernel/ostype").unwrap_or_else(|_| String::from("Linux"));
    let osrel = fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
    let k = format!("{} {}", ostype.trim(), osrel.trim())
        .trim()
        .to_owned();
    if !k.is_empty() {
        info.kernel = k;
    }

    if let Ok(cpuinfo) = fs::read_to_string("/proc/cpuinfo") {
        for line in cpuinfo.lines() {
            if let Some((_, val)) = line.split_once(':')
                && (line.starts_with("model name") || line.starts_with("Hardware"))
                && !val.trim().is_empty()
            {
                val.trim().clone_into(&mut info.cpu);
                break;
            }
        }
    }

    if let Ok(meminfo) = fs::read_to_string("/proc/meminfo") {
        for line in meminfo.lines() {
            if let Some(rest) = line.strip_prefix("MemTotal:") {
                let kib = rest
                    .split_whitespace()
                    .next()
                    .and_then(|n| n.parse::<u64>().ok())
                    .unwrap_or(0);
                if kib > 0 {
                    #[allow(clippy::cast_precision_loss)]
                    let gib = kib as f64 / (1024.0 * 1024.0);
                    info.memory = format!("{gib:.1} GiB RAM");
                }
                break;
            }
        }
    }

    if let Some(gpu) = detect_drm_gpu_label() {
        info.graphics = gpu;
    }
    if let Some(disk) = detect_root_disk_summary() {
        info.startup_disk = disk;
    }
    info
}

pub(super) fn detect_drm_gpu_label() -> Option<String> {
    let entries = fs::read_dir("/sys/class/drm").ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("card") || name.contains('-') {
            continue;
        }
        let dev_dir = entry.path().join("device");
        let vendor = fs::read_to_string(dev_dir.join("vendor")).unwrap_or_default();
        let vendor_trimmed = vendor.trim().to_ascii_lowercase();
        let label = match vendor_trimmed.as_str() {
            "0x1002" => "AMD Radeon Graphics",
            "0x8086" => "Intel Integrated Graphics",
            "0x10de" => "NVIDIA Graphics",
            "0x1af4" => "VirtIO GPU",
            "0x1234" => "QEMU Standard VGA",
            "0x15ad" => "VMware SVGA",
            _ => "",
        };
        if !label.is_empty() {
            return Some(label.to_owned());
        }
        if let Ok(uevent) = fs::read_to_string(dev_dir.join("uevent")) {
            for line in uevent.lines() {
                if let Some(drv) = line.strip_prefix("DRIVER=")
                    && !drv.trim().is_empty()
                {
                    return Some(drv.trim().to_owned());
                }
            }
        }
    }
    None
}

pub(super) fn detect_root_disk_summary() -> Option<String> {
    let out = Command::new("df").args(["-Pk", "/"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().nth(1)?;
    let cols: Vec<&str> = line.split_whitespace().collect();
    if cols.len() < 4 {
        return None;
    }
    let total_kib = cols[1].parse::<u64>().ok()?;
    let avail_kib = cols[3].parse::<u64>().ok()?;
    #[allow(clippy::cast_precision_loss)]
    let total_gb = total_kib as f64 / (1024.0 * 1024.0);
    #[allow(clippy::cast_precision_loss)]
    let avail_gb = avail_kib as f64 / (1024.0 * 1024.0);
    Some(format!("{avail_gb:.1} GiB free of {total_gb:.1} GiB"))
}
