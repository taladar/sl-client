---
id: viewer-audit-probe-ambient-multiply
title: suppress_global_ambient multiplies an absolute producer and decays it geometrically
topic: viewer
status: bugs
origin: static code audit (2026-08-26)
points: 2
refs: [viewer-audit-scene-change-guards-day-cycle]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-world-scene/src/probes.rs:201` — `suppress_global_ambient` runs
`ambient.brightness *= probe_ambient_scale();` in `PostUpdate`, against a
producer that writes an **absolute** value: `sky.rs:787` compares `to_bits()`
specifically to avoid dirtying `GlobalAmbientLight`.

The product is never what the sky asked for, so the sky rewrites every frame and
the resource is dirty every frame regardless of the guard. And `drive_sky`
early-returns when no sky frame resolves (`sky.rs:711`) — while it does, the
multiply runs unopposed and ambient decays geometrically toward zero.

This is currently correct **only by accident**: `probe_ambient_scale()` defaults
to exactly `0.0` (`:193`), which is idempotent. Set the documented
`SL_VIEWER_PROBE_AMBIENT_SCALE=0.5` A/B knob and ambient decays to nothing over
a few frames.

Fix: apply the suppression as part of computing the absolute ambient value, not
as a multiplicative post-pass. One of the three "right under the harness, wrong
on a live grid" scene defects — see
[[viewer-audit-scene-change-guards-day-cycle]].
