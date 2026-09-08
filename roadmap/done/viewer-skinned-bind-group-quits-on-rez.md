---
id: viewer-skinned-bind-group-quits-on-rez
title: A skinned draw takes the non-skinned bind group and the render handler quits the viewer
topic: viewer
status: done
origin: hit twice on aditi while live-checking the keyed group profile
  (2026-09-06)
refs: [viewer-edit-outline-skinned-mesh, viewer-animesh-transparent-box-shell,
  viewer-rigged-attachments-wearer-not-resolved]
---

Context: [context/viewer.md](../context/viewer.md).

On aditi, seconds after content rezzed — nothing selected, no floater open —
the renderer hit a wgpu validation error and Bevy's error handler **quit the
whole viewer**.

## The cause

`crate::water_clip` splits a translucent face that straddles the waterline into
two draws, spawning a *twin* that shares the face's **mesh asset**:

```text
commands.spawn((
    Mesh3d(mesh.0.clone()),   // the face's own, possibly rigged, mesh
    MeshMaterial3d(twin),
    Transform::IDENTITY,
    WaterClipTwin, WaterClipSide::Below, ChildOf(face),
));
```

It did not clone the face's `SkinnedMesh`. Bevy specializes the render pipeline
from the mesh asset's **vertex layout** (`bevy_pbr`'s `is_skinned(layout)`, true
when the mesh carries `JOINT_INDEX` + `JOINT_WEIGHT`) but chooses the bind group
from the **entity** (`skin_byte_offset(entity).is_some()`). So the twin of a
*rigged* face got the skinned pipeline and the model-only bind group — the
validation error, exactly.

This is the same defect as [[viewer-edit-outline-skinned-mesh]] — a mesh handle
cloned onto a new entity without its skin — in a second path that never got that
one's mitigation.

## Why it read as random, and near other people

