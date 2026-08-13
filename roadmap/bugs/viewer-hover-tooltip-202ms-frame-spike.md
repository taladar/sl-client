---
id: viewer-hover-tooltip-202ms-frame-spike
title: update_hover_tooltip spikes to 202 ms on a single frame
topic: viewer
status: bugs
origin: GPU-avatar Phase 4 perf capture (2026-08-13)
refs: [viewer-perf-hover-pick-raycast, viewer-perf-gpu-avatar-phase3-gpu-picking]
---

Context: [context/viewer.md](../context/viewer.md); code
`sl-client-bevy-viewer/src/hover_tooltip.rs`.

In a 27.6 s / 512-frame aditi tracy capture, `update_hover_tooltip` had
**mean 0.5 ms but max 202.9 ms** on a single frame (one instance out of 509).
That is abnormal: there is only ever **one** hover tooltip, so this system
should be trivial and near-constant. A 200 ms hitch in it is a
single-digit-FPS frame on its own.

## Why it's suspicious

Phase 3 moved hovering onto the async GPU ID-buffer pick
([[viewer-perf-gpu-avatar-phase3-gpu-picking]]), which is supposed to have
retired the old `MeshRayCast` cost ([[viewer-perf-hover-pick-raycast]]) and be
~0. A 202 ms spike means something occasionally does heavy work on this path.

## Candidate causes (to confirm by repro + a zoom on that frame)

- A **synchronous stall** somewhere on the hover path (a blocking
  readback/map wait, or a fallback that still ray-casts) on the frame the
  pick pipeline is still compiling or the readback is late.
- A **tooltip UI rebuild** — despawn+respawn of the tooltip widget/text on
  content change (the "build once, update in place" rule — see
  `sl-client-floater-build-once-update-in-place`); a full text relayout of a
  large tooltip would show here.
- Coincidence with an **asset-streaming stall** (see the sibling perf task):
  the spike may just be this system caught behind a big upload on the same
  frame — rule this out first by checking whether the 202 ms is *self* time or
  inherited wait.

## Verify

Reproduce with a tracy capture, find the single spiking instance, zoom to that
frame, and read this zone's **self** time and children. If self-time is high,
fix the heavy work (make it constant-time / build-once). If it's inherited
wait, reclassify as the asset-streaming spike, not a hover bug.
