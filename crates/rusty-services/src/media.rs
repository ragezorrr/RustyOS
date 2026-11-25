//! Audio (`wpctl`/`pactl`) and MPRIS2 media playback adapters (`RUSTYOS-011`, `RUSTYOS-031`, `RUSTYOS-053`).

use std::process::Command;
use std::sync::{Arc, Mutex};

use zbus::zvariant::Value;

use crate::audio_pulse::{PulseClient, PulseError};

/// Single discovered audio output sink (`RUSTYOS-156`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioOutputDevice {
    /// `WirePlumber` node ID (`"48"`) or `PulseAudio` sink name (`"alsa_output..."`).
    pub id: String,
    /// Human-readable device label (`"Built-in Audio Analog Stereo"`).
    pub name: String,
    /// True when this sink is the active default audio output.
    pub is_default: bool,
}

/// Audio volume, mute, and output device status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioStatus {
    /// Output master volume percentage (0..=100).
    pub volume_percent: u8,
    /// Master mute state.
    pub muted: bool,
    /// Backend service identified (e.g., "pipewire", "wireplumber", "alsa").
    pub backend: String,
    /// Active output sink label (`"Speakers"`).
    pub active_output_name: String,
    /// Available audio output sinks discovered from `wpctl` or `pactl` (`RUSTYOS-156`).
    pub outputs: Vec<AudioOutputDevice>,
}

impl Default for AudioStatus {
    fn default() -> Self {
        Self {
            volume_percent: 75,
            muted: false,
            backend: "default".to_owned(),
            active_output_name: "Speakers".to_owned(),
            outputs: Vec::new(),
        }
    }
}

/// Active MPRIS2 media playback status and metadata (`RUSTYOS-053`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MediaStatus {
    /// True if an active MPRIS2 media player is registered on the session bus.
    pub active: bool,
    /// True if the active media player is currently playing.
    pub playing: bool,
    /// Concise track title (empty when nothing is playing).
    pub title: String,
    /// Optional artist metadata.
    pub artist: Option<String>,
}

/// Media playback control command dispatched via MPRIS2 / `playerctl` (`RUSTYOS-031`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaAction {
    /// Toggle play/pause on the active player.
    PlayPause,
    /// Skip to next track.
    Next,
    /// Return to previous track.
    Previous,
    /// Stop playback.
    Stop,
}

/// Audio service interacting with PipeWire/WirePlumber or PulseAudio/ALSA.
#[derive(Debug, Default, Clone)]
pub struct AudioService {
    pulse_client: Arc<Mutex<Option<PulseClient>>>,
}

impl AudioService {
    /// Creates a new audio service adapter.
    #[must_use]
    pub fn new() -> Self {
        Self {
            pulse_client: Arc::new(Mutex::new(None)),
        }
    }

    fn with_pulse_client<R>(
        &self,
        mut f: impl FnMut(&mut PulseClient) -> Result<R, PulseError>,
    ) -> Option<R> {
        let mut guard = self.pulse_client.lock().ok()?;
        if guard.is_none()
            && let Ok(client) = PulseClient::connect()
        {
            *guard = Some(client);
        }
        if let Some(client) = guard.as_mut() {
            if let Ok(res) = f(client) {
                Some(res)
            } else {
                // Drop dead socket connection and retry once
                *guard = None;
                if let Ok(mut new_client) = PulseClient::connect() {
                    let res = f(&mut new_client).ok();
                    *guard = Some(new_client);
                    res
                } else {
                    None
                }
            }
        } else {
            None
        }
    }

