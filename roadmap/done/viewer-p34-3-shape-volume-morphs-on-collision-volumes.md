---
id: viewer-p34-3
title: Shape volume morphs on collision volumes
topic: viewer
status: done
origin: found while doing viewer-p34-1 (physics-wearable ingest)
refs: [viewer-p34-1, viewer-p34-2, viewer-p17-2, viewer-p34-4]
---

Context: [context/viewer.md](../context/viewer.md).

**Done.** The ~30 shape params carrying `<volume_morph>` children (`Big_Chest`,
`Small_Chest`, `Fat_Torso`, `Breast_Gravity`, `Muscular_Torso`,
`Squash_Stretch_Head`, `Bowed_Legs`, `Foot_Size`, …) now displace the avatar's
**collision volumes**, which is how a worn **fitted mesh** body follows the
shape sliders — a system-body morph target cannot reach one, and since
[[viewer-p17-2]] the volumes are bindable joints.

- `sl-avatar::volume` resolves an appearance into per-volume scale / position
  deltas (`VolumeDeformations`, the collision-volume counterpart of
  `SkeletalDeformations`: same `ResolvedParams` input, same pure Z-up maths,
  `effective_weight * (scale, pos)` accumulated across params — the net effect
  of the reference's telescoping volume pass in `LLPolyMorphTarget::apply`).
- The Bevy skeletal recurrence (`BevySkeleton::deformed_world_matrices`) adds
  them to each volume joint's rest scale / position. Bone names (`mChest`) and
  volume names (`LEFT_PEC`) are disjoint, so one pass covers both.
- The body-physics bounce ([[viewer-p34-2]]) reaches the same joints through the
  animation pose, so the two compose: a volume **rests** where the shape puts it
  and **bounces** around that. The physics `*_Driven` params are excluded from
  the resolver (their rest weight is zero and P34.2 applies their volume morphs
  per frame — counting them twice would double the bounce).

**A latent parsing bug fell out of this.** A morph param may be declared under
several `<mesh>` parts, and the reference builds one `LLPolyMorphTarget` per
declaration, each carrying that declaration's own volume morphs. Our param table
is keyed by id with **last-wins**, and the head params (`Squash_Stretch_Head`,
`Elongate_Head`) are re-declared `shared="1"` under the *eyelash* mesh
**without** volume morphs — so the winning declaration silently dropped their
`HEAD` displacement. `VisualParams::from_xml` now concatenates the volume-morph
lists across declarations of one id, which both fixes that and reproduces the
reference's multiplicity (a volume morph declared on two parts applies twice).

Unit-tested in `sl-avatar` (weighting, accumulation across params, sex gating,
the physics params staying out, the two-part multiplicity + the shared-param
merge) and in `sl-client-bevy` (a volume morph moves and scales the volume joint
and leaves its parent bone alone).

**Verified live on aditi** by toggling the effect on and off on one avatar in
one session (the `V` key): a fitted mesh body's chest, waist and hips visibly
change. Getting there needed three debug affordances, all kept:

- **`V`** toggles the displacement live (`VolumeMorphGain`, seeded from
  `SL_VIEWER_VOLUME_MORPH_GAIN`; `0` reproduces the pre-P34.3 rest volumes, a
  large value exaggerates a shape whose real displacements are centimetres).
- **`SL_VIEWER_VOLUME_FOCUS`** frames the avatar whose shape displaces its
  volumes the most (`=1`), or a pinned agent id — because the *agent's own*
  shape may barely displace anything, and then there is nothing to see however
  hard the effect is amplified. This cost most of the debugging: the agent's
  slim aditi shape moves `BELLY` by **zero** position and a small scale delta,
  and per-volume numbers read off the log without filtering by agent were
  mistaken for its own.
- **`SL_VIEWER_TPOSE=1`** freezes every avatar at its shaped rest pose (the AO
  otherwise walks and turns it, so no two frames are comparable). Note it is a
  poor tool for *this* kind of check on its own: bind pose **is** the T-pose, so
  a mesh that stopped following the skeleton looks identical to a correct one.

Also added: `BevySkeleton::is_collision_volume` (a Bento bone is `Extended` too,
so `support` alone cannot tell them apart) and a bind-time diagnostic reporting
whether a worn rig is *fitted* — how many volumes it binds, how many its own
joint overrides pin, and what share of its skin weight rides them (the agent's
mesh body: 26 volumes, 86%).

Still missing, filed as [[viewer-p34-4]]: the skeletal params' **scale
inheritance** onto the volumes (`LLPolySkeletalDistortion::setInfo`'s
`inheritScale()` pass), the *other* way a shape reaches a collision volume.
