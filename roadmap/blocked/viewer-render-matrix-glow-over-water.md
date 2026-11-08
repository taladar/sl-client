---
id: viewer-render-matrix-glow-over-water
title: Render matrix — a glowing prim over and under the water surface
topic: viewer
status: blocked
origin: viewer-render-context-matrix (2026-11-08) — curated pair not built
points: 2
refs: [viewer-render-context-matrix]
blocked_by: [viewer-render-rig-glow-pass]
---

Context: [context/testing.md](../context/testing.md).

One of the context matrix's planned curated pairs: a glowing subject
staged on the waterline by the `Eye` axis. Glow is built from the alpha
mask the materials write, and the sea is a translucent surface drawn over
the submerged half. So the question is whether the halo follows the glowing
face on both sides of the surface: no halo lost under the water, and none
added to the water itself where it reflects or refracts the face.

- Register the `glow-prim` subject from [[viewer-render-rig-glow-pass]]
  with `applies` including `Eye`, and add the curated cells to
  `render_matrix.rs`'s `CURATED` (eye under, grazing and over) within the
  R2 budget.
- Expectation: glow halo pixels outside the silhouette with the subject,
  none without it. Write the rule for the submerged half (seen through the
  surface) into `expectation()` rather than as per-cell data.
