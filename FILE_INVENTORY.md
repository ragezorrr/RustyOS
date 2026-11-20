# File Inventory

Актуальная карта файлов репозитория `RustyOS` (по состоянию на `RUSTYOS-128`).

## Правила, навыки и workflows (`.agents/`)
- `.agents/rules/approval-gates.md`
- `.agents/rules/graphics-performance.md`
- `.agents/rules/rust-quality.md`
- `.agents/rules/system-safety.md`
- `.agents/rules/token-efficiency.md`
- `.agents/skills/animation-engine/SKILL.md`
- `.agents/skills/desktop-shell/SKILL.md`
- `.agents/skills/gpu-rendering/SKILL.md`
- `.agents/skills/hardware-compatibility/SKILL.md`
- `.agents/skills/linux-graphics/SKILL.md`
- `.agents/skills/linux-integration/SKILL.md`
- `.agents/skills/performance-engineering/SKILL.md`
- `.agents/skills/release-engineering/SKILL.md`
- `.agents/skills/rust-engineering/SKILL.md`
- `.agents/skills/security/SKILL.md`
- `.agents/skills/testing-qa/SKILL.md`
- `.agents/skills/ui-ux-design/SKILL.md`
- `.agents/skills/wayland-compositor/SKILL.md`
- `.agents/skills/window-management/SKILL.md`
- `.agents/workflows/architecture-proposal.md`
- `.agents/workflows/implement-task.md`
- `.agents/workflows/performance-audit.md`
- `.agents/workflows/plan-task.md`
- `.agents/workflows/project-bootstrap.md`
- `.agents/workflows/release-checklist.md`

## Документация и ADR
- `AGENTS.md`
- `ARCHITECTURE.md`
- `DECISIONS.md`
- `DESIGN.md`
- `FILE_INVENTORY.md`
- `HARDWARE.md`
- `PERFORMANCE.md`
- `README.md`
- `ROADMAP.md`
- `SECURITY.md`
- `TASKS.md`
- `docs/DEVELOPMENT_ENVIRONMENT.md`
- `docs/GETTING_STARTED.md`
- `docs/HARDWARE_MATRIX.md`
- `docs/design/references/` (`context-menu.jpeg`, `control-center-overview.jpeg`, `desktop.jpeg`, `finder-window-dark.jpeg`, `system-settings-sidebar.jpeg`, `window-management.jpeg`)
- `docs/adr/0000-template.md`
- `docs/adr/0001-smithay-winit-prototype.md`
- `docs/adr/0002-window-management-architecture.md`
- `docs/adr/0003-native-drm-kms-backend.md`
- `docs/adr/0004-animation-engine.md`
- `docs/adr/0005-desktop-shell-and-ipc.md`
- `docs/adr/0006-system-services-integration.md`
- `docs/adr/0007-visual-shell-overlays.md`
- `docs/adr/0008-hardware-compatibility-matrix.md`
- `docs/adr/0009-distribution-and-session.md`
- `docs/adr/0010-direct-scanout-dmabuf.md`
- `docs/adr/0011-multi-monitor-drm-vrr.md`
- `docs/adr/0012-hidpi-fractional-scaling.md`
- `docs/adr/0013-low-latency-input-presentation.md`
- `docs/adr/0014-client-ux-idle-session-lock.md`

## Конфигурация workspace и CI
- `Cargo.toml`
- `Cargo.lock`
- `rust-toolchain.toml`
- `rustfmt.toml`
- `clippy.toml`
- `.github/workflows/ci.yml`
- `.gitignore`

## Крейты и приложения (`crates/`, `apps/`)
- `crates/rusty-wm/` (`Cargo.toml`, `src/lib.rs`)
- `crates/rusty-animation/` (`Cargo.toml`, `src/lib.rs`)
- `crates/rusty-ipc/` (`Cargo.toml`, `src/lib.rs`)
- `crates/rusty-services/` (`Cargo.toml`, `src/lib.rs`)
- `crates/rusty-ui/` (`Cargo.toml`, `src/{lib.rs, canvas.rs, geometry.rs, menu.rs, shadow.rs, theme.rs, traffic_lights.rs, window_frame.rs, window_header.rs}`)
- `crates/rusty-compositor/` (`Cargo.toml`, `src/`: `main.rs`, `lib.rs`, `config.rs`, `outputs.rs`, `frame_composer.rs`, `frame_timing.rs`, `input/`, `state/`, `shell_render/`, `context_menu/`, `surfaces/{mod,sdf,glyphs,xcursor,liquid_glass,window_clip,window_shadow,window_minimize}.rs`, `text.rs`, `icons.rs`, `icon_art.rs`, `launcher.rs`, `tray.rs`, `dbus_notifications.rs`, `telemetry.rs`, `screenshot.rs`, `xwayland.rs`)
- `apps/rusty-files/` (`Cargo.toml`, `src/`)
- `apps/rusty-installer/` (`Cargo.toml`, `src/`)
- `apps/rusty-settings/` (`Cargo.toml`, `src/`)
- `apps/rusty-terminal/` (`Cargo.toml`, `src/`)

## Сессия и упаковка (`bin/`, `packaging/`, `scripts/`)
- `bin/rustyos-session`
- `packaging/wayland-sessions/rustyos.desktop`
- `scripts/install.sh`
