---
id: viewer-phase-31-general-physics
title: Phase 31 — General physics foundation (avian3d)
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 31 — General physics foundation (`avian3d`)
---

Context: [context/viewer.md](../context/viewer.md).

Flexi prims (Phase 32) and avatar body physics (Phase 34) are client-side
simulations. Rather than hand-rolling a solver for each, stand up a shared
physics substrate on the `avian3d` Bevy physics engine first.
