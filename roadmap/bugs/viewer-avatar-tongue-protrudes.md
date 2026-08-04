---
id: viewer-avatar-tongue-protrudes
title: Avatar tongue protrudes (rigged mesh + base-system head)
topic: viewer
status: bugs
origin: user report during the viewer-sun-disc-grey aditi verification (2026-08-04)
refs: [viewer-render-cpu-skinning-crosscheck, viewer-avatar-skeleton-recovery]
---

Context: [context/viewer.md](../context/viewer.md).

On aditi the avatar's **tongue sticks straight out of the mouth**, well past
the lips. Seen on both a full mesh-body avatar (a mesh head) and on a
base-system avatar (the default `avatar_lad` head), so it is not specific to a
particular worn mesh — the base head geometry itself renders with the tongue
protruding.

The SL rest pose keeps the tongue tucked inside the mouth. A tongue poked out
in the neutral pose points at either:

- **A joint/rest-pose problem on the tongue bone.** The base head has a tongue
  driven by the face bones (`mFaceTongueBase` / `mFaceTongueTip` in the Bento
  skeleton). If we never apply the correct rest transform for those joints (or
  apply a joint override / a wrong bind pose), the tongue mesh floats to a
  default that reads as "out". Cross-check against
  [[viewer-render-cpu-skinning-crosscheck]] — dump the tongue-bone world
  transforms vs the reference bind pose.
- **A visual-param / morph not applied.** `avatar_lad.xml` has mouth / tongue
  driven params; if a default param that tucks the tongue is not evaluated on
  the base head, the tongue sits in its un-morphed extreme. Check whether the
  base-mesh morph table includes a tongue param we skip.
- **The mesh head case** may be a *separate* symptom (its own tongue mesh
  rigged to `mFaceTongue*`) that happens to share the same wrong joint pose —
  confirm the base-head case first, since it is fully client-side reproducible
  (no worn mesh needed) and likely the same root cause.

Reproducible on the base-system avatar, so a headless skinning/pose test that
reads the tongue-bone transform (or the tongue vertices' posed position) should
be able to pin it without a live login. Needs the Firestorm rest pose as the
reference target.
