---
id: viewer-p4-1
title: to_bevy_prim_mesh
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 4 — `sl-client-bevy` conversion
---

Context: [context/viewer.md](../context/viewer.md).

**P4.1. `to_bevy_prim_mesh`.** Add `to_bevy_prim_mesh(&PrimFace) -> Mesh`
and `to_bevy_prim_meshes(&PrimMesh) -> Vec<Mesh>` (TriangleList; POSITION +
optional NORMAL + UV_0 + `Indices::U32`), an analogue of `to_bevy_mesh`. Add
the `sl-prim` dependency; re-export the conversion and the `sl_prim` types the
viewer needs (`PrimShape` aliased `PrimShapeFloat` so it does not collide with
`sl_proto`'s quantized rez-params `PrimShape`). `sl-prim` is a pure geometry
crate with no store/fetcher, so — unlike `sl-mesh` / `sl-texture` — it has no
`sl-client-tokio` runtime counterpart and this stays a `sl-client-bevy`-only
change. The CHANGELOG is `git-cliff`-generated from commits, so no manual
entry was added.
