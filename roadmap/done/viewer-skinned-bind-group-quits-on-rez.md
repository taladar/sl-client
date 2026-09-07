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

## Still worth deciding separately

Whether a wgpu validation error should be fatal in a release build at all. The
reference viewer logs and drops the draw. Nothing here depends on that answer
any more, but it is the difference between one bad content path being a crash
and being an artifact.
