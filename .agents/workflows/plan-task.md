# Plan Task Workflow

1.  Identify task ID and acceptance criteria (`ROADMAP.md` + tail of `TASKS.md`).
2.  Consult `ARCHITECTURE.md` (especially §7 Extension Points) and at most one
    relevant skill; run 1–2 batched `rg` lookups only if §7 lacks a needed hook.
3.  Before `APPROVE:`, do not read full `.rs` implementation files; read exact
    15–40 line `StartLine..EndLine` slices only after `APPROVE:` when editing.
4.  Classify changes as routine or approval-gated.
5.  If approval-gated, present options and stop.
6.  Otherwise return a short implementation plan and test strategy.
