---
id: viewer-p8-3
title: Stitch tests
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 8 — `sl-sculpt` (sculpt-texture → geometry)
---

Context: [context/viewer.md](../context/viewer.md).

**P8.3. Stitch tests.** Unit tests per stitch type (counts; seam and pole
vertices are shared, not duplicated). `cargo test -p sl-sculpt`. 14 tests:
exact per-type vertex counts (plane `(N+1)²` > cylinder `N(N+1)` > torus `N²`
> sphere `N²-N+2`), face integrity (parallel arrays, in-range whole triangles,
unit normals, finite positions), degenerate + truncated fallback, and the
mirror X-reflection.
