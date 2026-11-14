---
description: Minimize token and context usage without reducing
  engineering quality.
trigger: always_on
---

# Token Efficiency

- Read only files relevant to the current task.
- **Two-stage reading (Pre-APPROVE vs Post-APPROVE):** before user `APPROVE:`, read only `ROADMAP.md`, the last 30–40 lines of `TASKS.md`/`DECISIONS.md`, `ARCHITECTURE.md` §7 (Extension Points), and run 1–2 batched `rg` commands. Defer line-range reads of `.rs` files until after `APPROVE:`.
- **No blind `edit_file`:** if the exact lines were not read in the current window (or after `cargo fmt` / context compaction), read the exact 15–35 line target slice once immediately before editing; avoid retries.
- **Check `ARCHITECTURE.md` §7 first:** reuse the documented extension points and signatures instead of repeating `rg` lookups across `crates/` or `~/.cargo`.
- **Tail-only history reads:** for `TASKS.md` and `DECISIONS.md`, read only the last 30–40 lines needed for appending.
- Use targeted search before opening large files.
- Do not reread unchanged files or repeat completed checks without
  reason.
- Load only the skill needed now.
- Keep plans, progress notes and final reports concise.
- Do not duplicate architecture documentation in prompts or status
  files.
- Avoid unnecessary abstractions, crates, files and dependencies.
- Keep session summaries factual; never store hidden chain-of-thought.
- Batch related inspections where useful.
- Ask before broad audits or large-context refactors when not required.
- Never skip tests, security checks or measurements to save tokens.
