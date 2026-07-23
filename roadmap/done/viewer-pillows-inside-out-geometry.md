---
id: viewer-pillows-inside-out-geometry
title: Pillows on the aditi test region render wrong — possibly inside out
topic: viewer
status: done
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

**Resolution (2026-07-23).** The pillows are **sculpts** (picked live: two
sculpt objects sharing one sculpt-map asset). Root cause: the reference
viewer's JPEG2000 decoder copies rows **bottom-up** into `LLImageRaw`
(`llimagej2coj.cpp` fills from `y = height-1` down), and both
`sculptGenerateMapVertices` and the `createSide` winding assume that row
order; our `DecodedImage` is top-down and `sl-sculpt` sampled V directly
with the same winding, so every real-convention sculpt was built as its own
V-mirror — triangle orientation inverted relative to the back-face cull,
i.e. inside out. (Our local OpenSim sphere-sculpt OAR had been authored
V-flipped, matching the bug, which is why P9.1 verification looked fine.)

Fix in `sl-sculpt::tessellate`: positions sample the map at the flipped V
(`position(u, 1 - v)` centrally in the grid builder, so the degenerate-map
sphere placeholder flips too and stays outward); UVs keep the unflipped
grid V, matching the reference pairing. Also ported the reference's
`ss = 1.f - ss` horizontal *texture*-coordinate reversal for
invert-XOR-mirror sculpts. Unit tests pin orientation by signed volume:
a real-convention sphere map (north pole on the visible top row) and the
placeholder must be outward, the invert flag inward, mirror stays outward;
plus a reverse-U UV-mirror test. The local grid's
`bin/slclient-sculpt.oar` map was regenerated in the real convention (its
Asset.db blob replaced in place). Verified live on aditi: the pillows now
render right-side out (user-confirmed, two picked).
