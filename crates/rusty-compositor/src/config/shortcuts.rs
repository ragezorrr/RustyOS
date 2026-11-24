//! Customizable global keyboard shortcut definitions and parser (`RUSTYOS-170`).

use smithay::input::keyboard::{Keysym, ModifiersState};

use super::{escape_toml_string, strip_toml_quotes};

/// Parsed keyboard modifier/keysym combination for configurable compositor shortcuts (`RUSTYOS-131`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct ShortcutCombo {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub logo: bool,
    pub keysym: Keysym,
}

impl ShortcutCombo {
    /// Checks whether live modifiers + keysym match this configured binding (ASCII letters matched case-insensitively).
    #[must_use]
    pub fn matches(self, mods: ModifiersState, sym: Keysym) -> bool {
        mods.ctrl == self.ctrl
            && mods.alt == self.alt
            && mods.shift == self.shift
            && mods.logo == self.logo
            && normalize_match_keysym(sym) == normalize_match_keysym(self.keysym)
    }

    /// Formats a shortcut back into canonical `Super+Shift+R` notation.
    #[must_use]
    pub fn canonical_string(self) -> String {
        let mut parts = Vec::with_capacity(5);
        if self.ctrl {
            parts.push("Ctrl".to_owned());
        }
        if self.logo {
            parts.push("Super".to_owned());
        }
        if self.alt {
            parts.push("Alt".to_owned());
        }
        if self.shift {
            parts.push("Shift".to_owned());
        }
        parts.push(keysym_canonical_name(self.keysym));
        parts.join("+")
    }
}

const fn normalize_match_keysym(sym: Keysym) -> u32 {
    let raw = sym.raw();
    if raw >= Keysym::A.raw() && raw <= Keysym::Z.raw() {
        raw + 0x20
    } else if raw == Keysym::ISO_Left_Tab.raw() {
        Keysym::Tab.raw()
    } else {
        raw
    }
}

fn keysym_canonical_name(sym: Keysym) -> String {
    match sym {
        Keysym::space => "Space".to_owned(),
        Keysym::Tab | Keysym::ISO_Left_Tab => "Tab".to_owned(),
        Keysym::comma => "Comma".to_owned(),
        Keysym::period => "Period".to_owned(),
        Keysym::slash => "Slash".to_owned(),
        Keysym::Left => "Left".to_owned(),
        Keysym::Right => "Right".to_owned(),
        Keysym::Up => "Up".to_owned(),
        Keysym::Down => "Down".to_owned(),
        Keysym::Return | Keysym::KP_Enter => "Return".to_owned(),
        Keysym::BackSpace => "BackSpace".to_owned(),
        Keysym::Delete => "Delete".to_owned(),
        Keysym::Escape => "Escape".to_owned(),
        Keysym::Print => "Print".to_owned(),
        _ => {
            let raw = normalize_match_keysym(sym);
            if let Some(ch) = Keysym::new(raw).key_char()
                && ch.is_ascii_alphanumeric()
            {
                return ch.to_ascii_uppercase().to_string();
            }
            if raw >= Keysym::F1.raw() && raw <= Keysym::F12.raw() {
                return format!("F{}", raw - Keysym::F1.raw() + 1);
            }
            format!("0x{raw:x}")
        }
    }
}

/// Parses `"Super+Space"`, `"Alt+W"`, `"Super+Shift+R"`, `"Alt+Left"` into [`ShortcutCombo`] (`RUSTYOS-131`).
#[must_use]
pub fn parse_shortcut_combo(raw: &str) -> Option<ShortcutCombo> {
    let stripped = strip_toml_quotes(raw);
    if stripped.is_empty() {
        return None;
    }
    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut logo = false;
    let mut keysym = None;

    for token in stripped.split('+').map(str::trim).filter(|t| !t.is_empty()) {
        let lower = token.to_ascii_lowercase();
        match lower.as_str() {
            "ctrl" | "control" => ctrl = true,
            "alt" | "opt" | "option" => alt = true,
            "shift" => shift = true,
            "super" | "logo" | "meta" | "win" | "cmd" => logo = true,
            key => {
                if keysym.is_some() {
                    return None;
                }
                keysym = parse_key_token(key);
                keysym?;
            }
        }
    }

    let ks = keysym?;
    let is_fn_or_print =
        ks == Keysym::Print || (ks.raw() >= Keysym::F1.raw() && ks.raw() <= Keysym::F12.raw());
    if !ctrl && !alt && !logo && !is_fn_or_print {
        return None;
    }
    Some(ShortcutCombo {
        ctrl,
        alt,
        shift,
        logo,
        keysym: ks,
    })
}

