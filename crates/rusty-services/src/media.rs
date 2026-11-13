//! Audio (`wpctl`/`pactl`) and MPRIS2 media playback adapters (`RUSTYOS-011`, `RUSTYOS-031`, `RUSTYOS-053`).

use std::process::Command;

use zbus::zvariant::Value;

/// Audio volume and mute status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioStatus {
    /// Output master volume percentage (0..=100).
    pub volume_percent: u8,
    /// Master mute state.
    pub muted: bool,
    /// Backend service identified (e.g., "pipewire", "wireplumber", "alsa").
    pub backend: String,
}

impl Default for AudioStatus {
    fn default() -> Self {
        Self {
            volume_percent: 75,
            muted: false,
            backend: "default".to_owned(),
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
pub struct AudioService;

impl AudioService {
    /// Creates a new audio service adapter.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Queries the current audio volume and mute state via `wpctl` or `pactl`.
    #[must_use]
    pub fn query_status(&self) -> AudioStatus {
        let mut status = AudioStatus::default();

        // 1. Try PipeWire's WirePlumber CLI: `wpctl get-volume @DEFAULT_AUDIO_SINK@`
        if let Ok(output) = Command::new("wpctl")
            .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
            .output()
            && output.status.success()
        {
            let text = String::from_utf8_lossy(&output.stdout);
            if let Some(rest) = text.strip_prefix("Volume:") {
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
                return status;
            }
        }

        // 2. Fallback to `pactl get-sink-volume @DEFAULT_SINK@`
        if let Ok(output) = Command::new("pactl")
            .args(["get-sink-volume", "@DEFAULT_SINK@"])
            .output()
            && output.status.success()
        {
            let text = String::from_utf8_lossy(&output.stdout);
            if let Some(idx) = text.find('%') {
                let prefix = &text[..idx];
                if let Some(last_space) = prefix.rfind(' ')
                    && let Ok(val) = prefix[last_space + 1..].parse::<u8>()
                {
                    status.volume_percent = val.min(100);
                    "pulseaudio".clone_into(&mut status.backend);
                }
            }
        }

        status
    }

    /// Sets master output volume (`0..=100`) via `wpctl` or `pactl`.
    #[must_use]
    pub fn set_volume(&self, percent: u8) -> bool {
        let clamped = percent.min(100);
        let wpctl_arg = format_wpctl_volume(clamped);
        if Command::new("wpctl")
            .args(["set-volume", "@DEFAULT_AUDIO_SINK@", &wpctl_arg])
            .status()
            .is_ok_and(|s| s.success())
        {
            return true;
        }
        let pactl_arg = format!("{clamped}%");
        Command::new("pactl")
            .args(["set-sink-volume", "@DEFAULT_SINK@", &pactl_arg])
            .status()
            .is_ok_and(|s| s.success())
    }

    /// Toggles master output mute via `wpctl` or `pactl`.
    #[must_use]
    pub fn toggle_mute(&self) -> bool {
        if Command::new("wpctl")
            .args(["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"])
            .status()
            .is_ok_and(|s| s.success())
        {
            return true;
        }
        Command::new("pactl")
            .args(["set-sink-mute", "@DEFAULT_SINK@", "toggle"])
            .status()
            .is_ok_and(|s| s.success())
    }
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
        if let Ok(output) = Command::new("playerctl")
            .args(["metadata", "--format", "{{status}}\t{{title}}\t{{artist}}"])
            .output()
            && output.status.success()
        {
            let text = String::from_utf8_lossy(&output.stdout);
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
        Command::new("playerctl")
            .arg(arg)
            .status()
            .is_ok_and(|s| s.success())
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
