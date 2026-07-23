---
id: viewer-stretch-global-axis-object
title: World-frame stretch still grows the object along its local axis
topic: viewer
status: bugs
origin: edit-gizmo live review (2026-07-23)
refs: [viewer-transform-gizmos, viewer-build-grid-options]
---

Context: [context/viewer.md](../context/viewer.md).

In the stretch tool with the **world** grid frame, the bounding box now
behaves as specified (axis-aligned to the mode's frame; a face drag changes
the box on that axis alone, honouring stretch-both-sides), but the **object
itself** still grows along its own local axis: the drag folds onto the
prim's nearest local scale axis (`gizmos.rs` `apply_face_scale` /
`nearest_local_axis`, delta ÷ alignment), so a rotated prim visibly
stretches along its tilted axis and the re-fitted box jumps on release.
The user reports the reference viewer genuinely stretches the object along
the chosen global axis.

Open questions to settle with a live Firestorm side-by-side on the local
grid (rotate a cube ≥ 30°, grid mode World, stretch a face handle, watch
the prim and its `scale` before/after):

- What exactly does the reference do to the PRIM's scale components for an
  oblique world-axis stretch? (A prim has only per-local-axis scale — an
  oblique world stretch cannot shear — so whatever it does is some fold;
  this session's reading of `llmanipscale.cpp` `stretchFace` found a
  `nearestAxis` single-component fold, but that reading was disputed —
  re-verify what `stretchFace` vs `adjustTexturesByScale` actually own.)
- How does the reference's manipulator box orient per grid mode
  (`llselectmgr.cpp` builds `mSelectionBBox` on the root prim's rotation —
  reconcile that with the observed world-aligned box)?
- Whether multiple local axes should share the delta (weighted by their
  projections onto the drag axis) instead of the single nearest axis — and
  what that does to the perpendicular extents.

Fix `apply_face_scale` (and the snap-ladder labelling that assumes the
single-axis fold) to match whatever the side-by-side establishes.
