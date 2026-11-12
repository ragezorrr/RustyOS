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
