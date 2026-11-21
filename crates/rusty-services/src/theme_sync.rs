//! Third-party application theme synchronization (`GTK 3/4`, `gsettings`, `RUSTYOS-135`).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;

/// Snapshot of RustyOS appearance settings synchronized to GTK 3/4 and `gsettings` (`RUSTYOS-135`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeSyncConfig {
    /// Whether dark appearance is active (`true` = Dark, `false` = Light).
    pub prefer_dark: bool,
    /// Optional XDG icon theme name (e.g. `"Papirus-Dark"` or `"Adwaita"`).
    pub icon_theme: Option<String>,
    /// Optional `XCursor` theme name (e.g. `"Adwaita"`).
    pub cursor_theme: Option<String>,
    /// Nominal cursor size in logical pixels (`16..=64`).
    pub cursor_size: u32,
    /// Optional UI font specification (e.g. `"Inter 10"`).
    pub font_name: Option<String>,
}

impl Default for ThemeSyncConfig {
    fn default() -> Self {
        Self {
            prefer_dark: true,
            icon_theme: None,
            cursor_theme: None,
            cursor_size: 24,
            font_name: None,
        }
    }
}

/// Merges `cfg` into an existing GTK `settings.ini` string while preserving all unrelated sections and keys.
#[must_use]
pub fn merge_gtk_settings_ini(existing: &str, cfg: &ThemeSyncConfig) -> String {
    let mut managed: Vec<(&str, String)> = vec![
        (
            "gtk-application-prefer-dark-theme",
            if cfg.prefer_dark { "1" } else { "0" }.to_owned(),
        ),
        (
            "gtk-cursor-theme-size",
            cfg.cursor_size.clamp(16, 64).to_string(),
        ),
    ];
    if let Some(icon) = cfg
        .icon_theme
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        managed.push(("gtk-icon-theme-name", sanitize_ini_value(icon)));
    }
    if let Some(cursor) = cfg
        .cursor_theme
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        managed.push(("gtk-cursor-theme-name", sanitize_ini_value(cursor)));
    }
    if let Some(font) = cfg
        .font_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        managed.push(("gtk-font-name", sanitize_ini_value(font)));
    }

    let mut out = Vec::new();
    let mut in_settings = false;
    let mut saw_settings = false;
    let mut written_keys: Vec<&str> = Vec::new();

    for line in existing.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if in_settings {
                for (k, v) in &managed {
                    if !written_keys.contains(k) {
                        out.push(format!("{k}={v}"));
                        written_keys.push(k);
                    }
                }
            }
            in_settings = trimmed.eq_ignore_ascii_case("[Settings]");
            if in_settings {
                saw_settings = true;
            }
            out.push(line.to_owned());
            continue;
        }
        if in_settings
            && let Some((raw_key, _)) = trimmed.split_once('=')
            && let key = raw_key.trim()
            && let Some((_, val)) = managed.iter().find(|(k, _)| *k == key)
        {
            if !written_keys.contains(&key) {
                out.push(format!("{key}={val}"));
                written_keys.push(key);
            }
            continue;
        }
        out.push(line.to_owned());
    }

    if !saw_settings {
        if !out.is_empty() && !out.last().is_some_and(String::is_empty) {
            out.push(String::new());
        }
        out.push("[Settings]".to_owned());
    }
    for (k, v) in &managed {
        if !written_keys.contains(k) {
            out.push(format!("{k}={v}"));
            written_keys.push(k);
        }
    }

    let mut joined = out.join("\n");
    joined.push('\n');
    joined
}

fn sanitize_ini_value(raw: &str) -> String {
    raw.chars().filter(|c| !c.is_control()).collect()
}

/// Non-blocking theme synchronizer for GTK 3/4 and `gsettings` (`RUSTYOS-135`).
#[derive(Debug, Default, Clone, Copy)]
pub struct ThemeSyncService;

