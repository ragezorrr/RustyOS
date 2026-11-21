# ADR-0079: Third-Party Application Theme Synchronization (`GTK 3/4` & `gsettings`, `RUSTYOS-135`)

- **Status:** accepted
- **Date:** 2026-11-21
- **Task:** `RUSTYOS-135`

## Context

Changing `Appearance` (`Light / Dark`), `IconTheme`, or `CursorStyle` in `rusty-settings` previously updated only native RustyOS windows and compositor chrome, leaving third-party GTK 3/4, Electron, and browser windows out of sync.

## Decision

1. **Add `crates/rusty-services/src/theme_sync.rs`:**
   - `ThemeSyncConfig` (`prefer_dark`, `icon_theme`, `cursor_theme`, `cursor_size`, `font_name`).
   - Pure `merge_gtk_settings_ini` that preserves unrelated sections and user keys while updating `gtk-application-prefer-dark-theme`, `gtk-icon-theme-name`, `gtk-cursor-theme-name`, `gtk-cursor-theme-size`, and `gtk-font-name`.
   - `ThemeSyncService::write_gtk_settings_to_dir` (atomic temp-file + rename for `gtk-3.0/settings.ini` and `gtk-4.0/settings.ini`) and `ThemeSyncService::apply_async` (non-blocking background thread also invoking `gsettings set org.gnome.desktop.interface color-scheme/icon-theme/cursor-theme` with silent graceful fallback).
2. **Compositor Hook:**
   - Invoke `ThemeSyncService::apply_async` in `RustyCompositorState::apply_user_config` when appearance changes and a real `config_path` is active (avoiding side effects in unit tests).

## Consequences

- Third-party GTK/Wayland/XWayland applications follow RustyOS Light/Dark, icon, and cursor changes without blocking the compositor event loop or overwriting custom user INI entries.
