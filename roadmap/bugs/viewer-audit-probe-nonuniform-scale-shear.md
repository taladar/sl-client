---
id: viewer-audit-probe-nonuniform-scale-shear
title: A reflection probe's non-uniform volume scale shears its sampling frame
topic: viewer
status: bugs
origin: static code audit (2026-08-26)
points: 3
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-world-scene/src/probes.rs:861` — the probe holder spawns with
`Transform::from_scale(probe.volume_scale())` and
`rotation: sample_rotation(world_rotation)`, where `sample_rotation` is
`world_rotation.inverse()`.

The doc immediately below quotes the composition it must undo —
`*original_transform * Affine3A::from_quat(self.rotation)` — and
`original_transform` is the **full affine including scale**. So the linear part
is `R * S * R^-1`: a shear for any non-uniform `S`. Only the rotation is
cancelled; the scale is folded into the reflection sampling frame as well as the
influence volume. Reflected directions bend anisotropically.

The doc's own phrasing ("leaving the `Transform` — and therefore the influence
volume — still tracking the prim") shows the scale was reasoned about as *only*
the volume. Worst on mirrors, where `hero_volume_scale` floors a thin panel to
`(2, 3, 1)` (`:1779`).

The existing test (`:2029`) composes `Quat`s only, so it structurally cannot see
this — widening it to `Affine3A` is the one assertion that catches it.
