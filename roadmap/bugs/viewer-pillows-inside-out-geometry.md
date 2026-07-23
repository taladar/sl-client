---
id: viewer-pillows-inside-out-geometry
title: Pillows on the aditi test region render wrong — possibly inside out
topic: viewer
status: bugs
origin: user report during the 2026-07-23 aditi verification session
---

Context: [context/viewer.md](../context/viewer.md).

Some **pillows** on the aditi test region (mesh or sculpties — unconfirmed
which) render **inside out**: the inside faces are visible and the outside
faces are not — i.e. the triangle winding is inverted relative to the
back-face cull, so the camera sees through the near surface onto the
interior of the far one.

Investigation route:

- Pick one with `P` to get its kind (mesh vs sculpt) and the `asset` id,
  then fetch and decode that asset offline to inspect the geometry.
- If **sculpt**: suspects are the sculpt-mode handling (sphere / torus /
  plane / cylinder stitching), the `mirror` / `invert` sculpt flags
  (which flip winding), or normal generation — compare against the
  reference's `sculpt_mirror` / `sculpt_invert` handling
  (`llvolume.cpp`, `sculpt_calc_mesh_resolution` and the LOD stitcher).
- If **mesh**: winding order / normals for that particular asset
  (possibly a mesh with negative-scale instancing or flipped-normal
  submeshes the reference handles).
- Compare the same pillows in Firestorm on aditi for the expected look.
