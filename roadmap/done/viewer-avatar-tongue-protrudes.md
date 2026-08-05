---
id: viewer-avatar-tongue-protrudes
title: Avatar tongue protrudes (rigged mesh + base-system head)
topic: viewer
status: done
origin: user report during the viewer-sun-disc-grey aditi verification (2026-08-04)
refs: [viewer-render-cpu-skinning-crosscheck, viewer-avatar-skeleton-recovery,
  viewer-avatar-face-bone-shape-brow-spike]
---

Context: [context/viewer.md](../context/viewer.md).

On aditi other avatars' **tongues stuck straight out of the mouth**, uniformly,
just above the upper lip (self was fine — self's mesh head plays a different
AO).

**Root cause (fixed):** SL animation *position* tracks are not all offsets. A
neutral-face Bento animation (`agent_bento_...`, id
`201d84aa-…`, played as the resting-face state) stores each face bone's
**absolute rest local position** as its position track — e.g. `mFaceTongueBase`
= `(0.039, 0, 0.005)`, exactly its `avatar_skeleton.xml` rest. Our skeletal
recurrence (`BevySkeleton::deformed_world_matrices`) treated **every** position
track as an offset and **added** it to the joint's rest, doubling it
(`0.039 + 0.039`) and sliding the whole face subtree forward — the tongue out of
the mouth (and the brow spike on heads with brow geometry). The reference
(`LLJointState::setPosition` via the pose blender) *replaces* the rest with a
non-pelvis position key; only `mPelvis` is the historical offset (its rest local
is the 1.067 m pelvis-above-root — replacing would collapse it).

Fix: a position track is an **absolute local position that replaces the rest**
for a normal bone; it stays an **offset (added)** for `mPelvis`
**and for collision volumes** (the body-physics breast/belly/butt displacements
this pipeline writes into the pose as deltas — replacing those caved the chest
in, a regression caught and fixed in the same pass). Verified live on aditi:
three tongue-out avatars' tongues went from ~0.14 m to ~0.10 m from `mHead`
(rest) and stayed there through the full pose, chests normal. Unit test:
`animation_position_track_is_absolute_for_bones_and_offset_for_pelvis_and_volumes`.

**Also fixed along the way:** attachment points (`Mouth`, `Chin`, `Nose`,
`Tongue`, …) were not bindable skeleton joints, so a mesh rigging verts to them
fell them back to `mPelvis` (down-spikes). Added them as joints
(`BevySkeleton::insert_attachment_points`, mirroring the reference's
`LLViewerJointAttachment`), test
`attachment_points_become_bindable_joints`.

**Not this bug (separate, still open):** the base-system `avatar_head.llm`
tongue was verified tucked all along (its geometry is weighted to
`mHead`/`mNeck`, no Bento tongue bones). And one avatar's **brow** spikes from
*shape* deformation of the face bones (stable across pose stages, briefly
corrected when a face animation blinks) — split out as
[[viewer-avatar-face-bone-shape-brow-spike]].