    /// Queries the current audio volume and mute state via native PipeWire socket or `wpctl`/`pactl` fallbacks.
    #[must_use]
    pub fn query_status(&self) -> AudioStatus {
        // 1. Try native PipeWire/PulseAudio UNIX domain socket first (sub-millisecond, zero-fork)
        if let Some(sink) = self.with_pulse_client(PulseClient::get_default_sink) {
            let active_output_name = if sink.description.is_empty() {
                sink.name.clone()
            } else {
                sink.description.clone()
            };
            let mut status = AudioStatus {
                volume_percent: sink.volume_percent,
                muted: sink.muted,
                backend: "pipewire-native".to_string(),
                active_output_name,
                outputs: self.list_outputs(),
            };
            if status.outputs.is_empty() {
                status.outputs.push(AudioOutputDevice {
                    id: sink.name.clone(),
                    name: status.active_output_name.clone(),
                    is_default: true,
                });
            } else if let Some(def) = status
                .outputs
                .iter()
                .find(|o| o.is_default)
                .or_else(|| status.outputs.first())
            {
                def.name.clone_into(&mut status.active_output_name);
            }
            return status;
        }

        let mut status = AudioStatus::default();

        // 2. Try PipeWire's WirePlumber CLI: `wpctl get-volume @DEFAULT_AUDIO_SINK@`
        if let Some(text) = run_media_cmd_output("wpctl", &["get-volume", "@DEFAULT_AUDIO_SINK@"])
            && let Some(rest) = text.strip_prefix("Volume:")
        {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if let Some(vol_str) = parts.first()
                && let Ok(vol_f) = vol_str.parse::<f32>()
            {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                {
                    status.volume_percent = ((vol_f * 100.0).round() as u8).min(100);
                }
            }
            status.muted = text.contains("[MUTED]");
            "wireplumber".clone_into(&mut status.backend);
            status.outputs = self.list_outputs();
            if let Some(def) = status
                .outputs
                .iter()
                .find(|o| o.is_default)
                .or_else(|| status.outputs.first())
            {
                def.name.clone_into(&mut status.active_output_name);
            }
            return status;
        }

        // 2. Fallback to `pactl get-sink-volume @DEFAULT_SINK@`
        if let Some(text) = run_media_cmd_output("pactl", &["get-sink-volume", "@DEFAULT_SINK@"]) {
            if let Some(idx) = text.find('%') {
                let prefix = &text[..idx];
                if let Some(last_space) = prefix.rfind(' ')
                    && let Ok(val) = prefix[last_space + 1..].parse::<u8>()
                {
                    status.volume_percent = val.min(100);
                    "pulseaudio".clone_into(&mut status.backend);
                }
            }
            status.outputs = self.list_outputs();
            if let Some(def) = status
                .outputs
                .iter()
                .find(|o| o.is_default)
                .or_else(|| status.outputs.first())
            {
                def.name.clone_into(&mut status.active_output_name);
            }
        }

        status
    }

    /// Lists available audio output sinks via `wpctl status` or `pactl list short sinks` (`RUSTYOS-156`).
    #[must_use]
    pub fn list_outputs(&self) -> Vec<AudioOutputDevice> {
        if let Some(text) = run_media_cmd_output("wpctl", &["status"]) {
            let parsed = parse_wpctl_status_sinks(&text);
            if !parsed.is_empty() {
                return parsed;
            }
        }

        let default_sink = run_media_cmd_output("pactl", &["get-default-sink"])
            .map_or_else(String::new, |s| s.trim().to_owned());

        if let Some(text) = run_media_cmd_output("pactl", &["list", "short", "sinks"]) {
            let parsed = parse_pactl_short_sinks(&text, &default_sink);
            if !parsed.is_empty() {
                return parsed;
            }
        }
        Vec::new()
    }

    /// Switches the default audio output sink by `WirePlumber` node ID or `PulseAudio` sink name (`RUSTYOS-156`, `RUSTYOS-171`).
    #[must_use]
    pub fn set_default_output(&self, id: &str) -> bool {
        let trimmed = id.trim();
        if trimmed.is_empty() {
            return false;
        }
        // 1. If non-numeric sink name, try native PulseAudio/PipeWire socket first
        if !trimmed.chars().all(|c| c.is_ascii_digit())
            && self
                .with_pulse_client(|c| c.set_default_sink(trimmed))
                .is_some()
        {
            return true;
        }
        if trimmed.chars().all(|c| c.is_ascii_digit())
            && run_media_cmd_status("wpctl", &["set-default", trimmed])
        {
            return true;
        }
        run_media_cmd_status("pactl", &["set-default-sink", trimmed])
    }

