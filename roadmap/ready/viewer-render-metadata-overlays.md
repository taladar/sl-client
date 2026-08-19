---
id: viewer-render-metadata-overlays
title: Render-metadata debug overlays
topic: viewer
status: ready
origin: Advanced/Develop menu survey (2026-07-22)
refs: [viewer-debug-render-beacons, viewer-render-type-toggles]
---

Context: [context/viewer.md](../context/viewer.md).

The Develop → Render Metadata overlay family — per-toggle world-space debug
draws the reference offers, filed as one task with a checklist (each is a
small gizmo layer over data we already hold; implement incrementally,
highest-value first):

- [ ] Bounding boxes; avatar hitboxes
- [ ] Normals / tangents
- [ ] Physics shapes (avian3d colliders)
- [ ] LOD info (current LOD per object); triangle count
- [ ] Lights (local light radii); reflection probes + probe updates
- [ ] Joints / collision skeleton (extends the existing avatar-geometry
      debug logging into a visual overlay)
- [ ] Raycast (last pick ray + hit)
- [ ] Wind vectors
- [ ] Texture anim / texture priority / texture area; texel-density heatmap
      (needs a debug material swap)
- [ ] Update-type flashes (colour objects by full/terse/cached update — the
      classic interest-list debugging aid)
- [ ] Octree/partition nodes — only if our spatial structure warrants it

Markers for *invisible* things (sound sources etc.) are
[[viewer-debug-render-beacons]]; hiding whole draw classes is
[[viewer-render-type-toggles]]. This task is the metadata gizmos, a
Develop-menu section listing them, and a shared "debug overlay" gizmo layer
(Bevy gizmos) they all draw into.

Reference (Firestorm, read-only): `pipeline.h` `RENDER_DEBUG_*`,
`menu_viewer.xml` (Render Metadata).

## Parity-audit addendum (2026-08-19)

The parity audit found these Develop ▸ Render Metadata / Rendering
overlays missing from the checklist: **Shadow Frusta, Occlusion,
Render Batches, Face Area, Sculpt, Texture Size, Texture Density
modes (None / Current / Desired / Full), GLTF Nodes,
particles-metadata**, plus from the Rendering cluster: **Axes,
Tangent Basis, Selected Texture Info Basis / Selected Material Info**
(menu_viewer.xml L4414–4726 and L4728–4947).
