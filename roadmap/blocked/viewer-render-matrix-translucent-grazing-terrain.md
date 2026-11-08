---
id: viewer-render-matrix-translucent-grazing-terrain
title: Render matrix — a translucent prim seen at a grazing angle over terrain
topic: viewer
status: blocked
origin: viewer-render-context-matrix (2026-11-08) — curated pair not built
points: 2
refs: [viewer-render-context-matrix]
blocked_by: [viewer-render-hud-and-actor-axes]
---

Context: [context/testing.md](../context/testing.md).

The "translucent grazing over terrain" curated pair: `prim-translucent-box`
resting on terrain, with the eye just above the ground. Terrain is drawn by
its own material (`TerrainMaterialPlugin`) and writes depth. So at a
grazing angle the translucent faces nearest the ground are the ones a
depth or ordering fault drops first, the way the rack's faces vanished
against the depth-writing sea.

- Needs a terrain stage (`ActorKind::Terrain` / a terrain composition from
  [[viewer-render-hud-and-actor-axes]]) that `render_stage.rs::stage` can
  place under a subject, and a grazing eye defined against the ground
  height rather than the sea.
- Expectation: the subject's points read `SeeThrough` with a red terrain
  composition behind them; points edge-on to the eye are reported, as the
  `Eye` axis already does.
