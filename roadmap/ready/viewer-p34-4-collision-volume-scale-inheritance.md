---
id: viewer-p34-4
title: Collision-volume scale inheritance from the skeletal params
topic: viewer
status: ready
origin: found while doing viewer-p34-3 (shape volume morphs)
refs: [viewer-p34-3, viewer-p17-2, viewer-p13-4]
---

Context: [context/viewer.md](../context/viewer.md).

**P34.4. Inherit a deformed bone's scale onto its collision volumes.**
[[viewer-p34-3]] applies the *morph* params' `<volume_morph>` children to the
collision volumes. The **skeletal** params (`param_skeleton`) reach them by a
second, separate mechanism that is still missing:
`LLPolySkeletalDistortion::setInfo` walks each deformed bone's children and, for
every child whose `inheritScale()` is true — which is *only*
`LLAvatarJointCollisionVolume` — records an extra deformation of
`cv_rest_scale ⊙ bone_scale_deformation`, applied at the param's weight
alongside the bone's own. So a body-thickness / torso / leg-length slider
fattens or stretches the volumes as well as the bones.

`SkeletalDeformations` (`sl-avatar::skeletal`) deliberately omits this, and says
so in its module docs: "collision volumes are not part of the rendered / skinned
skeleton". That was true when P13.4 wrote it and [[viewer-p17-2]] made it false
— a rigged mesh body rigs to the volumes, so it currently misses the scale half
of the shape while (since [[viewer-p34-3]]) tracking the morph half.

Shape: the resolver needs the `Skeleton` (for each bone's collision-volume
children and their rest scales), which it does not take today — so either a new
constructor (`from_resolved_with_skeleton`) or fold it into
`VolumeDeformations`, which already accumulates per-volume scale deltas and
would just gain a second source. Verify on aditi with a mesh body and an extreme
body-thickness slider, A/B with the `V` key (or
`SL_VIEWER_VOLUME_MORPH_GAIN=0`).
