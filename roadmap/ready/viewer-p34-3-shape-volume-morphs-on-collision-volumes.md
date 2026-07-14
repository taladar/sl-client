---
id: viewer-p34-3
title: Shape volume morphs on collision volumes
topic: viewer
status: ready
origin: found while doing viewer-p34-1 (physics-wearable ingest)
refs: [viewer-p34-1, viewer-p17-2]
---

Context: [context/viewer.md](../context/viewer.md).

**P34.3. Apply the *shape* params' volume morphs to the collision volumes.**
[[viewer-p34-1]] added parsing of `<volume_morph>` (`ParamEffect::Morph` now
carries `Vec<VolumeMorph>`), because the body-physics driven params displace the
`LEFT_PEC` / `BELLY` / `BUTT` volumes. But ~30 **ordinary shape** params carry
volume morphs too — `Big_Chest`, `Small_Chest`, `Fat_Torso`, `Breast_Gravity`,
`Muscular_Torso`, `Squash_Stretch_Head`, `Bowed_Legs`, `Foot_Size`, … — and
those are parsed but still **unapplied**: nothing yet accumulates
`effective_weight * (scale, pos)` onto a collision volume's rest transform, the
way `LLPolyMorphTarget::apply`'s volume pass does.

The system body does not care (it is not skinned to the collision volumes),
which is why this went unnoticed — but since [[viewer-p17-2]] the volumes
**are** bindable joints, so a worn **rigged mesh** body/clothing rigged to
`LEFT_PEC` / `BELLY` / … currently ignores the avatar's shape sliders entirely.
In the reference viewer a mesh body does respond to them (that is how a
fitted-mesh body follows the shape's chest / belly / butt sliders).

Shape: a resolver next to `SkeletalDeformations` (same `ResolvedParams` input)
accumulating per-volume scale / position deltas, folded into the
collision-volume joints of the Bevy skeleton instance — noting that
`LLPolyMorphTarget::apply` runs the volume pass
**only when the param's morph data exists on that part**, so a volume morph is
applied once per part carrying the morph, not once per param. Verify on aditi
with a fitted-mesh body and an extreme shape slider.
