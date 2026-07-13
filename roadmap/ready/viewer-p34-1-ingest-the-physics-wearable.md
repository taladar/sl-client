---
id: viewer-p34-1
title: Ingest the physics wearable
topic: viewer
status: ready
origin: VIEWER_ROADMAP.md — Phase 34 — Avatar cloth & body physics
refs: [viewer-p31-12a]
---

Context: [context/viewer.md](../context/viewer.md).

**P34.1. Ingest the physics wearable.** The `WT_PHYSICS` wearable params
— breast / belly / butt bounce driving params from `avatar_lad.xml`.

Driving the bounce visibly (a later Phase 34 step) needs the same
**per-frame visual-param morph pipeline** ([[viewer-p31-12a]]) the eye-blink
does — the current appearance pipeline bakes morphs into geometry once, so the
jiggle params cannot be animated per frame without it.
