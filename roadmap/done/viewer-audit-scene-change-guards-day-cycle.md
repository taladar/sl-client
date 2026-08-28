---
id: viewer-audit-scene-change-guards-day-cycle
title: The scene crate's write-on-change guards were reasoned about under a pinned sky
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 3
refs: [viewer-audit-probe-ambient-multiply, viewer-audit-tonemap-legacy-sky, viewer-audit-scene-live-daycycle-fixture]
---

Context: [context/viewer.md](../context/viewer.md).

`day_position` (`sl-viewer-world-scene/src/sky.rs:1988`) returns a pinned value
only when `SL_VIEWER_SKY_DAY_POSITION` is set; otherwise it reads
`SystemTime::now()`. So on any real grid the blend position advances
continuously and `blended_sky_settings` returns a different frame every frame —
which defeats every float-equality guard downstream.

The expensive one is `drive_terrain_lighting`
(`sl-viewer-world-scene/src/terrain.rs:106`):

```text
if current.0 == lighting { return; }
current.0 = lighting;
for (_id, material) in materials.iter_mut() { material.lighting = lighting; }
```

The comparison is float equality on `Vec3` colours, so it never holds and
**every region's `TerrainMaterial` is marked modified and its bind group
re-prepared, every frame**. The same reasoning error recurs at `sky.rs:739`,
`:1247`, `:1458` and `water.rs:361`; `drive_sky`'s doc claim that it "writes
nothing" under a static environment is true only under the screenshot harness.

The crate already owns the correct shape — `snap_shadow_direction`
(`sky.rs:198`) quantises for exactly this reason. Apply it to the colour and
scalar guards too.

This is one of three scene defects that are **correct under the screenshot
harness and wrong on a live grid** (with [[viewer-audit-probe-ambient-multiply]]
and [[viewer-audit-tonemap-legacy-sky]]). That is a coherent blind spot, so the
fix should land with a fixture that advances `day_position` between samples —
see [[viewer-audit-scene-live-daycycle-fixture]].

## Fixed (2026-08-28)

Fixed one level up from the guards, at the single continuously-advancing input
they all derive from: `day_position` (`sky.rs`) now rounds the day down to
`DAY_POSITION_STEPS` (32768) sampling cells per day, so `blended_sky_settings`
returns a **bit-identical** frame across the frames whose true position falls in
one cell — and every float-equality guard below it (the sky, cloud, star and
water material compare-then-`get_mut`, and `drive_terrain_lighting`'s
`Assets::iter_mut` over every region's terrain material) holds between steps
instead of missing on every frame. The pure
`quantised_day_position(now, day_length, day_offset)` carries the arithmetic;
the `SL_VIEWER_SKY_DAY_POSITION` pin is honoured exactly (a pinned position is
already stable), so the screenshot harness renders what it did before.

Quantising the shared input rather than each guard's colours and scalars was the
choice because the guards' inputs are whole `ShaderType` param blocks
(`SkyParams` alone is twenty fields), because a colour grid would still leave
the sun drifting per frame, and because a future guard downstream of the sky
inherits the fix instead of having to repeat it.

The step is a fraction of the day, not a number of seconds: what has to stay
imperceptible is how far the sun moves per step, and that is `360° / steps`
whatever the region's day length — a region running a five-minute day rotates
its sun 48× faster than Second Life's four-hour one and gets 48× more frequent
samples out of the same grid. At 32768 steps a step turns the sun 0.011°, below
the ~0.014° the shadow-caster direction is *already* snapped to
(`snap_shadow_direction`, `1 / SHADOW_MAP_SIZE` radians), so nothing visible
steps that was not already stepping; a four-hour day resamples every 0.44 s.

Seven tests. Five in `sky.rs` for the quantiser — stability inside a cell
(asserted on bit patterns, which is exactly what the guards compare), exactly
one step per cell, the step scaling with a short day length, the day step
staying finer than the shadow-direction snap, and the wrap/offset arithmetic.
Two in `terrain.rs` sample the guard's real input across a **moving** day cycle
(the four ported presets keyframed across a four-hour day, via a
`terrain_lighting` helper split out of `drive_terrain_lighting`): one second of
60 fps frames at dawn now resolves at most 3 distinct terrain lightings, and
resolved 60 — one per frame — before the fix; five minutes later the ground is
relit, so the cycle is quantised, not frozen.

The stale doc claims went with it: `drive_sky`, `drive_terrain_lighting`, the
cloud and water compare-then-`get_mut` comments, and the module header now say
what holds on a live grid rather than only under the pinned harness.

**Not addressed here:** the scene *app* fixture
([[viewer-audit-scene-live-daycycle-fixture]]) stays open — its (a) is now
covered by these pure tests, but its (b) `GlobalAmbientLight` convergence and
(c) legacy-sky tonemap assertions belong with the two defects they test, which
are still open ([[viewer-audit-probe-ambient-multiply]],
[[viewer-audit-tonemap-legacy-sky]]).
