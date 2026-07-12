---
id: viewer-p3-5
title: Shape tests
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 3 — `sl-prim` (pure Linden prim tessellation)
---

Context: [context/viewer.md](../context/viewer.md).

**P3.5. Shape tests.** Unit tests asserting non-degenerate counts and
correct face counts: box (6), cylinder, sphere, torus, hollow box (+ inner
face), cut prim (+ cut-edge faces). Deterministic-fixture style, as in the
`sl-mesh` tests. `cargo test -p sl-prim`.
