---
id: viewer-p34-2
title: Drive them
topic: viewer
status: ready
origin: VIEWER_ROADMAP.md — Phase 34 — Avatar cloth & body physics
---

Context: [context/viewer.md](../context/viewer.md).

**P34.2. Drive them.** Port `LLPhysicsMotion` /
`LLPhysicsMotionController` (a spring-damper per param, driven by joint
acceleration, built on the Phase 31 physics foundation) as a motion in the
Phase 18 animation controller, folding the resulting param weights into the
avatar morphs each frame. Reference: `llphysicsmotion.cpp`.
