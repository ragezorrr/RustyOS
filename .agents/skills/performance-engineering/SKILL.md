---
description: Use for frame pacing, latency, CPU/GPU load and memory
  optimization.
name: performance-engineering
---

# Performance Engineering

- Establish reproducible baseline before optimization.
- Track frame interval distributions and missed/late frames, not only
  average FPS.
- Measure event-loop stalls and input-to-display path.
- Change one variable at a time where possible.
- Record hardware, driver, kernel, display mode, backend and commit.
- Reject optimizations that cause material regressions elsewhere.
- Never remove correctness/security checks to improve benchmark numbers.
