---
description: Rust implementation, API and dependency quality rules.
globs: "**/*.rs, Cargo.toml, Cargo.lock, rust-toolchain.toml"
trigger: glob
---

# Rust Quality

- Prefer safe Rust; isolate and justify every `unsafe` block.
- Keep modules cohesive and APIs explicit.
- Handle errors intentionally; avoid runtime panics.
- Avoid unnecessary clones, allocations and locks in hot paths; measure
  first.
- Do not add dependencies without need and license/maintenance review.
- Keep event-loop callbacks short and non-blocking.
- Add tests for behavior and regressions.
- Run fmt, check, clippy and relevant tests.
- Public API changes require approval.
- **No third-party branding or codenames in code:** Do NOT use names of third-party OSes, desktop environments, or external codenames (including `macos`, `macOS`, `cutefish`, `golden gate`, `apple`) in file names, crate names, types, functions, variables, test names, constants, doc comments (`///`), code comments, `Cargo.toml` descriptions, or commit messages. Use original RustyOS terminology (`RustyOS`, `Liquid Glass`, `Neutral Glass`, `System Palette`, `Traffic Lights`, `TopBar`, `Dock`, `Control Center`) and describe visual/motion behaviors using technical properties (radii, blur/translucency, spring physics, token hex values).
