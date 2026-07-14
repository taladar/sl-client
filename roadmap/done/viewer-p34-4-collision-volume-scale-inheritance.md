---
id: viewer-p34-4
title: Collision-volume scale inheritance from the skeletal params
topic: viewer
status: done
origin: found while doing viewer-p34-3 (shape volume morphs)
refs: [viewer-p34-3, viewer-p17-2, viewer-p13-4]
---

Context: [context/viewer.md](../context/viewer.md).

**Done.** The **skeletal** params (`param_skeleton`) — `Height`, `Thickness`,
`Torso Length`, `Leg Length`, `Arm Length`, `Head Size`, `Hip Width`,
`Shoulders`, `Neck Thickness`, … — now deform the avatar's **collision volumes**
as well as its bones. That is the second, separate mechanism by which a shape
reaches a worn **fitted mesh** body; [[viewer-p34-3]] did the *morph* half (the
`<volume_morph>` children), this is the other half.

- Firestorm's `LLPolySkeletalDistortion::setInfo` walks each deformed bone's
  children and, for every child whose `inheritScale()` is true — which is *only*
  `LLAvatarJointCollisionVolume` — records a further deformation of
  `cv_rest_scale ⊙ bone_scale_deformation`, applied at the param's weight
  alongside the bone's own. It is therefore **proportional**, not additive: a
  bone scaled `+0.3` grows its volumes by 30%, whatever size they are.
- Resolved in `sl-avatar::volume` rather than `sl-avatar::skeletal`, folded into
  the existing `VolumeDeformations` as a second source
  (`from_resolved_with_skeleton`, which additionally takes the `Skeleton` — it
  needs each bone's volumes and their rest scales). One type then carries
  *everything* a shape does to a volume, and the Bevy layer keeps folding one
  accumulation into the volume joints. `SkeletalDeformations` stays
  skeleton-free; its module docs, which claimed the inheritance was deliberately
  omitted because "collision volumes are not part of the rendered / skinned
  skeleton" — true when [[viewer-p13-4]] wrote it, false since
  [[viewer-p17-2]] — are corrected in place.
- The viewer's `AvatarAssetLibrary` now keeps the parsed `Skeleton` alongside
  the flattened `BevySkeleton` (whose joint list no longer distinguishes a bone
  from a volume) and hands it to the resolver. The `V` key and the
  `SL_VIEWER_VOLUME_MORPH_GAIN` env A/B cover the new pass for free, since it
  lands in the same accumulation.
- Unit-tested in `sl-avatar` (weighting, the proportional-to-rest-scale rule,
  the bone *offset* not being inherited, a param naming a bone the skeleton
  lacks, and the two passes accumulating onto one volume) and in
  `sl-client-bevy` (a `param_skeleton` scaling `mTorso` grows the `BELLY` volume
  joint's world scale by 10% of its own — and does not, without this pass).

**Verified live on aditi** on the agent's own fitted BoM mesh body (26 volumes,
86% of its skin weight), A/B'd with the `V` key within one session.

Worth recording: this pass also dissolves the *subject-finding* problem P34.3
fought. That phase needed `SL_VIEWER_VOLUME_FOCUS` to hunt down the most extreme
shape in the region, because the morph pass barely engages a slim near-default
shape — the agent's own displaced `BELLY` by zero position and a tiny scale
delta. With the skeletal pass that same agent scores **0.55**: every avatar has
a height and a thickness, and each inherited delta is a fraction of the volume's
own size, so an ordinary shape now displaces its volumes plainly.
