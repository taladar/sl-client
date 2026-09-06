---
id: viewer-skinned-bind-group-quits-on-rez
title: A skinned draw takes the non-skinned bind group and the render handler quits the viewer
topic: viewer
status: bugs
origin: hit twice on aditi while live-checking the keyed group profile
  (2026-09-06)
refs: [viewer-edit-outline-skinned-mesh, viewer-animesh-transparent-box-shell,
  viewer-rigged-attachments-wearer-not-resolved]
---

Context: [context/viewer.md](../context/viewer.md).

On aditi, a few seconds after the scene starts rezzing — with **nothing
selected and no floater open** — the renderer hits a wgpu validation error and
Bevy's error handler **quits the whole viewer**. The session is otherwise
healthy (login, region handshake, EEP, appearance bake all complete first), so
the window simply vanishes on the user a second or two after content appears.

Exact messages, in order:

```text
ERROR bevy_render::error_handler: Caught rendering error: Validation Error
  In a CommandEncoder
    In a draw command, kind: MultiDrawIndirect
      The BindGroupLayout with 'mesh_layout' label of current set BindGroup
      with 'model_only_mesh_bind_group' label at index 2 is not compatible with
      the corresponding BindGroupLayout with 'skinned_me…' label
        Expected entry with binding 1 not found in assigned bind group layout
ERROR bevy_render::error_handler: Caught rendering error: Validation Error
  In Queue::submit … (the same mismatch)
ERROR bevy_render::error_handler: Quitting the application due to
  Validation RenderError
```

## Why this is not the outline bug

[[viewer-edit-outline-skinned-mesh]] is the same *mismatch* — a mesh
specialised into the skinned pipeline handed the `model_only` bind group — but
its source was the edit-selection shell, and that path was mitigated (the shell
is skipped for a face with a `SkinnedMesh`). Here no selection exists: the draw
is a **`MultiDrawIndirect` batch**, so whatever carries the skinned vertex
attributes is being batched with (or as) a non-skinned mesh in the ordinary
scene pass.

Candidates to check first, in order:

1. A **rigged in-world mesh or attachment** spawned with `JOINT_INDEX` /
   `JOINT_WEIGHT` but no `SkinnedMesh` component — the same shape as the
   outline bug, from the object/attachment path rather than the selection one
   (see [[viewer-rigged-attachments-wearer-not-resolved]] for a place where a
   rigged attachment is spawned before its wearer is known).
2. The **GPU in-place pose path** (`gpu_avatars`, on by default on a capable
   device — the log says "GPU in-place pose path ACTIVE"): it writes skin
   palettes into `SkinUniforms` for avatars that have no skinning joint
   entities, so a batch that specialised as skinned may be handed the
   model-only group. `SL_VIEWER_GPU_AVATARS=cpu` forces the legacy CPU path and
   is the cheapest A/B for this.
3. `bevy_pbr`'s **batching / `MultiDrawIndirect`** grouping in the fork
   ([[sl-client-bevy-pbr-fork]]) putting a skinned and a non-skinned mesh in one
   indirect batch.

## Not always — and not a code change

Two aditi sessions at 17:23 and 17:26 ran for minutes without it. The 18:46 and
18:48 sessions died within ~10 s of content appearing. The natural suspicion
(the uncommitted keyed group-profile conversion in the tree at 18:46) was
**tested and cleared**: a release build of plain `4f4651f7`, with that work
stashed, reproduced the identical error and quit at 19:01 — the same code that
had run happily at 17:26.

So the trigger is **what is in the scene** (whose rigged attachments are in
view), not a change in this repository, which is also why the per-commit
content tests do not catch it: they run against fixtures and the local grid,
never against whatever avatars are standing in an aditi region at the time.
The A/B below should therefore be run on a scene that reproduces it rather than
on an empty region.

## Why it matters beyond the artifact

A validation error quits the application. Whatever the ultimate fix, the
handler's behaviour makes any rigged-content bug a **crash** for the user, and
it makes live verification of unrelated work impossible on a region where it
fires. Worth deciding separately whether a validation error should be fatal in
a release build (the reference viewer logs and drops the draw).

## How to verify

Log into the aditi region above with the default (GPU) pose path and stand
where other avatars' attachments rez; the viewer must stay up. Then re-check
with `SL_VIEWER_GPU_AVATARS=cpu` to tell candidate 2 apart from 1 and 3.
