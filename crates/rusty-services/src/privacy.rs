//! Active microphone and camera privacy telemetry via PipeWire (`RUSTYOS-142`).
//!
//! Designed for background polling off the compositor event loop with zero-panic
//! fallback when PipeWire or `pw-dump` is unavailable.

use std::collections::BTreeSet;
use std::process::Command;

/// Snapshot of active audio capture (microphone) and video capture (camera) streams.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceUsageSnapshot {
    /// True when at least one application is actively capturing from a microphone.
    pub mic_active: bool,
    /// Names of applications currently using the microphone.
    pub mic_apps: Vec<String>,
    /// True when at least one application is actively capturing from a camera.
    pub camera_active: bool,
    /// Names of applications currently using the camera.
    pub camera_apps: Vec<String>,
}

impl DeviceUsageSnapshot {
    /// Returns true if either microphone or camera is actively in use.
    #[must_use]
    pub fn any_active(&self) -> bool {
        self.mic_active || self.camera_active
    }

    /// Formats a compact human-readable summary for tooltips or status badges.
    #[must_use]
    pub fn summary_label(&self) -> String {
        let mut parts = Vec::new();
        if self.mic_active {
            if self.mic_apps.is_empty() {
                parts.push("Mic active".to_owned());
            } else {
                parts.push(format!("Mic: {}", self.mic_apps.join(", ")));
            }
        }
        if self.camera_active {
            if self.camera_apps.is_empty() {
                parts.push("Camera active".to_owned());
            } else {
                parts.push(format!("Camera: {}", self.camera_apps.join(", ")));
            }
        }
        parts.join(" • ")
    }
}

/// Service querying active PipeWire capture streams (`RUSTYOS-142`).
#[derive(Debug, Clone, Default)]
pub struct DeviceUsageService;

impl DeviceUsageService {
    /// Queries PipeWire via `pw-dump Node` in a bounded subprocess.
    ///
    /// Returns an empty snapshot gracefully when PipeWire is unavailable.
    #[must_use]
    pub fn query_snapshot(&self) -> DeviceUsageSnapshot {
        let Ok(output) = crate::run_command_bounded(
            Command::new("pw-dump").arg("Node"),
            std::time::Duration::from_secs(3),
        ) else {
            return DeviceUsageSnapshot::default();
        };
        if !output.status.success() {
            return DeviceUsageSnapshot::default();
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_pipewire_dump_usage(&stdout)
    }
}

/// Extracts active microphone and camera consumer apps from `pw-dump` JSON output.
///
/// Uses a zero-dependency line/object scanner over `PipeWire:Interface:Node` entries
/// to avoid pulling extra JSON crates into `rusty-services`.
#[must_use]
pub fn parse_pipewire_dump_usage(raw_json: &str) -> DeviceUsageSnapshot {
    let mut mic_apps = BTreeSet::new();
    let mut cam_apps = BTreeSet::new();

    for chunk in raw_json
        .split("\"type\": \"PipeWire:Interface:Node\"")
        .skip(1)
    {
        let end = chunk
            .find("\"type\": \"PipeWire:Interface:")
            .unwrap_or(chunk.len());
        let node = &chunk[..end];

        let state = extract_json_str(node, "\"state\"");
        if state.as_deref() != Some("running") {
            continue;
        }
        let media_class = extract_json_str(node, "\"media.class\"").unwrap_or_default();
        let app_name = extract_json_str(node, "\"application.name\"")
            .or_else(|| extract_json_str(node, "\"node.description\""))
            .or_else(|| extract_json_str(node, "\"node.name\""))
            .unwrap_or_else(|| "Application".to_owned());

        let clean_app = app_name.trim();
        if clean_app.is_empty() {
            continue;
        }

        if media_class == "Stream/Input/Audio" {
            mic_apps.insert(clean_app.to_owned());
        } else if media_class == "Stream/Input/Video"
            || (media_class == "Video/Source" && !clean_app.contains("rusty-compositor"))
        {
            cam_apps.insert(clean_app.to_owned());
        }
    }

    let mic_vec: Vec<String> = mic_apps.into_iter().collect();
    let cam_vec: Vec<String> = cam_apps.into_iter().collect();
    DeviceUsageSnapshot {
        mic_active: !mic_vec.is_empty(),
        mic_apps: mic_vec,
        camera_active: !cam_vec.is_empty(),
        camera_apps: cam_vec,
    }
}

fn extract_json_str(haystack: &str, key: &str) -> Option<String> {
    let idx = haystack.find(key)?;
    let rest = &haystack[idx + key.len()..];
    let colon = rest.find(':')?;
    let after_colon = rest[colon + 1..].trim_start();
    let after_quote = after_colon.strip_prefix('"')?;
    let mut out = String::new();
    let mut escaped = false;
    for ch in after_quote.chars() {
        if escaped {
            out.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return Some(out);
        } else {
            out.push(ch);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_running_mic_and_camera_nodes_and_ignores_suspended() {
        let sample = r#"[
          {
            "id": 42,
            "type": "PipeWire:Interface:Node",
            "info": {
              "state": "running",
              "props": {
                "media.class": "Stream/Input/Audio",
                "application.name": "Firefox"
              }
            }
          },
          {
            "id": 43,
            "type": "PipeWire:Interface:Node",
            "info": {
              "state": "suspended",
              "props": {
                "media.class": "Stream/Input/Audio",
                "application.name": "IdleApp"
              }
            }
          },
          {
            "id": 44,
            "type": "PipeWire:Interface:Node",
            "info": {
              "state": "running",
              "props": {
                "media.class": "Stream/Input/Video",
                "application.name": "OBS Studio"
              }
            }
          }
        ]"#;

        let snap = parse_pipewire_dump_usage(sample);
        assert!(snap.mic_active);
        assert_eq!(snap.mic_apps, vec!["Firefox"]);
        assert!(snap.camera_active);
        assert_eq!(snap.camera_apps, vec!["OBS Studio"]);
        assert_eq!(snap.summary_label(), "Mic: Firefox • Camera: OBS Studio");
    }
}
