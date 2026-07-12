---
id: viewer-p9-1
title: Sculpt objects
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 9 — Sculpt rendering in the viewer
---

Context: [context/viewer.md](../context/viewer.md).

**P9.1. Sculpt objects.** For `SculptOrMeshKey::Sculpt(texture_key)`,
fetch + decode that sculpt map **through the same Phase 6 `TextureManager` /
`TextureStore`** (request the texture id, react to its `TextureDecoded`, read
the decoded `DecodedTexture` pixels as geometry input — reusing the store's
fetch / off-thread-decode / disk-cache, not a fresh inline decode); the object
stays in the "waiting on asset" state as a mesh does. Feed the pixels + type
into `sl_sculpt`, convert with `to_bevy_prim_mesh`, and texture via Phase 6.
**Done — mirroring the P7 mesh pipeline exactly, but keyed on the shared
texture store.** A sculpted prim is classified `Sculpt` (already done since
P5.1) and routed through `build_object_geometry`: it requests its sculpt map
through the shared `TextureManager` (the same store the Phase 6 face textures
use), and either stitches its face now (if the map is already decoded) or
parks a pending sculpt build. A new `apply_object_sculpts` system reads the
same `TextureDecoded` stream as `apply_prim_textures` — keying off a *pending
sculpt build* rather than a parked face material, so the two consumers never
contend — and on decode stitches the map with `tessellate_sculpt` into a
single-face `PrimMesh`, spawning its face child (textured from `TextureEntry`
slot 0) exactly as a plain prim's. The two deferred-build paths (mesh asset,
sculpt map) were unified into one `PendingGeometry` enum on `TrackedObject`,
and the prim / sculpt face spawn loop factored into one shared helper
`spawn_prim_faces` (`build_prim_faces` and `build_sculpt_faces` differ only in
how they produce the `PrimMesh`). New `sl-client-bevy` re-exports:
`tessellate_sculpt` (the
`sl_sculpt::tessellate` aliased so it does not collide with `sl_prim`'s
`tessellate`) + `SculptParams` / `SculptStitch`, and the `sl-sculpt` dep — the
sculpt mirror of P4's prim re-exports. Verified live on OpenSim (a provisioned
sphere-sculptie prim renders as a textured sphere).
