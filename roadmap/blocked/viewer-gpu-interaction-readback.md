---
id: viewer-gpu-interaction-readback
title: Pixel-level reaction spot checks (GPU, serial)
topic: viewer
status: blocked
origin: user request (2026-07) — end manual re-testing of UI interactions
points: 3
blocked_by: [viewer-edit-gizmo-interaction-tests,
  viewer-edit-selection-interaction-tests]
---

Context: [context/viewer.md](../context/viewer.md).

A thin SERIAL suite on `render_readback.rs`'s offscreen `RenderTarget` +
gpu_readback pattern (self-skipping loudly without a GPU adapter, as
today):

- after a headless click-select, the selection highlight overlay actually
  lights pixels;
- after entering edit with a selection, the gizmo rig renders on its
  overlay layer;
- after a translate drag, the object's pixels moved.

Deliberately few cases: everything provable without a GPU is proven in
the headless tiers; this catches only the "state changed but nothing
drew" class.
