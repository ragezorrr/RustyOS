# ADR-0083: Native `FileChooser` & `Settings` XDG Portals and Qt 5/6 Theme Synchronization (`RUSTYOS-154`)

- **Status:** Accepted
- **Date:** 2026-11-21
- **Decision ID:** `D-097`

## Context
Third-party browsers (Chromium, Firefox) and GTK/Qt applications use `xdg-desktop-portal` (`org.freedesktop.impl.portal.FileChooser` and `org.freedesktop.impl.portal.Settings`) for file dialogs and system `color-scheme` discovery, while native Qt 5/6 applications read `~/.config/kdeglobals` and `~/.config/qt{5,6}ct/qt{5,6}ct.conf`. Previously, RustyOS only exported `Screenshot` and `ScreenCast` on `org.freedesktop.impl.portal.desktop.rustyos` and synchronized only GTK 3/4 `settings.ini`.

## Decision
1. **`apps/rusty-files` File Picker Mode:**
   - Added `--pick-file`, `--pick-files`, `--pick-folder`, `--save-file <default_name>`, and `--title <title>` flags (`FilePickerMode`) with a dedicated bottom confirmation bar (`Cancel` / `Open` / `Select` / `Save` + `Save As` filename display) that outputs chosen paths to `stdout` and exits with status `0` (`1` on cancellation).
2. **`org.freedesktop.impl.portal.FileChooser` & `org.freedesktop.impl.portal.Settings` in `crates/rusty-services/src/portal.rs`:**
   - Registered `FileChooserPortalIface` (`OpenFile`, `SaveFile`, `SaveFiles`, `version = 3`) and `SettingsPortalIface` (`Read`, `ReadOne`, `ReadAll` for `org.freedesktop.appearance` `color-scheme` `1=Dark`/`2=Light` and `contrast`, `version = 2`) on `/org/freedesktop/portal/desktop` and declared them in `packaging/portals/{rustyos.portal, rustyos-portals.conf}`.
3. **Qt 5/6 Synchronization in `crates/rusty-services/src/theme_sync.rs`:**
   - Added `ThemeSyncService::write_qt_settings_to_dir`, `render_kdeglobals_ini`, and `render_qtct_conf` so `ThemeSyncService::apply_async` atomically updates `kdeglobals`, `qt5ct/qt5ct.conf`, and `qt6ct/qt6ct.conf` alongside GTK 3/4.

## Consequences
- Zero new Cargo dependencies.
- All portal-aware browsers and GTK/Qt applications open RustyOS's native `rusty-files` dialog and follow Light/Dark appearance changes automatically.
