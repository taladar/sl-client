---
id: viewer-p3-4
title: Sweep & faces
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 3 — `sl-prim` (pure Linden prim tessellation)
---

Context: [context/viewer.md](../context/viewer.md).

**P3.4. Sweep & faces.** `volume.rs`: sweep the profile along the path and
assemble per-face vertices / normals / UVs / indices (`createSide` /
`createCap`, fan-triangulated caps), carrying the Linden `face_id`. A public
`tessellate` builds the swept vertex grid (each profile point placed into
each path frame), then emits one `PrimFace` per semantic profile face — the
i-th face becoming Linden face index `i`. Sides are a `count × path.total`
grid strip (grid positions, sweep-parameter/`tex_t` UVs, two-triangles-per-
cell indices, accumulated-then-normalized normals with the reference viewer's
closed-seam / pole normal wrapping); caps are a centre-vertex triangle fan
with planar UVs and one flat normal. Two documented MVP simplifications in the
road map's "fan-triangulated caps" scope: hollow caps are a filled centre fan
(no annulus triangulation), and a hollow inner wall is a single smoothed strip
(no flat-column doubling).
