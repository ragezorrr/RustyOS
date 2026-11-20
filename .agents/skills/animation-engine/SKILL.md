---
description: Use for transitions, easing, clocks and reduced-motion
  behavior.
name: animation-engine
---

# Animation Engine

- Use monotonic time and time-based progression, not fixed frame counts.
- Support interruption, reversal and cancellation.
- Keep updates cheap and non-blocking.
- Make reduced motion a first-class preference.
- Test at multiple refresh rates and under frame drops.
- Animations must not delay critical user actions.
- Window lifecycle transitions use snapshot-backed center-anchored scale+fade (`0.93 <-> 1.0`):
  `180 ms` `EaseOutQuart` on open (triggered on first client buffer commit) and `160 ms`
  `EaseOutCubic` on teardown, alongside `280 ms`/`250 ms` Dock minimize/restore; skip immediately
  when `reduced_motion` is enabled.
