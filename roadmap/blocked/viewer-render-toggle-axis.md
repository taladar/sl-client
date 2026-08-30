---
id: viewer-render-toggle-axis
title: The toggle axis — every effect must differ where it should and nowhere else
topic: viewer
status: blocked
origin: test-harness plan (2026-08-30)
points: 5
refs: [viewer-render-type-toggles, viewer-render-readback-tier]
blocked_by: [viewer-render-context-matrix, viewer-render-overrides-resource]
---

Context: [context/testing.md](../context/testing.md).

The A/B shape that localised R21 — render with an effect on and off and
assert they differ — as a matrix axis. `RenderToggle::{Glow,
UnderwaterFog, PreWaterPass, DynamicExposure, Tonemap, LocalLights,
Particles, Sky, Water, Shadows, HudParticles}` (the per-type masks of
[[viewer-render-type-toggles]] extend the enum later).

`toggle_should_differ(toggle, cx)` predicts the direction: fog differs only
with the eye under water, the pre-water pass only with an eye, glow only
with a glow prim in the cell, exposure/tonemap/sky always, local lights
only with an `ObjectLight`, particles only with a particle actor or
subject. `capture_ab(scene, cx, toggle)` renders both and the sweep
asserts `differing()` inside the silhouette (or the actor's) is above
`DIFFER_MIN_FRACTION` exactly when the table says so — a toggle that
changes pixels where it must not is a finding too. Directional checks
where they are cheap: fog tints toward the fog colour, glow adds a halo
ring outside the silhouette, the plate is brighter with the sky on.

Teeth: a toggle wired to nothing fails its "must differ" cell.
