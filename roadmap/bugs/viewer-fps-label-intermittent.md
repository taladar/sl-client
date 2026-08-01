---
id: viewer-fps-label-intermittent
title: FPS status readout intermittently drops its "fps" label
topic: viewer
status: bugs
origin: user report during the world-frustum-culling profiling session
  (2026-08-01)
refs: [viewer-perf-world-frustum-culling-octree]
---

Context: [context/viewer.md](../context/viewer.md).

The on-screen FPS readout (the P19.1 diagnostics HUD / status-bar frame-rate
figure, `diagnostics.rs`) does not show the literal `fps` unit string all the
time — the number renders but the `fps` suffix is intermittently missing.

Likely a formatting / text-update path that rewrites the readout each frame and
occasionally emits only the number (e.g. a branch that writes the value without
the unit, a smoothed-value-not-yet-ready path, or width elision clipping the
suffix). Reproduce by watching the readout live; fix so the `fps` label is
always present alongside the value.

Low priority / cosmetic, but it is a persistent diagnostics-HUD glitch.