    /// Cycles to the next available audio output sink and returns the newly selected device (`RUSTYOS-156`).
    #[must_use]
    pub fn cycle_next_output(&self) -> Option<AudioOutputDevice> {
        let outputs = self.list_outputs();
        if outputs.len() < 2 {
            return outputs.into_iter().next();
        }
        let cur_idx = outputs.iter().position(|o| o.is_default).unwrap_or(0);
        let next = outputs[(cur_idx + 1) % outputs.len()].clone();
        if self.set_default_output(&next.id) {
            Some(AudioOutputDevice {
                is_default: true,
                ..next
            })
        } else {
            None
        }
    }

    /// Ensures hardware ALSA mixer channels (`Master`, `Speaker`, `Headphone`, `PCM`) are unmuted
    /// on session startup so fresh Arch/RustyOS installations have working audio immediately (`RUSTYOS-156`).
    pub fn ensure_session_audio_ready(&self) {
        for channel in ["Master", "Speaker", "Headphone", "PCM"] {
            let _ = run_media_cmd_status("amixer", &["-q", "sset", channel, "unmute"]);
        }
    }

    /// Sets master output volume (`0..=100`) via native PipeWire socket or `wpctl`/`pactl` fallbacks.
    #[must_use]
    pub fn set_volume(&self, percent: u8) -> bool {
        let clamped = percent.min(100);
        // 1. Try native socket first (zero-fork, < 0.2 ms!)
        if self
            .with_pulse_client(|c| c.set_sink_volume("@DEFAULT_SINK@", clamped))
            .is_some()
        {
            return true;
        }
        let wpctl_arg = format_wpctl_volume(clamped);
        if run_media_cmd_status("wpctl", &["set-volume", "@DEFAULT_AUDIO_SINK@", &wpctl_arg]) {
            return true;
        }
        let pactl_arg = format!("{clamped}%");
        run_media_cmd_status("pactl", &["set-sink-volume", "@DEFAULT_SINK@", &pactl_arg])
    }

    /// Toggles master output mute via native PipeWire socket or `wpctl`/`pactl` fallbacks.
    #[must_use]
    pub fn toggle_mute(&self) -> bool {
        // 1. Try native socket first (zero-fork)
        if let Some(toggled) = self.with_pulse_client(|c| {
            let cur = c.get_default_sink()?;
            c.set_sink_mute("@DEFAULT_SINK@", !cur.muted)?;
            Ok(true)
        }) {
            return toggled;
        }
        if run_media_cmd_status("wpctl", &["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"]) {
            return true;
        }
        run_media_cmd_status("pactl", &["set-sink-mute", "@DEFAULT_SINK@", "toggle"])
    }
}

const MEDIA_CMD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

fn run_media_cmd_output(program: &str, args: &[&str]) -> Option<String> {
    let output =
        crate::run_command_bounded(Command::new(program).args(args), MEDIA_CMD_TIMEOUT).ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn run_media_cmd_status(program: &str, args: &[&str]) -> bool {
    crate::run_command_bounded(Command::new(program).args(args), MEDIA_CMD_TIMEOUT)
        .is_ok_and(|out| out.status.success())
}

/// Media player control service interacting with MPRIS2 over Session D-Bus and `playerctl` (`RUSTYOS-031`).
#[derive(Debug, Default, Clone)]
pub struct MediaService;

impl MediaService {
    /// Creates a new MPRIS2/playerctl media service.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Queries the first active MPRIS2 player over Session D-Bus (or `playerctl`) (`RUSTYOS-053`).
    #[must_use]
    pub fn query_status(&self) -> MediaStatus {
        if let Some(status) = query_mpris_via_dbus() {
            return status;
        }
        if let Some(text) = run_media_cmd_output(
            "playerctl",
            &["metadata", "--format", "{{status}}\t{{title}}\t{{artist}}"],
        ) {
            return parse_playerctl_status_line(&text);
        }
        MediaStatus::default()
    }

    /// Sends a media playback control action to the first active MPRIS2 player (or `playerctl`).
    #[must_use]
    pub fn dispatch(&self, action: MediaAction) -> bool {
        if dispatch_mpris_via_dbus(action) {
            return true;
        }
        let arg = match action {
            MediaAction::PlayPause => "play-pause",
            MediaAction::Next => "next",
            MediaAction::Previous => "previous",
            MediaAction::Stop => "stop",
        };
        run_media_cmd_status("playerctl", &[arg])
    }
}

/// Parses a tab-delimited `playerctl metadata --format "{{status}}\t{{title}}\t{{artist}}"` line (`RUSTYOS-053`).
#[must_use]
pub fn parse_playerctl_status_line(line: &str) -> MediaStatus {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return MediaStatus::default();
    }
    let mut parts = trimmed.split('\t');
    let status_raw = parts.next().unwrap_or_default().trim();
    let title = parts.next().unwrap_or_default().trim().to_owned();
    let artist_raw = parts.next().unwrap_or_default().trim();
    let playing = status_raw.eq_ignore_ascii_case("Playing");
    let paused = status_raw.eq_ignore_ascii_case("Paused");
    let active = playing || paused || !title.is_empty();
    MediaStatus {
        active,
        playing,
        title,
        artist: (!artist_raw.is_empty()).then(|| artist_raw.to_owned()),
    }
}

