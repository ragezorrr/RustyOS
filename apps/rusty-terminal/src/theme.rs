//! RustyOS (`Clear Light` / `Clear Dark` via `rustyos_liquid_glass_v1` + opaque fallback)
//! palette, RustyOS accent sync, ANSI-16/256 color resolver, and live `config.toml` watcher.

use std::path::PathBuf;
use std::time::SystemTime;

use crate::vt::TermColor;

/// Complete color tokens for `Clear Light`, `Clear Dark`, and opaque mode.
#[derive(Debug, Clone)]
pub(crate) struct TerminalTheme {
    pub(crate) reduce_transparency: bool,
    pub(crate) reduced_motion: bool,
    pub(crate) accent: [u8; 4],
    pub(crate) window_bg: [u8; 4],
    pub(crate) header_bg: [u8; 4],
    pub(crate) tab_active_bg: [u8; 4],
    pub(crate) tab_hover_bg: [u8; 4],
    pub(crate) tab_border: [u8; 4],
    pub(crate) search_bg: [u8; 4],
    pub(crate) search_border: [u8; 4],
    pub(crate) search_match_bg: [u8; 4],
    pub(crate) search_current_bg: [u8; 4],
    pub(crate) window_border: [u8; 4],
    pub(crate) header_separator: [u8; 4],
    pub(crate) text_primary: [u8; 4],
    pub(crate) text_secondary: [u8; 4],
    pub(crate) fg_default: [u8; 4],
    pub(crate) selection_bg: [u8; 4],
    pub(crate) cursor_bg: [u8; 4],
    pub(crate) cursor_fg: [u8; 4],
    pub(crate) scrollbar_thumb: [u8; 4],
    pub(crate) badge_bg: [u8; 4],
    pub(crate) palette16: [[u8; 4]; 16],
}

/// Snapshot of user settings parsed from `~/.config/rustyos/config.toml`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TerminalConfigSnapshot {
    pub(crate) dark: bool,
    pub(crate) accent_index: usize,
    pub(crate) reduce_transparency: bool,
    pub(crate) reduced_motion: bool,
    pub(crate) glass_opacity: f32,
}

impl Default for TerminalConfigSnapshot {
    fn default() -> Self {
        Self {
            dark: true,
            accent_index: 0,
            reduce_transparency: false,
            reduced_motion: false,
            glass_opacity: 0.78,
        }
    }
}

/// Watches `config.toml` mtime cleanly without polling file contents every frame.
#[derive(Debug)]
pub(crate) struct ConfigWatcher {
    path: Option<PathBuf>,
    last_mtime: Option<SystemTime>,
    pub(crate) current: TerminalConfigSnapshot,
}

impl ConfigWatcher {
    #[must_use]
    pub(crate) fn new() -> Self {
        let path = config_file_path();
        let last_mtime = path
            .as_deref()
            .and_then(|p| std::fs::metadata(p).ok()?.modified().ok());
        let current = path
            .as_deref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map_or_else(TerminalConfigSnapshot::default, |text| {
                TerminalConfigSnapshot::parse_toml(&text)
            });
        Self {
            path,
            last_mtime,
            current,
        }
    }

    /// Checks whether `config.toml` changed on disk and returns `true` if updated.
    pub(crate) fn poll_reload(&mut self) -> bool {
        let Some(path) = self.path.as_deref() else {
            return false;
        };
        let Ok(meta) = std::fs::metadata(path) else {
            return false;
        };
        let Ok(mtime) = meta.modified() else {
            return false;
        };
        if self.last_mtime == Some(mtime) {
            return false;
        }
        self.last_mtime = Some(mtime);
        let Ok(text) = std::fs::read_to_string(path) else {
            return false;
        };
        let next = TerminalConfigSnapshot::parse_toml(&text);
        if next != self.current {
            self.current = next;
            return true;
        }
        false
    }
}

impl TerminalConfigSnapshot {
    #[must_use]
    pub(crate) fn parse_toml(text: &str) -> Self {
        let mut snap = Self::default();
        let mut section = String::new();
        for raw_line in text.lines() {
            let line = raw_line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                line[1..line.len() - 1].trim().clone_into(&mut section);
                continue;
            }
            if section != "ui" {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let key = k.trim();
            let val = v.trim().trim_matches('"').trim_matches('\'');
            match key {
                "theme" => snap.dark = val.eq_ignore_ascii_case("dark"),
                "accent_index" => {
                    if let Ok(idx) = val.parse::<usize>() {
                        snap.accent_index = idx.min(rusty_ui::ACCENT_PALETTE.len() - 1);
                    }
                }
                "reduce_transparency" => {
                    snap.reduce_transparency =
                        matches!(val.to_ascii_lowercase().as_str(), "true" | "1");
                }
                "reduced_motion" => {
                    snap.reduced_motion = matches!(val.to_ascii_lowercase().as_str(), "true" | "1");
                }
                "glass_opacity" => {
                    if let Ok(op) = val.parse::<f32>() {
                        snap.glass_opacity = op.clamp(0.35, 1.0);
                    }
                }
                _ => {}
            }
        }
        snap
    }
}

