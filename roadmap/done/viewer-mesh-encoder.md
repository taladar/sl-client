---
id: viewer-mesh-encoder
title: LLMesh encoder (inverse of the sl-mesh decoder)
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-mesh-model-upload
---

Context: [context/viewer.md](../context/viewer.md).

Done (2026-09-01): `sl-mesh/src/encode.rs` — `MeshModel` (the decoder's own
`Submesh` / `MeshSkin` / `PhysicsConvex` types, so a decoded asset can be
edited and written back), `encode_mesh` and the three block writers, with
every limit below enforced as a typed `MeshEncodeError` rather than a clamp.
Eleven unit tests pin the round trip through the decoder, the `NoGeometry`
marker, the influence stream's edge cases (four-influence = no terminator,
zero influences, an unnormalised weight, a joint the skin does not list), the
LOD chain, the convex decomposition and the header's write-order offsets.
`sl-test-assets::mesh` now builds a `Submesh` and calls the encoder; it writes
no LLSD of its own and no longer needs `flate2`.

Two deliberate departures from the plan above:

- **No `encode` feature.** The writer pulls in nothing the decoder does not
  already have (`sl-wire`, `flate2`), so a feature would have bought only
  `cargo hack` powerset time in the pre-commit hook. The module is
  unconditional.
- **A degenerate quantization domain writes `0` rather than dividing by its
  zero range** the way `writeModel` does — `0` dequantizes back to the domain
  minimum, which is the one value such a domain holds, where the reference
  casts an infinity. This is what lets a flat face (every vertex sharing a `z`)
  round trip at all.

`encode_physics_convex_block` **derives** its `Min`/`Max` from the hull points
rather than reading `PhysicsConvex`'s own, matching `Decomposition::asLLSD`:
a decomposition built by hand would otherwise have to keep a redundant
bounding box in step with its points, and a defaulted one would collapse every
point onto the origin.

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

Pulled forward (2026-09-01) by [[test-assets-rigged-mesh-encoder]]: the test
tiers need to *write* a rigged mesh, and `sl-test-assets::mesh` had started
hand-rolling the header and geometry blocks for its unit cube. Two encoders for
one format is one too many, and the fixture one would have been the wrong place
for the upload limits above. So the fixtures wait on this, and
`sl-test-assets::mesh` is rewritten to call it rather than write LLSD itself.

One detail confirmed against the reference while planning the fixtures: the
`0xFF` terminator is written only for a list **shorter** than four. After the
fourth influence `llvolume.cpp:2555-2612` sets its local `joint =
END_INFLUENCES` without consuming a byte, so a four-influence vertex carries no
terminator — and `sl_mesh::decode_weights` already matches that, which is what
makes the round trip a real contract rather than a self-consistent one.

Reference (Firestorm, read-only): `llmodel.cpp` (`writeModel` — the whole
encoder).

Builds on: `sl-mesh` (encode = inverse of the existing decode), `sl-llsd`,
`flate2`.
