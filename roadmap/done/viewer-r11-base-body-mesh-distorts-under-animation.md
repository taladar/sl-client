---
id: viewer-r11
title: Base-body mesh distorts under animation
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Known rendering issues (to fix)
---

Context: [context/viewer.md](../context/viewer.md).

**R11. Base-body mesh distorts under animation** — fixed by R13
(`sl-avatar` / `sl-client-bevy`). Surfaced by P18.3: a *shaped* avatar's limbs
(arms most visibly) stretch / distort while an animation plays, but look
correct at rest and return to correct on stop. The **skeleton was posed
correctly** all along — the joint world matrices are right and the bone
lengths stay constant under animation (verified live from a per-frame
`mShoulderLeft`→`mElbowLeft`→`mWristLeft` length dump: a steady `0.289` /
`0.214` throughout dance1), so the distortion was in the **skin**, not the
pose. The original premise here (that the base body needed the reference
viewer's `LLSkinJoint` **pivot** scheme —
`LLViewerJointMesh::uploadJointMatrices` baking `mRootToJointSkinOffset` /
`mRootToParentJointSkinOffset` into the skinning matrix) was **disproven**:
R12 measured the skin pivots as a sub-millimetre no-op, and R13 found the real
cause — the base-mesh joint-render-data list was **including the extended
(Bento) ancestors** (`mSpine*`) the reference viewer skips, shifting every
weight index past them so whole arm chains bound to the wrong joint (invisible
at bind pose, but a rest-pose armpit spike and gross arm distortion the moment
a joint rotated). The R13 `base_ancestor` fix (skip non-base ancestors,
`getBaseSkeletonAncestor` / SL-287) corrected the binding, and it was
*expected* to also fix this animation-time distortion. **Re-checked and
confirmed:** no new code was needed here. Verified live on the local OpenSim
(own shaped avatar playing dance1 via `--play-animation`/`--repeat-animation`,
offline screenshot harness, both head-on and a 50° orbit): across the full
range of poses — elbows bent, arms spread wide sideways, arms raised — the
limbs skin cleanly with no stretch, ballooning, or spikes. The arm distortion
R11 describes is gone.
Rigged-mesh bodies (Phase 17, ordinary skin weights) were never affected.
