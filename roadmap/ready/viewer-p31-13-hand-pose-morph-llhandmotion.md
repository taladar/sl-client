---
id: viewer-p31-13
title: Hand-pose morph (LLHandMotion)
topic: viewer
status: ready
origin: VIEWER_ROADMAP.md — Simulator authority & the Firestorm motion model (read before P31.2)
---

Context: [context/viewer.md](../context/viewer.md).

**P31.13. Hand-pose morph (`LLHandMotion`).** The always-on adjuster split
out of P31.8 that morphs the hand between the named hand-pose animations (the
`HandPose` a `.anim` header carries, P18.1), cross-fading when the pose
changes. Needs the hand-pose animation assets and a morph/blend between two
keyframe hand poses. Reference: `LLHandMotion` in `indra/newview/`.