fn parse_key_token(lower: &str) -> Option<Keysym> {
    let sym = match lower {
        "space" => Keysym::space,
        "tab" => Keysym::Tab,
        "comma" | "," => Keysym::comma,
        "period" | "." => Keysym::period,
        "slash" | "/" => Keysym::slash,
        "left" => Keysym::Left,
        "right" => Keysym::Right,
        "up" => Keysym::Up,
        "down" => Keysym::Down,
        "return" | "enter" => Keysym::Return,
        "backspace" => Keysym::BackSpace,
        "delete" | "del" => Keysym::Delete,
        "escape" | "esc" => Keysym::Escape,
        "print" => Keysym::Print,
        "f1" => Keysym::F1,
        "f2" => Keysym::F2,
        "f3" => Keysym::F3,
        "f4" => Keysym::F4,
        "f5" => Keysym::F5,
        "f6" => Keysym::F6,
        "f7" => Keysym::F7,
        "f8" => Keysym::F8,
        "f9" => Keysym::F9,
        "f10" => Keysym::F10,
        "f11" => Keysym::F11,
        "f12" => Keysym::F12,
        single if single.len() == 1 => {
            let b = single.as_bytes()[0];
            if b.is_ascii_lowercase() || b.is_ascii_digit() {
                Keysym::new(u32::from(b))
            } else {
                return None;
            }
        }
        _ => return None,
    };
    Some(sym)
}

/// User-customizable compositor shortcuts stored under `[shortcuts]` (`RUSTYOS-131`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutBindings {
    pub launcher: String,
    pub overview: String,
    pub lock_screen: String,
    pub settings: String,
    pub reload_config: String,
    pub quick_settings: String,
    pub close_window: String,
    pub toggle_maximize: String,
    pub toggle_fullscreen: String,
    pub minimize_window: String,
    pub snap_left: String,
    pub snap_right: String,
    pub snap_center: String,
    pub switch_layout: String,
    pub reduced_motion: String,
    pub toggle_notifications: String,
    pub clear_notifications: String,
}

impl Default for ShortcutBindings {
    fn default() -> Self {
        Self {
            launcher: String::from("Super+Space"),
            overview: String::from("Super+Tab"),
            lock_screen: String::from("Super+L"),
            settings: String::from("Super+Comma"),
            reload_config: String::from("Super+Shift+R"),
            quick_settings: String::from("Alt+S"),
            close_window: String::from("Alt+W"),
            toggle_maximize: String::from("Alt+M"),
            toggle_fullscreen: String::from("Alt+F"),
            minimize_window: String::from("Alt+H"),
            snap_left: String::from("Alt+Left"),
            snap_right: String::from("Alt+Right"),
            snap_center: String::from("Alt+C"),
            switch_layout: String::from("Alt+L"),
            reduced_motion: String::from("Alt+R"),
            toggle_notifications: String::from("Alt+N"),
            clear_notifications: String::from("Alt+Shift+N"),
        }
    }
}

impl ShortcutBindings {
    pub(super) fn apply_kv(&mut self, key: &str, val: &str) {
        let Some(combo) = parse_shortcut_combo(val) else {
            return;
        };
        let canonical = combo.canonical_string();
        let target = match key {
            "launcher" => &mut self.launcher,
            "overview" => &mut self.overview,
            "lock_screen" => &mut self.lock_screen,
            "settings" => &mut self.settings,
            "reload_config" => &mut self.reload_config,
            "quick_settings" => &mut self.quick_settings,
            "close_window" => &mut self.close_window,
            "toggle_maximize" => &mut self.toggle_maximize,
            "toggle_fullscreen" => &mut self.toggle_fullscreen,
            "minimize_window" => &mut self.minimize_window,
            "snap_left" => &mut self.snap_left,
            "snap_right" => &mut self.snap_right,
            "snap_center" => &mut self.snap_center,
            "switch_layout" => &mut self.switch_layout,
            "reduced_motion" => &mut self.reduced_motion,
            "toggle_notifications" => &mut self.toggle_notifications,
            "clear_notifications" => &mut self.clear_notifications,
            _ => return,
        };
        *target = canonical;
    }

    pub(super) fn append_toml(&self, out: &mut String) {
        use std::fmt::Write as _;

        let _ = writeln!(out, "\n[shortcuts]");
        for (k, v) in [
            ("launcher", &self.launcher),
            ("overview", &self.overview),
            ("lock_screen", &self.lock_screen),
            ("settings", &self.settings),
            ("reload_config", &self.reload_config),
            ("quick_settings", &self.quick_settings),
            ("close_window", &self.close_window),
            ("toggle_maximize", &self.toggle_maximize),
            ("toggle_fullscreen", &self.toggle_fullscreen),
            ("minimize_window", &self.minimize_window),
            ("snap_left", &self.snap_left),
            ("snap_right", &self.snap_right),
            ("snap_center", &self.snap_center),
            ("switch_layout", &self.switch_layout),
            ("reduced_motion", &self.reduced_motion),
            ("toggle_notifications", &self.toggle_notifications),
            ("clear_notifications", &self.clear_notifications),
        ] {
            let _ = writeln!(out, "{k} = \"{}\"", escape_toml_string(v));
        }
    }
}