fn first_mpris_bus_name(conn: &zbus::blocking::Connection) -> Option<String> {
    let dbus = zbus::blocking::fdo::DBusProxy::new(conn).ok()?;
    let names = dbus.list_names().ok()?;
    names
        .into_iter()
        .find(|n| n.as_str().starts_with("org.mpris.MediaPlayer2."))
        .map(|n| n.as_str().to_owned())
}

fn unwrap_zvariant_str(val: &Value<'_>) -> Option<String> {
    match val {
        Value::Str(s) => Some(s.as_str().trim().to_owned()).filter(|t| !t.is_empty()),
        Value::Value(inner) => unwrap_zvariant_str(inner),
        _ => None,
    }
}

fn query_mpris_via_dbus() -> Option<MediaStatus> {
    let conn = zbus::blocking::Connection::session().ok()?;
    let Some(player_bus) = first_mpris_bus_name(&conn) else {
        // Session D-Bus is reachable and no MPRIS2 player is active.
        return Some(MediaStatus::default());
    };
    let player_iface =
        zbus::names::InterfaceName::from_static_str("org.mpris.MediaPlayer2.Player").ok()?;
    let props = zbus::blocking::fdo::PropertiesProxy::builder(&conn)
        .destination(player_bus.as_str())
        .ok()?
        .path("/org/mpris/MediaPlayer2")
        .ok()?
        .build()
        .ok()?;
    let all = props.get_all(player_iface).ok()?;

    let playback = all
        .get("PlaybackStatus")
        .and_then(|v| unwrap_zvariant_str(v))
        .unwrap_or_default();
    let playing = playback.eq_ignore_ascii_case("Playing");

    let mut title = String::new();
    let mut artist = None;
    if let Some(meta_val) = all.get("Metadata").map(|v| &**v) {
        let dict_ref = match meta_val {
            Value::Dict(d) => Some(d),
            Value::Value(inner) => match &**inner {
                Value::Dict(d) => Some(d),
                _ => None,
            },
            _ => None,
        };
        if let Some(dict) = dict_ref {
            for (k, v) in dict.iter() {
                if let Some(key) = unwrap_zvariant_str(k) {
                    if key == "xesam:title" {
                        if let Some(t) = unwrap_zvariant_str(v) {
                            title = t;
                        }
                    } else if key == "xesam:artist" {
                        let arr_ref = match v {
                            Value::Array(a) => Some(a),
                            Value::Value(inner) => match &**inner {
                                Value::Array(a) => Some(a),
                                _ => None,
                            },
                            _ => None,
                        };
                        if let Some(arr) = arr_ref
                            && let Some(first) = arr.iter().find_map(unwrap_zvariant_str)
                        {
                            artist = Some(first);
                        } else if let Some(single) = unwrap_zvariant_str(v) {
                            artist = Some(single);
                        }
                    }
                }
            }
        }
    }

    Some(MediaStatus {
        active: true,
        playing,
        title,
        artist,
    })
}

