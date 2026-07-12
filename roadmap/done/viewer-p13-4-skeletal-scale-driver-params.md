---
id: viewer-p13-4
title: Skeletal-scale & driver params
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 13 — Base avatar in the viewer (replace spheres)
---

Context: [context/viewer.md](../context/viewer.md).

**P13.4. Skeletal-scale & driver params.** Apply `param_skeleton`
bone scale/position params and driver→driven propagation so proportions
(height, limb/head scale, pelvis) match; rebuild the skeleton instance's
rest transforms accordingly. Verify a shaped avatar (2nd login) looks correct.
**Done:** two new pure `sl-avatar` modules. `resolve` — `ResolvedParams` turns
a partial appearance vector into every param's effective weight: it fills in
the *non-transmitted* driven params from their (transmitted) drivers via the
Firestorm `LLDriverParam::getDrivenWeight` trapezoid ramp (the classic
transmitted `male` driver → the non-transmitted `Male_Skeleton` / `Male_Head`
… params), leaves a transmitted driven param at its wire value (the sender
already resolved it), decides avatar sex from the `male` param (`> 0.5`), and
sex-gates each param's `effective_weight` (`getSex() & avatar_sex ? weight :
default`, mirroring the gate the reference viewer applies before every
distortion). `skeletal` — `SkeletalDeformations` sums `effective_weight *
deformation` per bone into a scale + offset delta (the net of Firestorm
`LLPolySkeletalDistortion::apply`, which telescopes from a zero baseline, so a
param at any weight contributes `weight * deformation`; collision-volume
`inheritScale` is skipped as it never touches the skinned skeleton). `morph`'s
`MorphWeights` now routes through `ResolvedParams` too (new `from_resolved`),
so driven morphs and sex gating apply to P13.3 shapes as well. In
`sl-client-bevy`, `BevySkeleton` gains `deformed_local_transforms(&deform)`:
because the Second Life skeleton has semantics a plain nested transform
hierarchy cannot express — a bone's own scale stretches only its bound
geometry (never inherited into a child's world scale) while a parent's *local*
scale stretches its immediate child's position offset (the `scaleChildOffset`
mechanism that drives height / limb length) — it runs that exact world-matrix
recurrence and returns each joint's `parent_world⁻¹ · own_world` relative
transform, which Bevy's ordinary propagation re-composes back into the correct
world matrix regardless of how Bevy accumulates scale (the transmitted
skeletal bones are axis-aligned, so the relatives carry no shear and decompose
losslessly into a `Transform`); the rest bind poses / inverse bindposes are
left untouched, so the deformation reads as the skin's deviation from bind
pose. In the viewer, each skeleton-instance joint now carries an
`AvatarJoint { agent, index }` marker, `apply_avatar_morphs` became
`apply_avatar_appearance`
(one `ResolvedParams` per dirty avatar feeds both the morph mesh rebuild and
the joint re-deform), and a body's joints are re-set from
`deformed_local_transforms` on the same fresh-appearance / just-spawned dirty
signal the morphs use. Net-new library surface was three re-exports
(`ResolvedParams`, `SkeletalDeformations`, `BoneDeform`) plus the two
`sl-avatar` modules and the `BevySkeleton` method. Verified live on **both**
grids: OpenSim (`shaped 8 body part(s) + 133 joint(s) across 1 avatar(s)`) and
aditi with a genuinely shaped avatar (avatar1), each applying its morphs
*and* its full 133-joint skeletal deformation with no skinning / wgpu errors.
Driver→driven propagation of skeletal / morph params to *other* (non-agent)
avatars still waits on their appearance arriving (P14 baked slots carry it),
and a fully general SL skeleton under animation will need CPU world-matrix
posing (the nested-relative shortcut holds only while the pose is static +
shear-free), which the animation phase will revisit.
