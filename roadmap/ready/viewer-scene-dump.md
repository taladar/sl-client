---
id: viewer-scene-dump
title: Emit the shared scene-dump JSON beside the frames
topic: viewer
status: ready
origin: Firestorm cross-check harness plan (2026-09-01)
points: 5
refs: [viewer-render-pixel-oracle, viewer-screenshot-fixed-resolution]
---

Context: [context/testing.md](../context/testing.md).

Pixels say two viewers disagree. They do not say why, and the four
commonest causes look identical in an image: a prim in the wrong place, a
texture that resolved to a different asset id, a mesh stuck at a coarser
LOD, and a material that never arrived. Firestorm now writes a structured
dump next to its frames (`fstestscenedump.cpp`); this viewer must write
the same document or the comparison has nothing to compare.

Write `scene.json` into `--screenshot-dir` when the capture finishes, or
to `--scene-dump <path>`. The schema carries a `schema_version` — version
1 today — so a mismatch is an error rather than a confusing diff:

- `context` — viewer name, channel, version, grid, region name/id/handle,
  timestamp
- `camera` — origin and focus in both region and global coordinates, the
  three axes, FOV, aspect, near/far
- `environment` — day position, sun and moon direction, sun rotation, sky
  and water names
- `objects` — per object in the *agent's own region* (not neighbours, whose
  caching differs between viewers): id, local id, pcode, region position,
  rotation, scale, and per-face texture id, colour, scale/offset/rotation,
  bump, shiny, fullbright, glow, material ids; plus is-mesh, mesh id,
  is-sculpt, current LOD, flexible, light
- `avatars` — id, self flag, position, rotation, fully-loaded state
- `render` — draw distance, quality level, shadow and reflection-probe
  detail, max texture resolution, LOD factor

The units are the contract: region-local SL metres, Z-up, exactly as the
camera flags already take. Whatever this viewer stores internally, the
dump is in reference coordinates, so a diff never has to know which viewer
wrote which file.

Prefer sorting `objects` by id before writing. An unstable iteration order
turns every dump into a diff against itself and teaches whoever reads the
comparison to ignore it.

Refs [[viewer-render-pixel-oracle]] (the projection maths that already
turns a region position into a framing pixel) and
[[viewer-screenshot-fixed-resolution]].
