---
name: rustyos-token-efficiency
description: >-
  Use this skill when working on large multi-file tasks, refactors, or investigations in
  RustyOS to minimize context-window usage, avoid redundant file reads, and keep tool
  output compact.
---

# RustyOS Token-Efficiency Playbook

## 1. Search & Read Discipline
- **Map first, search second, read last**: Start from the crate map in `AGENTS.md`. Never run recursive `find` or `ls -R` on the workspace unless checking newly added files.
- **Exact symbol lookup**: Use `rg -n "<symbol>" crates/ apps/` (limiting paths to the relevant crate/app) to get exact file + line numbers before calling `read_file`.
- **Windowed reads only**: For files > 150 lines, always pass `StartLine` and `EndLine` to `read_file` (±40 lines around the target). Do not dump entire 1,500-line files (`rusty-wm/src/lib.rs`, `rusty-compositor/src/lib.rs`) into context.
- **Blacklist noise**: Never read `Cargo.lock`, `target/`, `.git/`, or binary files in `assets/`.

## 2. Compact Command Output
- Use `--message-format=short` for `cargo check` / `cargo clippy` during iteration so errors take 1 line instead of 15-line ASCII art blocks.
- Scope Cargo commands to the active crate first: `cargo test -p <crate> <test_name> -- --nocapture` only when debugging a failure; otherwise omit `--nocapture`.
- For passing runs or long logs, summarize or bound output (`| tail -n 30`). Never paste thousands of lines of `cargo test` output when everything passed.

## 3. Batching & Delegation
- **Parallelize independent calls**: Batch read-only `read_file` / `rg` calls in a single turn.
- **Progressive disclosure**: Only read a skill in `.agents/plugins/rustyos-dev/skills/` when the task actually enters that domain.
- **Subagent isolation**: Delegate broad codebase surveys or multi-crate audits to `research` or `rustyos-architect` so intermediate file reads stay out of the main conversation context.

## 4. Concise Editing & Reporting
- Make minimal, surgical `edit_file` calls rather than rewriting whole files.
- Avoid restating unchanged code or long tool outputs in responses; report only root cause, exact changes with clickable `file://` links, and verification status.
