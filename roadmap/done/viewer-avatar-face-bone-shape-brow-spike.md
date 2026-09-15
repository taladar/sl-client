---
id: viewer-avatar-face-bone-shape-brow-spike
title: Mesh-head brow spikes forward from face-bone shape deformation
topic: viewer
status: done
origin: split from viewer-avatar-tongue-protrudes during aditi testing (2026-08-05)
refs: [viewer-avatar-tongue-protrudes, viewer-avatar-skeleton-recovery]
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

## Finding (2026-09-15): a third explanation — the reference holds joints

Neither of the two candidates above: the reference viewer **never resets a
joint when the animation driving it ends**. `LLJointStateBlender::
blendJointStates` writes the blended local position / rotation into the
`LLJoint` itself, and for a joint no active motion touches it returns early
("instead of resetting joint state to default, just leave it unchanged from
last frame"). Nothing else puts the joint back — no `onDeactivate`, no
per-frame reset — short of `resetSkeleton` (Reset Skeleton, or a jellydoll /
render-mode change). A keyframe position key replaces the joint's position
outright (`setPosition(blended_pos)`), and the shape's
`LLPolySkeletalDistortion::apply` only telescopes *deltas* onto whatever
position the joint has.

So on a mesh head whose expression / blink animations key every face bone's
position — which is what the "spike vanishes on blink" clue shows this head
doing — the reference's face bones sit at the animation's authored positions
from the first blink on, and stay there between blinks. Ours re-derived every
joint from rest + shape offset each frame, so between blinks the forehead
sprang back to the shape-deformed rest: the spike, returning after every
blink. The shape math itself (driver ramps, sex gate, additive scale/offset,
`scaleChildOffset`, the skin palette) was re-checked against the reference
and matches.

The same hold is visible well beyond this avatar: a Bento hand stays curled
after its hand-pose animation stops, a jaw stays open, and so on — all
reference behaviour we did not reproduce.

## Fix

A joint channel now keeps the last value any animation gave it:

- CPU: `AnimationPose::hold`, folded per avatar in `drive_avatar_skeletons`
  (`AnimationPlayback::held`, forgotten when the avatar stops being rigged).
- GPU: pass B keeps a held row per (slot, joint) behind the local-pose rows in
  the same buffer (the pass is at the 8-storage-buffer floor), reads it for a
  channel no contribution produced and writes it for one that did. The block
  is copied across a buffer growth and zeroed when a slot's occupancy stamp
  changes; the debug readback mirror holds rows for every staged slot.
- Held values are keyframe channels only — idle deltas and CPU corrections
  compose on top and are never held — and the T-pose freeze bypasses them.

Known, documented divergences: a held joint does not follow a *later* shape
edit's delta (the reference telescopes it; the next animation keying the joint
overwrites both), and the CPU mini pose only holds joints in its subset, so a
socket worn on a joint after that joint's animation stopped sits at rest on the
CPU until the joint is animated again.

Verified: unit goldens (`golden_mirror_held_matches_pose_hold` — the CPU
`hold` and the pass-B mirror agree bit-for-bit across frames in which motions
stop) and the headless GPU gate `the_gpu_holds_a_stopped_motions_pose`, which
fails with the shader's hold disabled (worst diff 2.05) and passes with it.
Not verified live on the original avatar, which is gone; Reset Skeleton
([[viewer-avatar-skeleton-recovery]]) is now the only way to clear a held pose,
as in the reference, and is still unimplemented.
