---
id: test-assets-rigged-mesh-encoder
title: A rigged mesh fixture whose deformation is analytically checkable
topic: test
status: done
origin: asked while reviewing viewer-static-asset-library (2026-09-01)
points: 5
refs: [test-shared-test-assets, viewer-p17-2, viewer-r1]
blocked_by: [viewer-mesh-encoder]
---

Done (2026-09-01): `sl-test-assets::rigged` — `cylinder` /
`cylinder_mesh_asset` (a two-bone cylinder, 8 segments by 4 rings, weights
ramping linearly with height on `["mTorso", "mChest"]`), `pathological_rig`
(one quad, one pathology per vertex: weights summing to 0.6, no influences at
all, the full four influences, and an influence naming a joint the skin does
not list), `bento_override_rig` (Bento bones plus `alt_inverse_bind_matrix` /
`pelvis_offset` / `lock_scale_if_joint_position`) and
`lod_weight_mismatch_mesh_asset` (one geometry, disagreeing per-LOD `Weights`).

`mChest` is the cylinder's **upper** joint on purpose: it is the joint the
chest-twist motion of [[test-fake-grid-animation-assets]] rotates, so one asset
serves both catalogue prims — the animesh one bends where the plain rigged one
stands at its bind pose.

Five unit tests in `sl-test-assets` pin the round trips and the closed-form
deformation; the oracle derives its expectation from the vertex's *height*
rather than from the weights it just read back, so a wrong weight stream cannot
satisfy it by agreeing with itself. A sixth in `sl-client-bevy` runs the
pathological rig through the real `to_bevy_rigged_mesh` **after** an asset round
trip — the three tests already there hand it weights built in memory, and the
codec/packer boundary is exactly where the two can silently disagree.

The fake grid gained `PrimFixture::animated_mesh` (the extended-mesh
`ANIMATED_MESH_ENABLED` flag), `SceneFixtures::object_animations` with an
`ObjectAnimationFixture` pushed as `ObjectAnimation` at the end of the arrival
burst, and the catalogue entries `rigged-mesh` and `animesh-cylinder`, both
shaped by `RIGGED_MESH_ASSET`.

Not done here: `sl-viewer-world-scene`'s own `rigged_strip` still builds its
own `Submesh` + `MeshSkin` for the no-grid render scene. It is the same *kind*
of fixture from a second generator, but folding it into this one would rebless
a committed render baseline, which is not this task's business.

[[viewer-mesh-encoder]] cleared the blocker (2026-09-01): `sl_mesh::encode`
writes the header, the geometry blocks, the `skin` block and the convex
decomposition, and `sl-test-assets::mesh` already calls it for the unit cube.
What it deliberately does *not* police is the content, so every pathological
fixture below is writable: an unnormalised weight set, a vertex with no
influences, a four-influence vertex (which carries no terminator), and an
influence naming a joint the `skin` block does not list.

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

The encoder itself was **not** this task. Writing an LLMesh asset is
production work the viewer needs anyway for model upload, so it landed in
`sl-mesh` as [[viewer-mesh-encoder]], which this waited on;
`sl-test-assets::mesh` now *calls* it rather than hand-rolling the header and
geometry blocks — one format, one encoder.

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
