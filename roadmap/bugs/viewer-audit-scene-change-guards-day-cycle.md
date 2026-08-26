---
id: viewer-audit-scene-change-guards-day-cycle
title: The scene crate's write-on-change guards were reasoned about under a pinned sky
topic: viewer
status: bugs
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
