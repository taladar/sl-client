---
id: viewer-r4
title: Prim rendering fidelity
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Known rendering issues (to fix)
---

Context: [context/viewer.md](../context/viewer.md).

**R4. Prim rendering fidelity.** Two independent fixes; the "too large /
misplaced / flat" perception was a real bug, distinct from the TE-placement
gap. Live-verified against populated aditi builds (a crosshair pick tool,
`pick_object` in `objects.rs`, press `P`, reports the object under the centre
of the screen — full id, mesh/sculpt asset, scale, world-scale, shape — so a
wrongly rendered object is identified by *looking* at it; plus a
`SL_VIEWER_LOG_OBJECTS` diagnostic that flags region-sized / sky objects).

- **Linkset children inherited the root's scale (the "too large / stretched"
  cause).** Every object entity carried `object.scale`, and a linkset child
  parents to the root entity — so Bevy composed `root_scale × child_scale`,
  oversizing children *and* shearing them (a non-uniform parent scale on a
  rotated child). Second Life prims each have an absolute size and never
  inherit the root's scale. Fixed by moving the scale off the object entity
  (now position/rotation only) onto a per-object **geometry holder** child
  ([`geometry_transform`]) that only that object's own faces hang off, so the
  scale reaches the geometry but never the child prims. Empty OpenSim has no
  linksets, so it never showed there.
- **Per-face `TextureEntry` placement.** `scale_s` / `scale_t` (repeat),
  offset, and rotation are applied as the material's `uv_transform`
  (`texture_face_uv_transform` in `sl-client-bevy`, a port of the reference
  viewer's `llface.cpp` `xform` about the face centre), covering prim, sculpt,
  and mesh faces. Also fixed the **upside-down prim textures**: `sl-prim` UVs
  are OpenGL bottom-up, so `to_bevy_prim_mesh` now flips V (`1 - v`) to match
  `to_bevy_mesh` / wgpu's top-down sampling. (bump / shiny / glow / fullbright
  stay deferred — non-goals.)
