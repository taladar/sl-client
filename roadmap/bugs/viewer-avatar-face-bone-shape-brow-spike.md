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

Mechanism (traced through `avatar_lad.xml`): every face-chain `param_skeleton`
is a **group-1** (non-transmitted) param — `Head Size` (655), `Forehead Angle`
(31629), `Egg_Head` (30646), `Big_Brow` (30001), `Square_Head`, `Head Length`
(30772)… — but each is **driven by a transmitted group-0 slider** (`Head Size`
id 682 → 655, `Big_Brow` id 1 → 30001, `Forehead Angle` id 629 → 31629,
`Egg_Head` id 646, `Head Shape` id 193, `Head Length` id 773). So a wearer's
shape **does** reach these skeletal deforms via the driver, for any avatar.
`Head Size` scales `mSkull`/`mHead`/`mFaceRoot`/forehead **uniformly** (the
whole head scales coherently — no spike on its own); the spike comes from the
**differential** forehead deforms — chiefly **`Big_Brow`** (offsets
`mFaceForeheadCenter` +0.007 m **forward**) and `Forehead Angle` / `Egg_Head` —
which push the forehead bone forward relative to the rest of the head. A
large-man shape (big head + prominent brow) sets these; medium-woman shapes
leave them ~neutral, which is why only one avatar showed it. Standard
forehead-from-`mHead` ≈ 0.145 m; his ≈ 0.164.

Why it may be viewer-specific — the open question: an offline scan of the mesh
cache (classifying a head by **geometry actually weighted to face bones**, not
merely listing them — bodies/clothing list the whole skeleton) found
**288 of 344 real head parts ship no joint positions** (no
`alt_inverse_bind_matrix`), so most mesh heads cannot compensate for a
shape-deformed skeleton at all. If the reference genuinely renders his brow
fine, either his head is one of the 56 that **do** ship joint positions (and we
fail to apply the forehead override, or double-count it against our scale
inheritance — check `joint_position_overrides` on the `mFace*` bones and the
`deformed_world_matrices` scale path), or the reference does **not** propagate
the shape's face-bone `param_skeleton` to a BoM mesh-head avatar the way we do.
Decide between these by capturing his shape param values (esp.
`Head Size`/`Big_Brow`/ `Forehead Angle`) and his head-mesh's
`alt_inverse_bind_matrix` — needs the avatar live (he left) or the
[[viewer-avatar-state-dump-replay]] capture, then compare our forehead deform to
the reference's `LLPolySkeletalDistortion` for those exact values.
