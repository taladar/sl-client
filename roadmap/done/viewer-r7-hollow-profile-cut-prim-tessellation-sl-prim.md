---
id: viewer-r7
title: Hollow / profile-cut prim tessellation (sl-prim)
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Known rendering issues (to fix)
---

Context: [context/viewer.md](../context/viewer.md).

**R7. Hollow / profile-cut prim tessellation (`sl-prim`).** A heavily
hollowed, profile-cut cylinder (a curved "railing" wall) rendered see-through.
The original diagnosis (inner wall / cut-end caps wound wrong) was
**incorrect** — a winding analysis of the picked case (`profile_curve` circle,
`profile_hollow` 0.95, cut 0.04–0.51) showed the outer wall (+radial), inner
wall (−radial, faces into the hole), and both cut-end caps (`PROFILE_BEGIN` /
`PROFILE_END`, facing the removed arc) were all wound outward correctly. The
real culprit was the **path (top/bottom) caps**: `build_cap` always emitted a
centre-vertex triangle **fan**, but a hollow prim's cap ring is
`outer ++ reversed-inner`, so the inner-ring half of the fan wound backwards —
~half the cap triangles (measured: 37 `+Z` / 36 `−Z` on the top) were
back-face culled, and you saw straight through the cap into the hollow
interior (the "enclosed side vanishes"). Fixed by tessellating a **hollow cap
as an annulus** (`build_hollow_cap` / `hollow_cap_indices` in `sl-prim`
`volume.rs`), a faithful port of the reference viewer's `LLVolumeFace::
createCap` hollow branch: an area-based ear split that walks one pointer
forward from the outer-ring start and one backward from the inner-ring start,
emitting the non-back-facing triangle at each step (top / bottom windings
flipped) with no centre vertex — so the hole stays open and every triangle
winds outward. A solid (non-hollow) cap keeps the centre fan. The
`sl-client-bevy` `to_bevy_prim_mesh` bridge is unchanged (geometry-only).
Regression test `hollow_cut_cylinder_caps_wind_consistently` asserts every
path-cap triangle now winds `+Z` (top) / `−Z` (bottom) and that the cap is an
annulus (tri count = vert count − 2, no centre fan).
