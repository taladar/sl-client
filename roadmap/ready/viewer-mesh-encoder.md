---
id: viewer-mesh-encoder
title: LLMesh encoder (inverse of the sl-mesh decoder)
topic: viewer
status: ready
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-mesh-model-upload
---

Context: [context/viewer.md](../context/viewer.md).

Add an **`encode` feature to `sl-mesh`** that serialises an intermediate SL
model into the raw binary LLMesh asset — the exact inverse of the LOD / `skin` /
`physics_convex` / `physics_mesh` decoder `sl-mesh` already has. Reference
`sl-llsd` (binary LLSD) + `flate2` (zlib). Spec-exact from the reference
`llmodel.cpp` `writeModel`:

- **Header** is *uncompressed* binary LLSD: a map of section name →
  `{offset, size}`, offsets relative to the end of the header, in write order
  `skin`, `physics_convex`, then the LOD / `physics_mesh` blocks. Section names:
  `lowest_lod` / `low_lod` / `medium_lod` / `high_lod` (the last required, each
  requiring the next-higher present), `physics_mesh`, `physics_convex`, `skin`.
- **Each block** is binary LLSD then **zlib deflate at level 9**.
- **Quantization**: `Position` = 3×u16 across a per-model `PositionDomain`
  (Min/Max, written into every face); `Normal` = u16 over the fixed [-1, 1];
  `TexCoord0` = 2×u16 across a **per-face** `TexCoord0Domain`; `TriangleList` =
  u16 indices. Tangents are not transported.
- **`physics_convex`** = a `HullList` (u8 point-count per hull) plus
  u16-quantized `Positions` over its own domain, and `BoundingVerts` for the
  base hull.
- **Weights** (skin submesh): per vertex, up to four `(u8 joint, u16 weight)`
  pairs, with `0xFF` terminating a list shorter than four.
- **`skin` block**: `joint_names`, a flattened 16-float `bind_shape_matrix`,
  a per-joint `inverse_bind_matrix`, and the optional joint-override fields
  `alt_inverse_bind_matrix` / `lock_scale_if_joint_position` / `pelvis_offset`.

**Limits to enforce** (reject rather than emit an invalid asset): ≤8 faces per
model, u16 indices only, a lower LOD may not have more vertices than the LOD
above it, ≤110 joints, joint index ≤254, ≤256 hulls, and ≤256 points per hull.

Splitting the encoder out as a pure `sl-mesh` `encode` feature (mirroring the
existing decode) keeps the on-wire format math in the pure crate, beneath the
viewer-side floater and preview. The quantized-domain fields it emits feed the
client-side cost estimate ([[viewer-mesh-cost-estimate]]) and the upload
sequence ([[viewer-mesh-upload-sequence]]).

Reference (Firestorm, read-only): `llmodel.cpp` (`writeModel` — the whole
encoder).

Builds on: `sl-mesh` (encode = inverse of the existing decode), `sl-llsd`,
`flate2`.
