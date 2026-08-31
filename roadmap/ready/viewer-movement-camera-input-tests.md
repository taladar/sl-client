---
id: viewer-movement-camera-input-tests
title: Movement keys and camera modes, headless
topic: viewer
status: ready
origin: user request (2026-07) — test in-world input reactions
points: 5
blocked_by: [viewer-world-test-harness]
---

Context: [context/viewer.md](../context/viewer.md).

Keyboard `Action`s (via `InputBindings`/`InputMode` and injected
`KeyboardInput`) drive locomotion; assert:

- the `ControlFlags` in outgoing agent-update `SlCommand`s for
  walk/run/jump/turn;
- mode transitions third-person ↔ mouselook ↔ flycam;
- a focused text field swallows WASD (`world_has_keyboard` gating).

Camera: inject `AccumulatedMouseMotion`/`AccumulatedMouseScroll`/
`ButtonInput` and assert the `CameraMode` machine's orbit/pan/zoom
transforms (Alt+left-drag orbit, right-drag, wheel dolly) against the
reference-faithful geometry documented in `camera.rs`.
