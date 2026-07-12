---
id: viewer-p31-14
title: Locomotion IK adjustments
topic: viewer
status: ready
origin: VIEWER_ROADMAP.md — Simulator authority & the Firestorm motion model (read before P31.2)
---

Context: [context/viewer.md](../context/viewer.md).

**P31.14. Locomotion IK adjustments.** The locomotion group split out of
P31.8 — every piece needs an **inverse-kinematics solver** the project does
not have. `LLKeyframeWalkMotion` matches the walk/run playback speed to the
ground velocity, and the always-on `LLWalkAdjustMotion` foot-plants with
pelvis lag to kill foot-skate; `LLKeyframeStandMotion` twists the lower body /
feet to face the look direction with foot IK (a standing avatar's legs follow
the camera); `LLKeyframeFallMotion` blends the landing recovery;
`LLFlyAdjustMotion` banks the fly. Build a small foot / limb IK pass first,
then layer these over the P18 blend. Reference: `llkeyframewalkmotion.cpp` /
`llkeyframestandmotion.cpp` / `llkeyframefallmotion.cpp`.
