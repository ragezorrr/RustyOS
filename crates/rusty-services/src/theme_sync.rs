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
        (
            "gtk-decoration-layout",
            ":minimize,maximize,close".to_owned(),
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

    /// Atomically writes Qt 5/6 appearance files (`kdeglobals`, `qt5ct/qt5ct.conf`, `qt6ct/qt6ct.conf`)
    /// under `config_root` so third-party Qt applications sync with RustyOS Light/Dark tokens (`RUSTYOS-154`).
    ///
    /// # Errors
    /// Returns an error if creating directories or atomically replacing Qt config files fails.
    pub fn write_qt_settings_to_dir(
        config_root: &Path,
        cfg: &ThemeSyncConfig,
    ) -> Result<(), String> {
        fs::create_dir_all(config_root)
            .map_err(|err| format!("failed to create {}: {err}", config_root.display()))?;
        let kde_path = config_root.join("kdeglobals");
        let kde_tmp = config_root.join(".kdeglobals.rustyos.tmp");
        fs::write(&kde_tmp, render_kdeglobals_ini(cfg))
            .map_err(|err| format!("failed to write {}: {err}", kde_tmp.display()))?;
        fs::rename(&kde_tmp, &kde_path)
            .map_err(|err| format!("failed to replace {}: {err}", kde_path.display()))?;

        let qtct_body = render_qtct_conf(cfg);
        for (dir_name, file_name) in [("qt5ct", "qt5ct.conf"), ("qt6ct", "qt6ct.conf")] {
            let dir = config_root.join(dir_name);
            fs::create_dir_all(&dir)
                .map_err(|err| format!("failed to create {}: {err}", dir.display()))?;
            let path = dir.join(file_name);
            let tmp = dir.join(format!(".{file_name}.rustyos.tmp"));
            fs::write(&tmp, &qtct_body)
                .map_err(|err| format!("failed to write {}: {err}", tmp.display()))?;
            fs::rename(&tmp, &path)
                .map_err(|err| format!("failed to replace {}: {err}", path.display()))?;
        }
        Ok(())
    }

    /// Spawns a non-blocking background thread to sync GTK 3/4 `settings.ini`, Qt 5/6 `kdeglobals`/`qtct`, and `gsettings`.
    pub fn apply_async(cfg: ThemeSyncConfig) {
        let _ = thread::Builder::new()
            .name("rustyos-theme-sync".to_owned())
            .spawn(move || {
                if let Some(config_root) = xdg_config_home() {
                    let _ = Self::write_gtk_settings_to_dir(&config_root, &cfg);
                    let _ = Self::write_qt_settings_to_dir(&config_root, &cfg);
                }
                apply_gsettings_best_effort(&cfg);
            });
    }
}

/// Renders a clean `kdeglobals` INI matching RustyOS Light (`#F5F6FA`, `#007AFF`) or Dark (`#1C1C1D`, `#0A84FF`) tokens (`RUSTYOS-154`).
#[must_use]
pub fn render_kdeglobals_ini(cfg: &ThemeSyncConfig) -> String {
    let icon = cfg
        .icon_theme
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map_or_else(|| "Adwaita".to_owned(), sanitize_ini_value);
    let font_spec = cfg
        .font_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map_or_else(
            || "Inter,10,-1,5,50,0,0,0,0,0".to_owned(),
            sanitize_ini_value,
        );
    let (scheme_name, win_bg, view_bg, btn_bg, fg, accent) = if cfg.prefer_dark {
        (
            "RustyOSDark",
            "28,28,29",
            "38,38,40",
            "44,44,45",
            "242,242,247",
            "10,132,255",
        )
    } else {
        (
            "RustyOSLight",
            "245,246,250",
            "255,255,255",
            "236,237,241",
            "28,28,30",
            "0,122,255",
        )
    };
    format!(
        "[General]\nColorScheme={scheme_name}\nfont={font_spec}\n\n\
         [Icons]\nTheme={icon}\n\n\
         [KDE]\nLookAndFeelPackage=org.rustyos.desktop\nSingleClick=false\n\n\
         [Colors:Window]\nBackgroundNormal={win_bg}\nForegroundNormal={fg}\n\n\
         [Colors:View]\nBackgroundNormal={view_bg}\nForegroundNormal={fg}\nDecorationFocus={accent}\n\n\
         [Colors:Button]\nBackgroundNormal={btn_bg}\nForegroundNormal={fg}\nDecorationFocus={accent}\n\n\
         [Colors:Selection]\nBackgroundNormal={accent}\nForegroundNormal=255,255,255\n"
    )
}

/// Renders a `qt5ct.conf` / `qt6ct.conf` configuration synced to `ThemeSyncConfig` (`RUSTYOS-154`).
#[must_use]
pub fn render_qtct_conf(cfg: &ThemeSyncConfig) -> String {
    let icon = cfg
        .icon_theme
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map_or_else(|| "Adwaita".to_owned(), sanitize_ini_value);
    let palette_mode = if cfg.prefer_dark { "darker" } else { "default" };
    format!(
        "[Appearance]\ncolor_scheme_path={palette_mode}\ncustom_palette=false\nicon_theme={icon}\nstandard_dialogs=xdgdesktopportal\nstyle=Fusion\n\n\
         [Fonts]\ngeneral=\"Inter,10,-1,5,50,0,0,0,0,0\"\nfixed=\"JetBrains Mono,10,-1,5,50,0,0,0,0,0\"\n"
    )
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
    run_gsettings(&[
        "set",
        "org.gnome.desktop.wm.preferences",
        "button-layout",
        ":minimize,maximize,close",
    ]);
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
        assert!(merged.contains("gtk-decoration-layout=:minimize,maximize,close"));
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

        ThemeSyncService::write_qt_settings_to_dir(&dir, &cfg)?;
        let kde = fs::read_to_string(dir.join("kdeglobals"))
            .map_err(|e| format!("read kdeglobals: {e}"))?;
        let qt6 = fs::read_to_string(dir.join("qt6ct/qt6ct.conf"))
            .map_err(|e| format!("read qt6ct: {e}"))?;
        assert!(kde.contains("ColorScheme=RustyOSLight"));
        assert!(kde.contains("0,122,255"));
        assert!(qt6.contains("standard_dialogs=xdgdesktopportal"));
        let _ = fs::remove_dir_all(&dir);
        Ok(())
    }
}
