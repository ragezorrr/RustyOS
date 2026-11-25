//! Embedded rasterized SVG icons & illustrations for `rusty-settings` (`RUSTYOS-146`).
//!
//! Source SVGs (BSD-3-Clause, see `assets/icons/settings/LICENSE`) live in
//! `assets/icons/settings/svg/` and are pre-rasterized at `@2x` into
//! `assets/icons/settings/png/` so `rusty-settings` needs zero runtime SVG
//! dependencies while rendering crisp anti-aliased icons.

use crate::state::SettingsPage;

const PNG_ABOUT: &[u8] = include_bytes!("../../../assets/icons/settings/png/about.png");
const PNG_ACCOUNTS: &[u8] = include_bytes!("../../../assets/icons/settings/png/accounts.png");
const PNG_APPEARANCE: &[u8] = include_bytes!("../../../assets/icons/settings/png/appearance.png");
const PNG_BATTERY: &[u8] = include_bytes!("../../../assets/icons/settings/png/battery.png");
const PNG_BLUETOOTH: &[u8] = include_bytes!("../../../assets/icons/settings/png/bluetooth.png");
const PNG_CURSOR: &[u8] = include_bytes!("../../../assets/icons/settings/png/cursor.png");
const PNG_DATETIME: &[u8] = include_bytes!("../../../assets/icons/settings/png/datetime.png");
const PNG_DEFAULTAPPS: &[u8] = include_bytes!("../../../assets/icons/settings/png/defaultapps.png");
const PNG_DISPLAY: &[u8] = include_bytes!("../../../assets/icons/settings/png/display.png");
const PNG_DOCK: &[u8] = include_bytes!("../../../assets/icons/settings/png/dock.png");
const PNG_FONTS: &[u8] = include_bytes!("../../../assets/icons/settings/png/fonts.png");
const PNG_GLOBE: &[u8] = include_bytes!("../../../assets/icons/settings/png/globe.png");
const PNG_GLOBE_GRID: &[u8] = include_bytes!("../../../assets/icons/settings/png/globe-grid.png");
const PNG_ICONTHEME: &[u8] = include_bytes!("../../../assets/icons/settings/png/icontheme.png");
const PNG_KEYBOARD: &[u8] = include_bytes!("../../../assets/icons/settings/png/keyboard.png");
const PNG_MENUBAR: &[u8] = include_bytes!("../../../assets/icons/settings/png/menubar.png");
const PNG_NETWORK: &[u8] = include_bytes!("../../../assets/icons/settings/png/network.png");
const PNG_NOTIFICATIONS: &[u8] =
    include_bytes!("../../../assets/icons/settings/png/notifications.png");
const PNG_SOFTWAREUPDATE: &[u8] =
    include_bytes!("../../../assets/icons/settings/png/softwareupdate.png");
const PNG_SOUND: &[u8] = include_bytes!("../../../assets/icons/settings/png/sound.png");
const PNG_TOUCHPAD: &[u8] = include_bytes!("../../../assets/icons/settings/png/touchpad.png");
const PNG_WALLPAPER: &[u8] = include_bytes!("../../../assets/icons/settings/png/wallpaper.png");

const PNG_LIGHT_MODE: &[u8] = include_bytes!("../../../assets/icons/settings/png/light_mode.png");
const PNG_DARK_MODE: &[u8] = include_bytes!("../../../assets/icons/settings/png/dark_mode.png");
const PNG_AUTO_MODE: &[u8] = include_bytes!("../../../assets/icons/settings/png/auto_mode.png");

/// Returns the embedded `@2x` PNG bytes for a `SettingsPage` sidebar glyph.
#[must_use]
pub(crate) const fn sidebar_icon_png(page: SettingsPage) -> &'static [u8] {
    match page {
        SettingsPage::Wifi => PNG_NETWORK,
        SettingsPage::Bluetooth => PNG_BLUETOOTH,
        SettingsPage::Network => PNG_GLOBE_GRID,
        SettingsPage::About => PNG_ABOUT,
        SettingsPage::Battery => PNG_BATTERY,
        SettingsPage::Users => PNG_ACCOUNTS,
        SettingsPage::SoftwareUpdate => PNG_SOFTWAREUPDATE,
        SettingsPage::Appearance => PNG_APPEARANCE,
        SettingsPage::Wallpaper => PNG_WALLPAPER,
        SettingsPage::DockDesktop => PNG_DOCK,
        SettingsPage::MenuBar => PNG_MENUBAR,
        SettingsPage::Notifications => PNG_NOTIFICATIONS,
        SettingsPage::Display => PNG_DISPLAY,
        SettingsPage::Sound => PNG_SOUND,
        SettingsPage::Keyboard | SettingsPage::Shortcuts => PNG_KEYBOARD,
        SettingsPage::Mouse | SettingsPage::CursorStyle => PNG_CURSOR,
        SettingsPage::Touchpad => PNG_TOUCHPAD,
        SettingsPage::DateTime => PNG_DATETIME,
        SettingsPage::DefaultApps => PNG_DEFAULTAPPS,
        SettingsPage::Language => PNG_GLOBE,
        SettingsPage::Fonts => PNG_FONTS,
        SettingsPage::IconTheme => PNG_ICONTHEME,
    }
}

/// Returns embedded `@2x` PNG bytes for Appearance theme preview illustrations (`Light`/`Dark`/`Auto`).
#[must_use]
pub(crate) const fn theme_illustration_png(choice: &str) -> Option<&'static [u8]> {
    match choice.as_bytes() {
        b"light" => Some(PNG_LIGHT_MODE),
        b"dark" => Some(PNG_DARK_MODE),
        b"auto" => Some(PNG_AUTO_MODE),
        _ => None,
    }
}

/// Decodes an embedded PNG sprite into `(width, height, rgba_bytes)`.
#[must_use]
pub(crate) fn decode_png(bytes: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    rusty_services::image_decode::decode_png_rgba(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_sidebar_and_theme_svg_pngs_decode_cleanly() {
        for &page in &SettingsPage::SIDEBAR {
            let decoded = decode_png(sidebar_icon_png(page));
            assert!(decoded.is_some());
            if let Some((w, h, rgba)) = decoded {
                assert_eq!(w, 64);
                assert_eq!(h, 64);
                assert_eq!(rgba.len(), w * h * 4);
            }
        }
        for key in ["light", "dark", "auto"] {
            let decoded = theme_illustration_png(key).and_then(decode_png);
            assert!(decoded.is_some());
        }
    }
}
