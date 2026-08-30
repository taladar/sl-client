---
id: viewer-render-pixel-oracle
title: A pixel-oracle library — decidable verdicts over a captured frame
topic: viewer
status: ready
origin: test-harness plan (2026-08-30)
points: 3
refs: [viewer-render-readback-tier]
---

Context: [context/testing.md](../context/testing.md).

The readback tests carry their oracles as private test helpers with a
hard-coded green subject / red other. Promote them into
`sl-client-bevy-viewer/src/pixel_oracle.rs` — pure functions over a
`Frame { pixels, width, height }` (non-square for HUD cells), CPU-only,
each with a synthetic-frame teeth test:

`dominant(pixel) -> Option<Marker>`, `Patch`/`presence`,
`CellVerdict::{Translucent, Solid, Missing, Background}`,
`read_cell(frame, at, subject, other)`, `Silhouette { centre, radius }`,
`coverage`, `centroid`, `mean_luminance` (Rec. 709 on linearised values;
once tonemap is in the rig the target is display-referred, so add
`Frame::linear_pixel`), `differing(a, b, within)`, `mirror_symmetry`,
`health -> FrameHealth`, `tinted_toward`, `classify_ground_water_sky`.

Constants are documented with their justification: the existing
`SATURATION`, `CHANNEL_PRESENT`, `PATCH_RADIUS`/`PATCH_MAJORITY`,
`REFRACTED_MIN_SLOPE`, `EDGE_ON_MARGIN`; new `MIN_COVERAGE` (0.25 — a unit
sphere covers ~0.79 of its projected disc; tolerates coarse LOD and still
catches "drew nothing"), `MAX_FRAME_COVERAGE` (0.90, "filled the screen"),
`LUMINANCE_ORDER_RATIO` (1.5 — midday vs midnight sunlight differ by more
than 2× on a lit plate), `DIFFER_MIN_FRACTION` (5 % of silhouette pixels
each beyond 8/255 — quantisation cannot fake it), `SYMMETRY_MIN` (0.90).

The existing readback tests are rewritten on the library without changing
what they assert; the teeth tests draw a green disc under a red strip and
require `Translucent` at the overlap, `Solid` in the disc, `Missing` on the
strip, `Background` elsewhere.
