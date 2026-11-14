---
description: Use for test plans, validation, reproduction and acceptance
  reports.
name: testing-qa
---

# Testing and QA

- Convert requirements into observable acceptance criteria.
- Prefer deterministic unit tests for state machines and policy.
- Add integration tests for protocol/process boundaries.
- Keep hardware tests reproducible and environment-labelled.
- For compositor/renderer/Winit changes (when display exists), run `--winit --max-frames N` on
  both `WAYLAND_DISPLAY` (Wayland) and `WAYLAND_DISPLAY="" EGL_PLATFORM=""` (X11), plus with a
  real client (`--spawn ... rusty-files`), verifying zero `EGL_BAD_SURFACE` or fallback warnings.
- For UI/Visual changes, capture a live `--winit` screenshot in `scratch/` (without mutating
  `~/.config/rustyos/config.toml` permanently) and visually inspect the PNG via `read_file` against
  `docs/design/references/macos-27-golden-gate/` across Light and Dark modes (plus
  `reduce_transparency = true` fallback when touched).
- Inspect git diff and run targeted tests after changes.
- Report tests not run and why; never imply success without evidence.
