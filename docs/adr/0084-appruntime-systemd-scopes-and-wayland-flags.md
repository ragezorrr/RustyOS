# ADR-0084: Application Runtime Isolation (`systemd --user` Scopes), Chromium/Electron Wayland Flag Injection & XDG Directory/MIME Bootstrap (`RUSTYOS-155`)

- **Status:** Accepted
- **Date:** 2026-11-21
- **Decision ID:** `D-098`

## Context
Previously, applications launched via Dock, Spotlight, TopBar, or keyboard shortcuts in `rusty-compositor` were spawned directly inside the compositor's cgroup (`rustyos-session`), meaning a runaway child tree could compete with the compositor or leave orphaned background workers. Additionally, Chromium and Electron applications required manual command-line flags (`--ozone-platform-hint=auto --enable-features=WaylandWindowDecorations`) when `.desktop` entries did not read environment variables, and fresh user accounts lacked pre-populated XDG directories (`user-dirs.dirs`) and desktop-specific MIME associations (`rustyos-mimeapps.list`).

## Decision
1. **`crates/rusty-services/src/appruntime.rs`:**
   - Added `is_chromium_or_electron_binary` and `inject_wayland_browser_flags(program, args)` to append `--ozone-platform-hint=auto` and `--enable-features=WaylandWindowDecorations` for Chromium/Chrome/Brave/Vivaldi/Edge/Electron/VS Code/Discord/Slack binaries unless an `--ozone-platform*` flag is already present.
   - Added `sanitize_systemd_app_id`, `is_systemd_user_scope_available`, and `build_scoped_launch_command(program, args, seq, use_systemd_scope)` wrapping spawned apps in `systemd-run --user --scope --quiet --collect --unit=app-rustyos-<appid>-<seq>.scope -- <program> <args...>` when `systemd --user` is active, with automatic fallback to direct `Command::new(program)` when unavailable.
   - Added `ensure_xdg_user_directories`, `ensure_rustyos_mimeapps_list`, and `bootstrap_user_session_environment` to idempotently create `Desktop`, `Documents`, `Downloads`, `Music`, `Pictures`, `Pictures/Screenshots`, `Videos`, `~/.config/user-dirs.dirs`, and `~/.config/rustyos-mimeapps.list` (`inode/directory=org.rustyos.files.desktop;`).
2. **Compositor & Packaging Integration:**
   - Wired `build_scoped_launch_command` into `LauncherState::spawn_resolved_program` (`crates/rusty-compositor/src/launcher.rs`) and `bootstrap_user_session_environment` into `RustyCompositorState::new` (`crates/rusty-compositor/src/state/mod.rs`).
   - Added `packaging/applications/rustyos-mimeapps.list` and installed it to `/usr/share/applications/rustyos-mimeapps.list` in `packaging/arch/PKGBUILD`.

## Consequences
- Every desktop application runs in its own isolated `app-rustyos-<appid>-<seq>.scope` cgroup when `systemd --user` is active and falls back transparently in containers/tests.
- Chromium and Electron browsers and IDEs open natively on Wayland with proper window decorations out of the box.
- Fresh user sessions always have standard XDG folders and associate folders with `org.rustyos.files.desktop`.
