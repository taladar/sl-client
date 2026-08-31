---
id: viewer-movement-camera-input-tests
title: Movement keys and camera modes, headless
topic: viewer
status: ready
origin: user request (2026-07) — test in-world input reactions
points: 5
refs: [viewer-camera-input-interaction-tests, viewer-ui-keyboard-text-harness]
blocked_by: [viewer-world-test-harness]
---

Context: [context/viewer.md](../context/viewer.md),
[context/testing.md](../context/testing.md).

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

**What is left (2026-08-31).** Everything above except the first bullet
has landed elsewhere, through real messages rather than injected
resources: the camera gestures and the mouselook / flycam halves of the
mode machine in [[viewer-camera-input-interaction-tests]]
(`world_test::camera_tests`), and the WASD-swallowing and per-mode action
resolution in [[viewer-ui-keyboard-text-harness]]
(`input_context::typed_tests`, `input_action::typed_tests`). What nobody
tests is the **outbound** half — the `ControlFlags` a held movement key
puts on the wire — which is `movement::drive_avatar_controls` and the
agent-update it feeds, not the camera.
