# Performance Audit Workflow

1.  Reproduce a concrete symptom.
2.  Record hardware, kernel, driver, backend, resolution and refresh.
3.  Collect frame pacing, missed frames, event-loop stalls and relevant
    CPU/GPU metrics.
4.  Establish baseline before changing code.
5.  Identify bottleneck from evidence.
6.  Propose a minimal experiment.
7.  Ask approval if experiment requires architecture/system changes.
8.  Compare before/after and report regressions as well as improvements.
