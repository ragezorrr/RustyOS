---
name: rustyos-bugfix
description: >-
  Use this skill when diagnosing, isolating, fixing, or writing regression tests for bugs
  in RustyOS (Wayland/Smithay compositor, GLES/SHM rendering, input/focus, D-Bus services,
  packaging, or Rust clippy/compiler/runtime failures).
---

# RustyOS Systematic Bug-Hunting & Verification Playbook

## 1. Disciplined 5-Step Workflow
1. **Reproduce & Pinpoint**: Capture the exact failing test, compiler error, or runtime symptom. Never guess before reading the relevant lines.
2. **Isolate Root Cause**: Trace the invariant violation (state machine, coordinate space, Wayland commit order, lifetime/borrow, or D-Bus/subprocess error path) rather than patching symptoms.
3. **Minimal, Idiomatic Fix**:
   - Preserve `unsafe_code = "deny"` and zero-panic guarantees (no `.unwrap()`, `.expect()`, `panic!()`).
   - Never silence warnings with broad `#[allow(...)]` unless structurally justified.
   - Keep cognitive complexity <= 25 and function args <= 7 (`clippy.toml`).
4. **Regression Test**: Add a focused, deterministic unit test in the affected crate (`rusty-wm`, `rusty-ui`, `rusty-ipc`, `rusty-animation`, `rusty-services`, `rusty-compositor`, or app `tests.rs`) that fails before the fix and passes after.
5. **Layered Verification**: Verify crate-first, then workspace-wide.

## 2. RustyOS-Specific Bug Checklists

### Wayland / Smithay / Compositor (`rusty-compositor`, `rusty-wm`)
- **Double-buffered state**: Ensure Wayland surface & `rustyos_surface_glass_v1` changes apply atomically on `wl_surface.commit`, not mid-configuration.
- **Coordinate spaces**: Verify logical vs physical (scale factor) coordinates, work-area insets (top bar / dock), and output rotation (`0|90|180|270`).
- **Damage tracking**: When visual state changes (animations, shadows, glass blur, focus ring), verify damage rectangles cover the full dirty region (including shadow bleed).
- **Focus & grabs**: Check pointer leave/enter transitions, popup/context-menu dismissal on outside click or `Escape`, and window close while focused/minimized.

### UI & Rendering (`rusty-ui`, `apps/*`)
- **Pixel format & clipping**: Verify `PixelFormat::Bgra8888` channel order, stride bounds, and `push_clip`/`pop_clip` balance in `UiPainter`.
- **UTF-8 safety**: Always use `clamp_char_boundary`, `prev_word_boundary`, `next_word_boundary` for text editing/slicing — never slice `&str` by raw byte index.
- **Animation convergence**: Verify `tick()` returns `false` once settled so the event loop does not busy-spin at 100% CPU.

### Linux Services & IPC (`rusty-services`, `rusty-ipc`)
- **Deadlock / hang prevention**: Never call `.output()` directly on unbounded external commands; use `rusty_services::run_command_bounded`.
- **Missing daemons**: Handle absent D-Bus services (NetworkManager, BlueZ, UPower, PipeWire, Polkit) gracefully without panicking.
- **IPC framing**: Enforce `MAX_IPC_MESSAGE_BYTES` (`8 KiB`) and strict input parsing.

## 3. Verification Commands (Fast -> Full)
Run fast scoped checks while iterating:
```bash
cargo check -p <crate> --message-format=short
cargo test -p <crate> <test_filter>
cargo clippy -p <crate> --all-targets -- -D warnings
```
Run full CI-equivalent gates before concluding:
```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p rusty-compositor -- --check
```
If shell scripts, desktop entries, or Polkit policies changed:
```bash
sh -n bin/rustyos-session
bash -n packaging/arch/PKGBUILD scripts/install.sh scripts/rustyos-install.sh
xmllint --noout packaging/polkit/org.rustyos.updater.policy packaging/polkit/org.rustyos.installer.policy
```
