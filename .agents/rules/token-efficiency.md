---
description: Minimize token and context usage without reducing
  engineering quality.
trigger: always_on
---

# Token Efficiency

- Read only files relevant to the current task.
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
