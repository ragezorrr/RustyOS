# ADR-0080: Native `rusty-greeter` Display Manager Client for `greetd` (`RUSTYOS-136`)

- **Status:** accepted
- **Date:** 2026-11-21
- **Task:** `RUSTYOS-136`

## Context

RustyOS had an in-session `LockScreen` and `greetd` packaging scaffolding in `packaging/archiso`, but no native Wayland greeter client matching RustyOS's visual identity for pre-session login over `GREETD_SOCK`.

## Decision

1. **Add `apps/rusty-greeter`:**
   - Built directly on `crates/rusty-ui` (`UiDrawList`, `UiPainter`, `UiFontEngine`, `TextField`, `TextField::password`, `PushButton`, `ThemePalette`) and `smithay-client-toolkit` with zero new external dependencies.
   - Implement a zero-dependency native-endian length-prefixed JSON-IPC client in `apps/rusty-greeter/src/greetd.rs` (`CreateSession`, `PostAuthMessageResponse`, `StartSession`, `CancelSession` + `Success`, `AuthMessage`, `Error`).
   - Provide `--demo`, `--light`, `--dark`, `--user`, and `--session-cmd` flags for unprivileged local `--winit` verification.
2. **Packaging & CI:**
   - Register `apps/rusty-greeter` in root `Cargo.toml`, install `/usr/bin/rusty-greeter` in `packaging/arch/PKGBUILD`, and verify it in `.github/workflows/ci.yml`.

## Consequences

- RustyOS now has a unified visual experience from pre-session `greetd` login through the desktop session, with unit-tested `greetd` IPC framing and zero host systemd/DM side effects during development.
