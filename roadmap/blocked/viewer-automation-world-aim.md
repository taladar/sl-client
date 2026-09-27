---
id: viewer-automation-world-aim
title: World actions — pick-verified aiming, reveal, click and pie
topic: viewer
status: blocked
origin: viewer automation design review (2026-09-28)
points: 8
blocked_by: [viewer-automation-world-model, viewer-automation-synthetic-input]
refs: [viewer-cpu-pick-resolver]
---

Context: [context/automation.md](../context/automation.md).

A click on an object must land on *that* object, not on whatever happens to
be in front of it, and a test must not need to know where the camera is.

## Wanted

- Screen projection for a `WorldNode`: on screen or not, projected bounds.
- An **aim point verified by the pick resolver** — the GPU pick
  (`gpu_pick.rs`, `GpuPickResolved`) live, the CPU resolver in headless
  tiers — to hit this object; `covered` when no candidate point does.
- **Reveal**: when no verified point is on screen, frame the object with
  the camera (the viewer's focus-on-object path) and re-aim; restore is not
  implied — the test sees the camera where reveal left it.
- Actions over the synthetic input: click / touch, right-click (the object
  pie opens with this object as its target), hover (hover text shows),
  select in build mode, drag.

Acceptance: in a `WorldTest` fixture a prim hidden behind another is
reported covered and a click on it reveals, then hits it; a right-click
opens the object pie whose stashed target is that prim.
