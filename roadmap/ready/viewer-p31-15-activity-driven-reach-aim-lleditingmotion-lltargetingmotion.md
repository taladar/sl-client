---
id: viewer-p31-15
title: Activity-driven reach / aim (LLEditingMotion / LLTargetingMotion)
topic: viewer
status: ready
origin: VIEWER_ROADMAP.md — Simulator authority & the Firestorm motion model (read before P31.2)
---

Context: [context/viewer.md](../context/viewer.md).

**P31.15. Activity-driven reach / aim (`LLEditingMotion` /
`LLTargetingMotion`).** The activity group split out of P31.8, needing
selection / target state the viewer does not track. `LLEditingMotion` reaches
the hand toward the object the agent has selected / is editing;
`LLTargetingMotion` aims the arm at a target (mouselook aim / the point
gesture). Both are locally driven and want a limb IK reach (shares the P31.14
solver). Reference: the two motion classes in `indra/newview/`.