impl TerminalTheme {
    /// Builds `Clear Dark` / `Clear Light` or `Opaque` RustyOS theme from current config.
    #[must_use]
    pub(crate) fn from_config(cfg: &TerminalConfigSnapshot, glass_override: Option<bool>) -> Self {
        let use_glass = glass_override.unwrap_or(!cfg.reduce_transparency);
        let reduce_transparency = !use_glass;
        let accent = rusty_ui::theme::accent_for(cfg.accent_index, cfg.dark);

        // Keep window backdrop translucent when Liquid Glass is active (`rustyos_liquid_glass_v1`)
        // so the compositor's frosted backdrop blur renders smoothly behind terminal text.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let dark_glass_alpha = (cfg.glass_opacity * 238.0).clamp(155.0, 240.0) as u8;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let light_glass_alpha = (cfg.glass_opacity * 252.0).clamp(192.0, 244.0) as u8;

        if cfg.dark {
            let win_alpha = if reduce_transparency {
                255
            } else {
                dark_glass_alpha
            };
            Self {
                reduce_transparency,
                reduced_motion: cfg.reduced_motion,
                accent,
                window_bg: [22, 23, 27, win_alpha],
                header_bg: [
                    34,
                    36,
                    42,
                    if reduce_transparency {
                        255
                    } else {
                        win_alpha.max(210)
                    },
                ],
                tab_active_bg: [255, 255, 255, 34],
                tab_hover_bg: [255, 255, 255, 18],
                tab_border: [255, 255, 255, 36],
                search_bg: [32, 34, 40, 242],
                search_border: [255, 255, 255, 42],
                search_match_bg: [255, 214, 10, 88],
                search_current_bg: [255, 159, 10, 170],
                window_border: [255, 255, 255, 22],
                header_separator: [255, 255, 255, 24],
                text_primary: [242, 244, 248, 255],
                text_secondary: [158, 163, 174, 255],
                fg_default: [236, 239, 244, 255],
                selection_bg: [accent[0], accent[1], accent[2], 96],
                cursor_bg: [accent[0], accent[1], accent[2], 235],
                cursor_fg: [18, 20, 24, 255],
                scrollbar_thumb: [255, 255, 255, 92],
                badge_bg: [255, 255, 255, 24],
                palette16: DARK_PALETTE_16,
            }
        } else {
            let win_alpha = if reduce_transparency {
                255
            } else {
                light_glass_alpha
            };
            Self {
                reduce_transparency,
                reduced_motion: cfg.reduced_motion,
                accent,
                window_bg: [250, 251, 254, win_alpha],
                header_bg: [
                    255,
                    255,
                    255,
                    if reduce_transparency {
                        255
                    } else {
                        win_alpha.max(228)
                    },
                ],
                tab_active_bg: [255, 255, 255, 235],
                tab_hover_bg: [0, 0, 0, 14],
                tab_border: [0, 0, 0, 26],
                search_bg: [255, 255, 255, 246],
                search_border: [0, 0, 0, 32],
                search_match_bg: [255, 204, 0, 110],
                search_current_bg: [255, 149, 0, 195],
                window_border: [0, 0, 0, 24],
                header_separator: [0, 0, 0, 22],
                text_primary: [28, 30, 34, 255],
                text_secondary: [108, 113, 124, 255],
                fg_default: [28, 30, 36, 255],
                selection_bg: [accent[0], accent[1], accent[2], 68],
                cursor_bg: [accent[0], accent[1], accent[2], 230],
                cursor_fg: [255, 255, 255, 255],
                scrollbar_thumb: [0, 0, 0, 74],
                badge_bg: [0, 0, 0, 16],
                palette16: LIGHT_PALETTE_16,
            }
        }
    }

    /// Resolves any VT foreground color (`DefaultFg`, `DefaultBg`, ANSI 0..15, 256-palette, or RGB).
    #[must_use]
    pub(crate) fn resolve_fg(&self, color: TermColor, bold_bright: bool) -> [u8; 4] {
        match color {
            TermColor::DefaultFg => self.fg_default,
            TermColor::DefaultBg => self.window_bg,
            TermColor::Indexed(idx) => {
                let adjusted = if bold_bright && idx < 8 { idx + 8 } else { idx };
                self.resolve_indexed(adjusted)
            }
            TermColor::Rgb(r, g, b) => [r, g, b, 255],
        }
    }