fn dispatch_mpris_via_dbus(action: MediaAction) -> bool {
    let Ok(conn) = zbus::blocking::Connection::session() else {
        return false;
    };
    let Some(player_bus) = first_mpris_bus_name(&conn) else {
        return false;
    };
    let method = match action {
        MediaAction::PlayPause => "PlayPause",
        MediaAction::Next => "Next",
        MediaAction::Previous => "Previous",
        MediaAction::Stop => "Stop",
    };
    conn.call_method(
        Some(player_bus.as_str()),
        "/org/mpris/MediaPlayer2",
        Some("org.mpris.MediaPlayer2.Player"),
        method,
        &(),
    )
    .is_ok()
}

/// Formats `percent` (`0..=100`) into `wpctl` scalar volume string (e.g. `"0.75"`).
#[must_use]
pub fn format_wpctl_volume(percent: u8) -> String {
    let clamped = percent.min(100);
    format!("{:.2}", f32::from(clamped) / 100.0)
}

/// Parses `wpctl status` output and extracts audio output sinks under `Audio` -> `Sinks:` (`RUSTYOS-156`).
#[must_use]
pub fn parse_wpctl_status_sinks(output: &str) -> Vec<AudioOutputDevice> {
    let mut in_audio = false;
    let mut in_sinks = false;
    let mut sinks = Vec::new();

    for raw_line in output.lines() {
        let trimmed = raw_line.trim();
        if trimmed == "Audio" {
            in_audio = true;
            in_sinks = false;
            continue;
        }
        if in_audio && (trimmed == "Video" || trimmed == "Settings") {
            break;
        }
        if !in_audio {
            continue;
        }
        let cleaned = trimmed.trim_start_matches(['│', '├', '└', '─', ' ']).trim();
        if cleaned.starts_with("Sinks:") {
            in_sinks = true;
            continue;
        }
        if in_sinks
            && (cleaned.starts_with("Sources:")
                || cleaned.starts_with("Sink endpoints:")
                || cleaned.starts_with("Source endpoints:")
                || cleaned.starts_with("Filters:")
                || cleaned.starts_with("Streams:")
                || cleaned.starts_with("Devices:"))
        {
            in_sinks = false;
            continue;
        }
        if !in_sinks || cleaned.is_empty() {
            continue;
        }

        let is_default = cleaned.starts_with('*');
        let after_star = cleaned.trim_start_matches('*').trim();
        let Some((id_str, rest)) = after_star.split_once('.') else {
            continue;
        };
        let id = id_str.trim();
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let name_part = rest.split(" [vol:").next().unwrap_or(rest).trim();
        if name_part.is_empty() {
            continue;
        }
        sinks.push(AudioOutputDevice {
            id: id.to_owned(),
            name: name_part.to_owned(),
            is_default,
        });
    }
    sinks
}

/// Parses `pactl list short sinks` tab-delimited lines (`RUSTYOS-156`).
#[must_use]
pub fn parse_pactl_short_sinks(output: &str, default_sink: &str) -> Vec<AudioOutputDevice> {
    let mut sinks = Vec::new();
    for line in output.lines() {
        let mut cols = line.split('\t');
        let _idx = cols.next();
        let Some(sink_name) = cols.next().map(str::trim).filter(|s| !s.is_empty()) else {
            continue;
        };
        let is_default = !default_sink.is_empty() && sink_name == default_sink.trim();
        let pretty = humanize_pactl_sink_name(sink_name);
        sinks.push(AudioOutputDevice {
            id: sink_name.to_owned(),
            name: pretty,
            is_default,
        });
    }
    if !sinks.is_empty() && !sinks.iter().any(|s| s.is_default) {
        sinks[0].is_default = true;
    }
    sinks
}

fn humanize_pactl_sink_name(sink_name: &str) -> String {
    let lower = sink_name.to_ascii_lowercase();
    if lower.contains("hdmi") {
        "HDMI / DisplayPort Audio".to_owned()
    } else if lower.contains("bluez") || lower.contains("bluetooth") {
        "Bluetooth Audio".to_owned()
    } else if lower.contains("usb") {
        "USB Audio Output".to_owned()
    } else if lower.contains("analog") || lower.contains("speaker") {
        "Built-in Speakers".to_owned()
    } else {
        sink_name.to_owned()
    }
}
