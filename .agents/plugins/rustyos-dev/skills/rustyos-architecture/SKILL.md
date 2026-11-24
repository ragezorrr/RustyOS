---
name: rustyos-architecture
description: >-
  Use this skill when designing new RustyOS crates, Wayland protocols, system services,
  native apps, session/packaging integrations, or refactoring cross-crate boundaries
  in the RustyOS Linux desktop operating system.
---

# RustyOS Modular Linux Desktop Architecture

## 1. Originality Policy
- Every crate, protocol, binary, service, asset, and UX convention in RustyOS is **100% original**.
- **Never** copy code, identifiers, filenames, protocol names, or branding from CutefishOS or any other desktop environment.
- We adopt proven **architectural principles** of modern modular Wayland desktop environments (strict layer separation, unified custom toolkit, decoupled system-service framework, native Wayland compositor, thin native apps, XDG portals, and reproducible Linux packaging) implemented natively in Rust.

## 2. Six-Layer Modular Architecture

| Layer | RustyOS Modules | Responsibility & Boundary Rules |
| :--- | :--- | :--- |
| **1. Toolkit & Motion** | `crates/rusty-ui`, `crates/rusty-animation` | Shared rendering (`UiDrawList`/`UiPainter`, CPU `UiCanvas`, GPU `GlesSurfaceRenderer`), widgets, typography (`fontdue`), focus, clipboard, theme & motion tokens. **Zero** dependency on `smithay` or `zbus`. |
| **2. Window Policy** | `crates/rusty-wm` | Pure deterministic state machine for outputs, workspaces (`1..=9`), stacking, focus, floating placement, master-stack tiling, snap targets. **Headless & 100% unit-testable** without Wayland or GPU. |
| **3. Compositor & Protocols** | `crates/rusty-compositor`, `protocols/rustyos-liquid-glass-v1.xml` | Smithay 0.7 Wayland server (`CheckOnly`, `NestedWinit`, `DrmProbe`, `NativeDrmKms`), XWayland, input/gestures, damage tracking, shell chrome (top bar, dock, launcher, notifications, lock screen), Liquid Glass compositor shaders. |
| **4. System Services & IPC** | `crates/rusty-services`, `crates/rusty-ipc` | Least-privilege Linux adapters (`zbus` D-Bus for NetworkManager/BlueZ/UPower/logind/timedate/Polkit, PipeWire/Pulse audio, XDG Portals, storage, trash/file ops, updates) + bounded `0600` Unix socket IPC (`v1`, max `8 KiB`). Graceful fallback if any daemon is absent. |
| **5. Native Applications** | `apps/rusty-{files,settings,terminal,view,greeter,installer,polkit}` | Thin Wayland clients consuming `rusty-ui`, `rusty-services`, and `rusty-ipc`. Shared UI/service logic belongs in `crates/`, **never duplicated** across apps. |
| **6. Session & Distro Packaging** | `bin/rustyos-session`, `packaging/*`, `scripts/*` | Wayland session launcher, systemd user units, greetd integration, Polkit `.policy` XML, `xdg-desktop-portal` config, Plymouth/GRUB themes, Arch `PKGBUILD`, and `archiso` Live ISO builder. |

## 3. Architectural Rules for New Features
1. **Keep apps thin**: If two apps need the same widget, theme token, Wayland helper (e.g. `liquid_glass`), or Linux system query, extract it into `rusty-ui` or `rusty-services`.
2. **Pure core, impure shell**: Put state transitions, layout math, and validation in pure Rust modules with deterministic unit tests; keep Wayland/DRM/D-Bus I/O at the edges.
3. **Bounded I/O & Security**:
   - All subprocesses in `rusty-services` must use `run_command_bounded` with explicit timeouts.
   - All IPC messages in `rusty-ipc` must stay bounded (`<= MAX_IPC_MESSAGE_BYTES`) and versioned (`IPC_PROTOCOL_VERSION`).
   - Privileged actions (installer, updater) go through Polkit policies (`packaging/polkit/`) and dedicated helper binaries (`rustyos-installer-service`, `rustyos-update-helper`).
4. **Protocol discipline**: Custom Wayland protocols live in `protocols/rustyos-*.xml`, use double-buffered state committed atomically on `wl_surface.commit`, and define explicit protocol error enums.
5. **Packaging parity**: Any new binary, `.desktop` file, systemd unit, portal, or Polkit policy must be wired into `packaging/arch/PKGBUILD` and validated in `.github/workflows/ci.yml`.
