---
id: viewer-camera-no-pull-in-transparent-prims
title: Camera collision skips fully-transparent prims
topic: viewer
origin: camera-collision / static-prim-colliders discussion (2026-08-15)
refs: [viewer-physics-static-prim-colliders, viewer-perf-camera-collision-broad-phase]
---

Context: [context/viewer.md](../context/viewer.md).

Camera collision currently pulls the third-person eye in at **any** prim in the
shared avian index (`build_static_colliders`), because a prim is usually
visually solid — including phantom prims, which are opaque and should occlude.
But a prim can be **fully transparent** (a 100 %-alpha face, an invisiprim, a
clear water pane): the camera pulling in at something you can see straight
through looks like a bug.

Idea: make `collide_camera` ([`crate::camera`]) skip prims that are effectively
**invisible** — either give those prims no collider, or a distinct collision
layer the camera ray masks out (the `NonSolid` layer already demonstrates the
mechanism), or carry a per-collider "opaque" flag the ray filter consults.

## Implications to explore (why this is only an idea)

- **What counts as transparent?** A whole prim at alpha 0, or per-**face**
  transparency (a prim with one clear face and five opaque)? A collider is
  per-object, so per-face transparency cannot be expressed by excluding the
  whole prim — the camera would clip through the opaque faces.
- **Alpha modes.** Alpha-blend vs alpha-mask vs fully-transparent texture vs a
  transparent *material* (legacy `TextureEntry` alpha, GLTF base-colour alpha,
  the fullbright/alpha-mask flags). The "is this opaque" test must match what
  the renderer actually draws.
- **Invisiprims / alpha 0 tricks.** Builders deliberately use alpha-0 prims as
  invisible collision/sit surfaces; skipping them might change where the camera
  sits vs. the reference.
- **Cost.** Deciding opacity per prim (and keeping it current as textures /
  materials stream in and change) adds work to the collider build / a per-frame
  reclassification — weigh against the visual win.
- **Reference behaviour.** Check whether Firestorm's object-partition camera
  pushback actually skips transparent geometry before matching it.

Deliberately parked in `ideas/` — may not be worth it; explore the above before
promoting to `ready/`.
