---
id: viewer-occlusion-culling
title: Occlusion culling
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
refs: [viewer-statistics-floater]
---

Context: [context/viewer.md](../context/viewer.md).

Occlusion culling: stop drawing (and stop spending fetch priority on)
objects hidden behind others — indoors and in dense builds this is the
reference's biggest scene-cost lever after LOD. The reference uses hardware
occlusion queries over its octree; Bevy has gained **GPU occlusion culling**
(two-phase depth pyramid) in recent releases — evaluate that first
(enabling + measuring it on our scenes) before considering any custom
CPU-side scheme.

Scope: enable/integrate the chosen mechanism for prims, meshes and avatars;
verify no popping artifacts with our alpha handling; feed "was occluded"
into the fetch-priority computation (`render_priority.rs`) so hidden
objects also fetch at lower priority; expose the on/off setting and a
culled-count stat ([[viewer-statistics-floater]]).

Reference (Firestorm, read-only): `llvieweroctree` /
`llspatialpartition` occlusion queries, `RenderOcclusionTimeout`.

Builds on: the scene mirror and the render-priority system.
