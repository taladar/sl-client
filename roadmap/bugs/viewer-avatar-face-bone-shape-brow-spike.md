---
id: viewer-avatar-face-bone-shape-brow-spike
title: Mesh-head brow spikes forward from face-bone shape deformation
topic: viewer
status: bugs
origin: split from viewer-avatar-tongue-protrudes during aditi testing (2026-08-05)
refs: [viewer-avatar-tongue-protrudes]
---

Context: [context/viewer.md](../context/viewer.md).

Distinct from the (fixed) protruding-tongue animation bug: on at least one
avatar the **brow/forehead geometry spikes forward off the face**. Unlike the
tongue, this is **not** animation-driven — the forehead bone's distance from
`mHead` is elevated already at the **deformed rest** (measured ~0.164 m vs
~0.134 on our own avatar) and is **stable across every pose stage** (rest =
world0 = final). So it comes from the avatar's **shape/skeletal deformation** of
the face bones, which the worn mesh head (no joint positions in its skin) does
not compensate for — the mesh's inverse-bind was baked against the standard
face-bone positions, so a shape-deformed forehead drags the brow geometry
forward.

Diagnostic clue: the spike **briefly disappears when the avatar blinks** — a
facial (blink/expression) animation momentarily poses the forehead bones to
their authored rest position (now applied correctly as an absolute after the
[[viewer-avatar-tongue-protrudes]] fix), snapping the brow back; between blinks
it returns to the shape-deformed rest and spikes again.

To investigate: which **transmitted** shape params deform the face-bone chain
(head size / face shear vs the group-1 face params that are *not* transmitted
for other avatars), whether our `SkeletalDeformations` over-applies that
deformation to the deep face bones vs the reference, and whether the reference
simply does not propagate that deformation to a BoM mesh head the way we do.
Needs the specific avatar live (it left before this could be pinned) or the
[[viewer-avatar-state-dump-replay]] capture to reproduce its shape offline.
