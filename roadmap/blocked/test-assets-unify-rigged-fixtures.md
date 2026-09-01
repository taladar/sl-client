---
id: test-assets-unify-rigged-fixtures
title: Two generators make a rigged fixture; there should be one
topic: test
status: blocked
origin: noticed while doing test-assets-rigged-mesh-encoder (2026-09-01)
points: 3
refs: [test-assets-rigged-mesh-encoder, viewer-r1, viewer-p17-2]
blocked_by: [viewer-audit-render-fixtures-crate]
---

Context: [context/testing.md](../context/testing.md).

[[test-assets-rigged-mesh-encoder]] added `sl-test-assets::rigged`, and
`sl-viewer-world-scene/src/render_scene.rs` already had `rigged_strip`.
Both build a `(Submesh, MeshSkin)` for the same purpose — exercise the
skinning path — and `sl-test-assets`' own crate docs open by arguing that
for a fixture oracle to mean the same thing in two tiers "the generator
has to be one".

They are not gratuitously different, which is why this is a task and not
a deletion:

| | `render_scene::rigged_strip` | `rigged::cylinder` |
| --- | --- | --- |
| shape | flat strip, 18 verts, 16 tris | closed cylinder, 45 verts, 64 tris |
| joints | `mPelvis` / `mTorso` | `mTorso` / `mChest` |
| weights | ramp scaled to sum **0.9** | ramp summing to one |
| output | in-memory only | that, plus asset bytes |

Each difference is load-bearing somewhere:

- the **0.9** is deliberate. `rigged_strip`'s doc argues it at length: a
  rig that arrives tidy would let a regression in `to_bevy_rigged_mesh`'s
  renormalization pass unnoticed, because the check would be looking at
  data that never needed fixing. That is the [[viewer-r1]] case, and the
  render baseline is what catches it.
- `mChest` is the cylinder's upper joint because it is the joint the
  catalogue's chest-twist motion drives, which is what lets one asset
  serve both the rigged and the animesh prim.
- the strip is flat because a render baseline wants a subject whose
  silhouette changes obviously when the deformation is wrong.

So the unification is a **design decision**, not a move. The shapes are
the question: either `sl-test-assets::rigged` grows the strip beside the
cylinder (and an unnormalised variant, since `pathological_rig` makes
only *one* of its four vertices unnormalised — not enough for a whole
subject to read as distorted), or the render scene keeps its own geometry
and imports only the ramp and the skin builders. Pick one and say why.

**Why this waits on [[viewer-audit-render-fixtures-crate]].** Today
`sl-test-assets` is the only crate that turns on `sl-texture/encode`, so
`sl-j2c-encode` is absent from the viewer's production dependency tree.
Making it a non-dev dependency of `sl-viewer-world-scene` — a library
five crates and the viewer binary depend on — would put the JPEG2000
*encoder* in all of them, to serve a fixture. Once the fixtures live in
their own crate, that crate can depend on `sl-test-assets` for free.

Costs to budget: reblessing
`baselines/sl-client-bevy-viewer/render/rigged-mesh.toml`, whose recorded
facts (18 vertices, 16 triangles, the `±0.125` bounds) are the strip's
exact geometry, and re-checking the `SymmetricAbout` claim beside the
scene, which names `±0.125 on X` in its own reason string.

Acceptance: one generator produces every rigged fixture in the workspace;
the malformed-weight subject the render baseline needs still arrives
malformed, and its doc says so where the fixture is built rather than
where it is used.