    /// Resolves an optional cell background (`None` for transparent terminal backdrop).
    #[must_use]
    pub(crate) fn resolve_bg(&self, color: TermColor) -> Option<[u8; 4]> {
        match color {
            TermColor::DefaultBg => None,
            TermColor::DefaultFg => Some(self.fg_default),
            TermColor::Indexed(idx) => Some(self.resolve_indexed(idx)),
            TermColor::Rgb(r, g, b) => Some([r, g, b, 245]),
        }
    }

    #[must_use]
    pub(crate) fn resolve_indexed(&self, idx: u8) -> [u8; 4] {
        if idx < 16 {
            return self.palette16[usize::from(idx)];
        }
        if idx < 232 {
            let n = idx - 16;
            let b_i = n % 6;
            let g_i = (n / 6) % 6;
            let r_i = n / 36;
            let step = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            return [step(r_i), step(g_i), step(b_i), 255];
        }
        let gray = 8 + (idx - 232) * 10;
        [gray, gray, gray, 255]
    }
}

/// RustyOS Dark ANSI-16 (tuned for crisp legibility on dark Liquid Glass, zero purple shift).
const DARK_PALETTE_16: [[u8; 4]; 16] = [
    [38, 41, 48, 255],    // 0 Black
    [255, 69, 58, 255],   // 1 Red
    [50, 215, 75, 255],   // 2 Green
    [255, 214, 10, 255],  // 3 Yellow
    [10, 132, 255, 255],  // 4 Blue
    [255, 100, 130, 255], // 5 Magenta/Rose
    [90, 200, 250, 255],  // 6 Cyan
    [229, 232, 238, 255], // 7 White
    [118, 124, 136, 255], // 8 Bright Black
    [255, 105, 97, 255],  // 9 Bright Red
    [92, 230, 118, 255],  // 10 Bright Green
    [255, 224, 66, 255],  // 11 Bright Yellow
    [64, 156, 255, 255],  // 12 Bright Blue
    [255, 132, 160, 255], // 13 Bright Rose
    [112, 215, 255, 255], // 14 Bright Cyan
    [255, 255, 255, 255], // 15 Bright White
];

/// RustyOS Light ANSI-16 (high contrast on translucent light glass).
const LIGHT_PALETTE_16: [[u8; 4]; 16] = [
    [28, 30, 34, 255],    // 0 Black
    [215, 42, 34, 255],   // 1 Red
    [30, 148, 52, 255],   // 2 Green
    [168, 106, 0, 255],   // 3 Yellow/Amber
    [0, 106, 224, 255],   // 4 Blue
    [194, 48, 86, 255],   // 5 Rose
    [0, 128, 168, 255],   // 6 Cyan
    [216, 220, 226, 255], // 7 White
    [102, 108, 118, 255], // 8 Bright Black
    [235, 62, 52, 255],   // 9 Bright Red
    [40, 168, 66, 255],   // 10 Bright Green
    [194, 124, 0, 255],   // 11 Bright Amber
    [0, 122, 255, 255],   // 12 Bright Blue
    [218, 64, 104, 255],  // 13 Bright Rose
    [12, 148, 194, 255],  // 14 Bright Cyan
    [248, 249, 252, 255], // 15 Bright White
];

#[must_use]
pub(crate) fn config_file_path() -> Option<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME")
        && !xdg.is_empty()
    {
        return Some(PathBuf::from(xdg).join("rustyos/config.toml"));
    }
    let home = std::env::var("HOME").ok()?;
    (!home.is_empty()).then(|| PathBuf::from(home).join(".config/rustyos/config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ui_theme_and_resolves_rustyos_palettes() {
        let cfg = TerminalConfigSnapshot::parse_toml(
            "[ui]\ntheme = \"light\"\nreduce_transparency = false\nglass_opacity = 0.72\n",
        );
        assert!(!cfg.dark);
        assert!(!cfg.reduce_transparency);

        let clear = TerminalTheme::from_config(&cfg, None);
        assert!(clear.window_bg[3] < 255);
        assert_eq!(clear.accent, [0x00, 0x7A, 0xFF, 255]);

        let opaque = TerminalTheme::from_config(&cfg, Some(false));
        assert_eq!(opaque.window_bg[3], 255);
        assert_eq!(opaque.resolve_indexed(196), [255, 0, 0, 255]);
    }
}
