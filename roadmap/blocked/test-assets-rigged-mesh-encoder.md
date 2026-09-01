---
id: test-assets-rigged-mesh-encoder
title: A rigged mesh fixture whose deformation is analytically checkable
topic: test
status: blocked
origin: asked while reviewing viewer-static-asset-library (2026-09-01)
points: 5
refs: [test-shared-test-assets, viewer-p17-2, viewer-r1]
blocked_by: [viewer-mesh-encoder]
---

Context: [context/testing.md](../context/testing.md).

`sl-test-assets::mesh` writes a single-submesh unit cube: no skin block,
no weights. So nothing in the workspace can produce a **rigged** mesh —
a mesh body, mesh clothing, or the mesh an animesh object wears — and the
skinning path ([[viewer-p17-2]], and the weight-normalisation finding of
[[viewer-r1]]) has no fixture of its own. It was tested against live-grid
content, which is not reproducible and not committed.

Fetching real rigged content off a grid was considered and rejected: a
Library asset's in-world permissions are not a redistribution licence
(unlike the LGPL-shipped files [[viewer-static-asset-library]] vendored),
a mesh body is multiple megabytes across four LODs, and — decisively — a
well-made body cannot support the assertion we actually want. A two-bone
cylinder with linearly ramped weights has a **closed-form** deformed
position per vertex, so an oracle can assert exact numbers rather than
"looks about right".

The encoder itself is **not** this task. Writing an LLMesh asset is
production work the viewer needs anyway for model upload, so it belongs
in `sl-mesh` behind its `encode` feature: [[viewer-mesh-encoder]], which
this waits on. `sl-test-assets::mesh` should end up *calling* that rather
than hand-rolling the header and geometry blocks the way it does today —
one format, one encoder.

What is left here is the fixtures. A well-formed one (the cylinder), and
— the part real content could not provide — deliberately **pathological**
ones, because what breaks skinning code is never average content:

- weights that do not sum to one (the [[viewer-r1]] case: Bevy does not
  renormalise, so an unnormalised rig drags vertices toward the origin);
- a vertex with **zero** influences (chased 2026-09-01: `sl-mesh` reports
  an empty influence list and `sl_client_bevy::meshes::pack_influences`
  applies the reference's fallback — full weight on joint 0 — so the two
  agree in effect; the fixture pins that they keep agreeing);
- a four-influence vertex, so the no-terminator case is exercised by an
  asset and not only by reading the C++;
- an influence naming a joint index the skin does not have;
- Bento joint names, and a joint-position override via
  `alt_inverse_bind_matrix` / `pelvis_offset`;
- per-LOD `Weights` streams that disagree.

Animesh needs no new asset class: an animesh object is a rigged-mesh prim
carrying the animated-object flag, with the region sending
`ObjectAnimation`. So the catalogue gains a rigged prim and an animesh
prim, the latter playing the chest-twist motion from
[[test-fake-grid-animation-assets]].

Acceptance: every fixture round trips through `sl_mesh::decode_skin` /
`decode_lod`; the cylinder's deformed vertices match the closed-form
expectation; the catalogue shows a rigged prim and an animesh prim.
