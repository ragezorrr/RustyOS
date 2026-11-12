---
description: Use for Rust code, crates, APIs, ownership, errors and
  tests.
name: rust-engineering
---

# Rust Engineering

- Follow stable Rust and the repository MSRV.
- Prefer cohesive modules and explicit types.
- Keep unsafe minimal, justified, documented and tested.
- Avoid panics in runtime paths; propagate contextual errors.
- Avoid premature frameworks and unnecessary trait layers.
- Keep hot paths allocation- and lock-conscious; benchmark before
  optimizing.
- Review dependency maintenance, features and licenses.
- Run fmt, check, clippy and relevant tests.
- Ask before changing public APIs or workspace architecture.