Worn rigged submeshes deliberately **share** one converted mesh asset across
wearers (`GeometryCache`'s rigged slots) so Bevy can batch them. On a
storage-buffer device — every desktop GPU — `no_automatic_skin_batching` returns
early, so skinned meshes really are batched, and the batch takes its bind group
from **one representative entity**. A single malformed twin therefore poisons
the whole `MultiDrawIndirect` batch and takes down every other wearer drawn in
it.

It fires only when a *rigged* face straddles the water plane, which depends
entirely on who is standing where — hence "not always", and hence the suspicion
that it was about whatever was in the scene rather than about this repository.

## What the first write-up got wrong

All three of its candidates were wrong, and the crash text it quoted was
truncated in the one place that mattered. The full log names the pipeline:

```text
... is not compatible with the corresponding BindGroupLayout with
'skinned_mesh_layout' label of RenderPipeline with
'pbr_alpha_blend_mesh_pipeline' label
```

`pbr_alpha_blend_mesh_pipeline` — the **transparent** phase, not the opaque one.
That is the water-clip path's own phase, and rigged faces are forced to
`TextureAlpha::Blend` (a rigged face cannot alpha-mask), which is why the
failing draw was always a rigged *transparent* one.

- Candidate 1 (a rigged mesh spawned with no `SkinnedMesh`) was the right
  *shape* but the wrong site: `build_rigged_submeshes` inserts the skin
  atomically. The malformed entity was its twin, not it.
- Candidate 2 (the GPU in-place pose path) is not involved.
- Candidate 3 (`bevy_pbr` batching putting a skinned and a non-skinned mesh in
  one batch) is not it either — but batching *is* why one bad entity is not a
  local artifact. The batch is well-formed; its representative was not.

## The fix

The twin clones the face's `SkinnedMesh`. That is also the correct rendering:
the twin draws the *same posed geometry*, clipped to the other side, so it must
skin identically. Skipping rigged faces instead (the outline bug's mitigation)
would have left rigged content unclipped at the waterline.

## How it was verified

On aditi, with an avatar standing in the shallows so its rigged faces straddle,
the two builds differ only in that clone:

| build | skinned splits | malformed twins | exit |
| --- | --- | --- | --- |
| without the clone | 7 | **14** | 1 |
| with the clone | 40 | **0** | 0 |

Before the fix the malformed entities were caught in four separate aditi runs
(4–9 each) and identified by their component fingerprint — `Mesh3d` +
`MeshMaterial3d<FaceMaterial>` + `Transform` + `Visibility` + `ChildOf` +
`Aabb`, and crucially **no** `PrimFaceEntity`, `AvatarBodyPart`,
`AvatarPickTarget` or `Name` — with a parent chain of face < body root. That
fingerprint matches this spawn site and no other.

Pinned by `a_skinned_straddling_face_gives_its_twin_the_skin` and
`an_unskinned_straddling_face_gives_its_twin_no_skin` in `water_clip`.

## Generalised, so the next one is not found the same way

Finding this needed a rigged face to cross a water plane while somebody was
looking, which is why it survived as long as it did. The same split exists for
**morph targets** — the pipeline key comes from the mesh asset
(`mesh.morph_targets()`), the bind group from the entity's morph index — so a
twin of a morphed face would fail identically. It is not reachable today
(runtime morphs are attached to avatar base parts, which carry no
`PrimFaceEntity` and so never reach the split), and that is exactly why it is
worth pinning rather than waiting for a mesh head to bring facial morphs to a
worn submesh.

The twin now copies **every component the mesh bind group is built from**, and
`a_twins_bind_group_inputs_match_its_faces` enumerates the four combinations of
skinned × morphed, asserting the twin matches its face in each. The test has
teeth: dropping the morph copy fails it on the `morphed: true` rows.

The SL asset categories deliberately do **not** form an axis of that matrix.
Every face reaching the split is a `PrimFaceEntity` with a `FaceMaterial`; a
prim, mesh and sculpt face are all spawned by one `spawn_face_entity` and differ
only in geometry the twin never inspects, while a worn rigged submesh and an
animesh submesh differ precisely by carrying a `SkinnedMesh`. Enumerating the
categories would re-test one path four times and still miss the combinations
that break; enumerating the bind-group inputs covers every category by
construction.

## What else came out of it

- **A runtime guard**, `crate::skin_agreement`: the attribute/`SkinnedMesh`
  agreement is checked in the main world on whatever changed, *before* the
  extract that would hand a mismatch to wgpu, and the run ends naming the
  entity, its ancestry and its mesh. It is deliberately still fatal — making the
  validation error non-fatal would hide a real bug in exactly the release builds
  this project tests with. It is the runtime twin of `render_test`'s
  `unskinned_violations`, which decides the same property for scenes a test can
  build; this one covers content only a grid can produce. It is what turned this
  from an unattributable crash into a named entity in one run.
- **A reconciliation in `avatar_assets`**: `BASE_PARTS` *declares* each part
  skinned or rigid while `build_base_mesh` *derives* the attributes from the
  file's own `has_weights`. Nothing kept the two in step, which is how the
  earlier real-Linden-eye-parts crash happened. They are now reconciled at load,
  and a part whose declaration disagrees with its mesh is skipped with a warning
  rather than rendered into a validation error. (The vendored character
  directory agrees for all eight parts; an `SL_VIEWER_ASSETS` override need
  not.)

## A second bug the same module was hiding

The reconciler re-tested a face only when its transform moved, its `Aabb` was
**added**, or the water level changed. Nothing there notices *geometry* moving
while the entity stands still — and three kinds of content do exactly that, all
of them carrying `PrimFaceEntity` and so reaching this split:

| producer | mechanism | amplitude |
| --- | --- | --- |
| flexi prims | client-side sim mutates the mesh asset | metres |
| animation | GPU skin palette | metres |
| body physics on a mesh body | volume-joint deltas through that palette | centimetres |

Bevy's `calculate_bounds` rewrites the `Aabb` on `AssetChanged<Mesh3d>` and the
posed avatar bound is written back every frame, so all three mark the bounds
**changed, never added**. A flexi drooping into the sea was therefore never
split, and its submerged half went on being painted over by the depth-writing
sea — the very defect the split exists to fix. The test is now
`aabb.is_changed()`, pinned in both directions by
`a_face_that_deforms_into_the_water_is_split` and
`a_face_that_deforms_clear_of_the_water_is_made_whole`, which both fail against
`is_added()`.

The cost that buys is real and is written down rather than papered over: avatar
faces now pay the straddle test every frame, because the posed bound is
rewritten every frame ([[viewer-posed-avatar-bounds-rewritten-every-frame]]).

## Decided: a validation error stays fatal

Whether a wgpu validation error should be fatal in a release build was left open
while the cause was being found, since the reference viewer logs and drops the
draw. **Decided (2026-09-08): it stays fatal.** A crash is preferable to
corruption — crashes get fixed, silent corruption does not — and at this stage
nothing is released, so there is no user to spare the hard failure. That the
reference viewer chooses otherwise is not a reason to copy it; it ships to
people who cannot fix it.

The effort belongs in making a fatal error *say what broke*, not in making it
quieter. `crate::skin_agreement` is that: it fails **earlier** than the wgpu
error it pre-empts, in the main world, naming the entity, its ancestry and its
mesh — which is what turned this bug from an unattributable crash into a
diagnosis in one run.
