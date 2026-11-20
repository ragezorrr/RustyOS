//! Visual theme, palettes, and traffic-light colors for RustyOS.

/// Supported UI theme modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeMode {
    #[default]
    Light,
    Dark,
    Auto,
}

impl ThemeMode {
    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "dark" => Self::Dark,
            "auto" | "system" => Self::Auto,
            _ => Self::Light,
        }
    }

    #[must_use]
    pub const fn is_dark(self, system_dark: bool) -> bool {
        match self {
            Self::Light => false,
            Self::Dark => true,
            Self::Auto => system_dark,
        }
    }
}

/// The three window-control buttons in a window titlebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrafficLightKind {
    Close,
    Minimize,
    Maximize,
}

impl TrafficLightKind {
    /// Active base color in RGBA.
    #[must_use]
    pub const fn active_color(self) -> [u8; 4] {
        match self {
            Self::Close => [0xEE, 0x5A, 0x6A, 255],    // #EE5A6A
            Self::Minimize => [0xF4, 0xB6, 0x3F, 255], // #F4B63F
            Self::Maximize => [0x3C, 0xC6, 0x8A, 255], // #3CC68A
        }
    }

    /// Color for unfocused / inactive windows.
    #[must_use]
    pub const fn inactive_color() -> [u8; 4] {
        [110, 114, 122, 255]
    }

    /// Pressed color (darkened by 20% on click down).
    #[must_use]
    pub const fn pressed_color(self) -> [u8; 4] {
        let [r, g, b, a] = self.active_color();
        [
            ((r as u16 * 8) / 10) as u8,
            ((g as u16 * 8) / 10) as u8,
            ((b as u16 * 8) / 10) as u8,
            a,
        ]
    }

    /// Ink color for the hover microglyph (`x`, `-`, `+`).
    #[must_use]
    pub const fn glyph_ink() -> [u8; 4] {
        [0, 0, 0, 150]
    }
}

/// Standard 7 accent colors for RustyOS.
pub const ACCENT_PALETTE: [[u8; 4]; 7] = [
    [0, 122, 255, 255],   // 0: Blue #007AFF
    [255, 59, 48, 255],   // 1: Red #FF3B30
    [52, 199, 89, 255],   // 2: Green #34C759
    [175, 82, 222, 255],  // 3: Purple #AF52DE
    [255, 45, 85, 255],   // 4: Pink #FF2D55
    [255, 149, 0, 255],   // 5: Orange #FF9500
    [142, 142, 147, 255], // 6: Grey #8E8E93
];

/// Precomputed color scheme for Light or Dark theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemePalette {
    pub is_dark: bool,
    pub accent: [u8; 4],
    pub window_bg: [u8; 4],
    pub sidebar_bg: [u8; 4],
    pub card_bg: [u8; 4],
    pub card_active_bg: [u8; 4],
    pub divider: [u8; 4],
    pub text_primary: [u8; 4],
    pub text_secondary: [u8; 4],
    pub text_muted: [u8; 4],
}

impl ThemePalette {
    #[must_use]
    pub fn new(is_dark: bool, accent_idx: usize) -> Self {
        let accent = ACCENT_PALETTE[accent_idx % ACCENT_PALETTE.len()];
        if is_dark {
            Self {
                is_dark: true,
                accent,
                window_bg: [0x1C, 0x1C, 0x1D, 255],
                sidebar_bg: [0x26, 0x26, 0x28, 255],
                card_bg: [0x2C, 0x2C, 0x2D, 255],
                card_active_bg: [0x3E, 0x3E, 0x41, 255],
                divider: [0x38, 0x38, 0x3A, 255],
                text_primary: [0xF5, 0xF5, 0xF7, 255],
                text_secondary: [0x98, 0x98, 0x9D, 255],
                text_muted: [0x63, 0x63, 0x66, 255],
            }
        } else {
            Self {
                is_dark: false,
                accent,
                window_bg: [0xF5, 0xF6, 0xFA, 255],
                sidebar_bg: [0xEC, 0xED, 0xF1, 255],
                card_bg: [0xFF, 0xFF, 0xFF, 255],
                card_active_bg: [0xD9, 0xDA, 0xDD, 255],
                divider: [0xE5, 0xE5, 0xEA, 255],
                text_primary: [0x1D, 0x1D, 0x1F, 255],
                text_secondary: [0x6E, 0x6E, 0x73, 255],
                text_muted: [0x86, 0x86, 0x8B, 255],
            }
        }
    }
}