impl ThemeSyncService {
    /// Atomically writes `gtk-3.0/settings.ini` and `gtk-4.0/settings.ini` under `config_root`.
    ///
    /// # Errors
    /// Returns an error if creating directories or atomically replacing `settings.ini` fails.
    pub fn write_gtk_settings_to_dir(
        config_root: &Path,
        cfg: &ThemeSyncConfig,
    ) -> Result<(), String> {
        for sub in ["gtk-3.0", "gtk-4.0"] {
            let dir = config_root.join(sub);
            fs::create_dir_all(&dir)
                .map_err(|err| format!("failed to create {}: {err}", dir.display()))?;
            let path = dir.join("settings.ini");
            let existing = fs::read_to_string(&path).unwrap_or_default();
            let merged = merge_gtk_settings_ini(&existing, cfg);
            let tmp = dir.join(".settings.ini.rustyos.tmp");
            fs::write(&tmp, merged)
                .map_err(|err| format!("failed to write {}: {err}", tmp.display()))?;
            fs::rename(&tmp, &path)
                .map_err(|err| format!("failed to replace {}: {err}", path.display()))?;
        }
        Ok(())
    }

    /// Spawns a non-blocking background thread to sync GTK 3/4 `settings.ini` and `gsettings`.
    pub fn apply_async(cfg: ThemeSyncConfig) {
        let _ = thread::Builder::new()
            .name("rustyos-theme-sync".to_owned())
            .spawn(move || {
                if let Some(config_root) = xdg_config_home() {
                    let _ = Self::write_gtk_settings_to_dir(&config_root, &cfg);
                }
                apply_gsettings_best_effort(&cfg);
            });
    }
}

fn xdg_config_home() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|v| !v.is_empty())
                .map(|home| PathBuf::from(home).join(".config"))
        })
}

fn apply_gsettings_best_effort(cfg: &ThemeSyncConfig) {
    let scheme = if cfg.prefer_dark {
        "prefer-dark"
    } else {
        "prefer-light"
    };
    run_gsettings(&["set", "org.gnome.desktop.interface", "color-scheme", scheme]);
    if let Some(icon) = cfg
        .icon_theme
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        run_gsettings(&["set", "org.gnome.desktop.interface", "icon-theme", icon]);
    }
    if let Some(cursor) = cfg
        .cursor_theme
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        run_gsettings(&["set", "org.gnome.desktop.interface", "cursor-theme", cursor]);
    }
}

fn run_gsettings(args: &[&str]) {
    let _ = Command::new("gsettings")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_gtk_settings_ini_preserves_user_keys_and_updates_theme_fields() {
        let existing = "[Settings]\ngtk-enable-animations=1\ngtk-application-prefer-dark-theme=0\n";
        let cfg = ThemeSyncConfig {
            prefer_dark: true,
            icon_theme: Some("Papirus-Dark".to_owned()),
            cursor_theme: Some("Adwaita".to_owned()),
            cursor_size: 24,
            font_name: Some("Inter 10".to_owned()),
        };
        let merged = merge_gtk_settings_ini(existing, &cfg);
        assert!(merged.contains("gtk-enable-animations=1"));
        assert!(merged.contains("gtk-application-prefer-dark-theme=1"));
        assert!(!merged.contains("gtk-application-prefer-dark-theme=0"));
        assert!(merged.contains("gtk-icon-theme-name=Papirus-Dark"));
        assert!(merged.contains("gtk-cursor-theme-name=Adwaita"));
        assert!(merged.contains("gtk-cursor-theme-size=24"));
        assert!(merged.contains("gtk-font-name=Inter 10"));
    }

    #[test]
    fn write_gtk_settings_creates_gtk3_and_gtk4_files_atomically() -> Result<(), String> {
        let dir =
            std::env::temp_dir().join(format!("rustyos_theme_sync_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let cfg = ThemeSyncConfig {
            prefer_dark: false,
            icon_theme: Some("Adwaita".to_owned()),
            cursor_theme: None,
            cursor_size: 24,
            font_name: None,
        };
        ThemeSyncService::write_gtk_settings_to_dir(&dir, &cfg)?;
        let gtk3 = fs::read_to_string(dir.join("gtk-3.0/settings.ini"))
            .map_err(|e| format!("read gtk3: {e}"))?;
        let gtk4 = fs::read_to_string(dir.join("gtk-4.0/settings.ini"))
            .map_err(|e| format!("read gtk4: {e}"))?;
        assert!(gtk3.contains("gtk-application-prefer-dark-theme=0"));
        assert!(gtk4.contains("gtk-icon-theme-name=Adwaita"));
        let _ = fs::remove_dir_all(&dir);
        Ok(())
    }
}
