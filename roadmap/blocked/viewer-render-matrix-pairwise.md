---
id: viewer-render-matrix-pairwise
title: The generated all-pairs sweep (opt-in)
topic: viewer
status: blocked
origin: test-harness plan (2026-08-30)
points: 2
blocked_by: [viewer-render-toggle-axis, viewer-render-hud-and-actor-axes]
---

Context: [context/testing.md](../context/testing.md).

`pairwise_cells(applies) -> Vec<ContextSet>`: every value pair of every
axis pair appears in at least one cell (about sixteen cells for five
ternary axes). Runs only under `SL_VIEWER_RENDER_MATRIX=full` with the
`gpu-full` nextest profile; the curated pairs stay the default. Logs what
it skipped so a truncated sweep never reads as complete coverage.
