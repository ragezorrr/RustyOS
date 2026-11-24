# RustyOS — Project Agent Constitution

> **Token-first rule:** Keep this file lean. Load detailed runbooks on demand from `.agents/plugins/rustyos-dev/skills/` instead of scanning the repo blindly.

## 1. Originality & Identity (Non-Negotiable)
- **RustyOS** is an independent, original Linux desktop OS written in Rust (`edition = "2024"`, MSRV `1.99`).
- **Never** copy code, identifiers, filenames, protocol names, or branding from CutefishOS or any other OS/DE. Architectural ideas (e.g., clean separation of toolkit, compositor, system services, session, apps, portals) may inspire **original** RustyOS designs only.

## 2. Workspace Map (Use Before Running `find`/`ls`)
- **Core Crates (`crates/`)**:
  - `rusty-ui` — Shared UI toolkit (`UiCanvas`, `UiDrawList`/`UiPainter`, `GlesSurfaceRenderer`, `ThemePalette`, `ShadowElevation`, `WindowHeader`, widgets, `fontdue` text, clipboard, focus).
  - `rusty-app` — Shared Wayland/SCTK + `calloop` application host (`RustyApp`, `AppConfig`, `AppContext`, automatic GLES2/SHM presentation, canonical `rustyos-liquid-glass-v1` client bindings).
  - `rusty-animation` — Refresh-rate independent motion engine (`MotionTokens`, `Easing`, interruptible transitions, `reduced_motion`).
  - `rusty-wm` — Pure deterministic WM policy (outputs, workspaces `1..=9`, stacking, focus, floating & master-stack tiling, snap targets).
  - `rusty-compositor` — Smithay 0.7 Wayland compositor (`NestedWinit`, `DrmProbe`, `NativeDrmKms`, `CheckOnly`), shell rendering, input, XWayland, `rustyos-liquid-glass-v1`.
  - `rusty-ipc` — Bounded (`8 KiB`), zero-dep Unix socket (`0600`) versioned (`v1`) IPC.
  - `rusty-services` — Least-privilege Linux service adapters (`zbus` D-Bus, PipeWire/Pulse, logind, polkit, portals, UPower, NetworkManager/BlueZ, storage, file manager, updates, `run_command_bounded`).
- **Native Apps (`apps/`)**: `rusty-files`, `rusty-settings`, `rusty-terminal`, `rusty-view`, `rusty-greeter`, `rusty-installer`, `rusty-polkit`.
- **Protocols & System**: `protocols/rustyos-liquid-glass-v1.xml`, `bin/rustyos-session`, `assets/` (Inter, JetBrainsMono, icons, wallpapers), `packaging/` (Arch `PKGBUILD`, `archiso`, systemd, polkit, portals, plymouth, grub), `scripts/`.

## 3. Hard Rust & Quality Invariants
- `unsafe_code = "deny"` across workspace; keep FFI strictly isolated where already required.
- Zero-panic discipline: `clippy::unwrap_used`, `expect_used`, `panic`, `dbg_macro`, `todo`, `unimplemented` are warned and `-D warnings` fails CI. Always propagate `Result`/`Option` or graceful fallbacks.
- `clippy::all` + `clippy::pedantic` clean; cognitive complexity <= 25, args <= 7 (`clippy.toml`).
- Formatting: `cargo fmt --all` (`max_width = 100`, 4 spaces, Unix LF).
- Subprocesses in services must use `rusty_services::run_command_bounded` with explicit timeouts.

## 4. Token-Efficiency Protocol
1. **Locate before reading**: Use targeted `rg -n` to find exact symbols/lines; read only narrow `StartLine..EndLine` slices. Never read `Cargo.lock` or `target/`.
2. **Scoped verification first**: Run `cargo check -p <crate> --message-format=short` and `cargo test -p <crate>` while iterating; run full `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and `cargo run -p rusty-compositor -- --check` before final handoff.
3. **Minimal diffs & compact output**: Edit only what is required for the task; summarize results concisely with clickable `file://` links.
4. **Progressive skills**: Activate the matching skill in `.agents/plugins/rustyos-dev/skills/` (`rustyos-architecture`, `rustyos-design-system`, `rustyos-bugfix`, `rustyos-token-efficiency`) or delegate heavy tasks to the `rustyos-architect` subagent.
