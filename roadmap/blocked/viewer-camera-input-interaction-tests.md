---
id: viewer-camera-input-interaction-tests
title: Camera, movement, mouselook and HUD picking under a synthetic pointer
topic: viewer
status: blocked
origin: test-harness plan (2026-08-30)
points: 5
refs: [viewer-ui-interaction-harness]
blocked_by: [viewer-world-test-harness, viewer-cpu-pick-resolver]
---

Context: [context/testing.md](../context/testing.md).

`camera.rs` has ten tests, none of which moves the mouse. With the
synthetic pointer and the fixture world:

- Alt-drag orbits the third-person rig, Ctrl+Alt-drag changes elevation,
  a plain drag leaves it alone; the wheel zooms and crossing the minimum
  distance enters mouselook; a wheel over a blocking panel does nothing.
- Mouselook aims from raw `MouseMotion` (no `CursorMoved`), grabs the
  cursor, and a focused text field frees it.
- Keys resolve to actions per mode through real `KeyboardInput` messages
  (not `ButtonInput<KeyCode>` poked directly); a focused text entry
  releases every action; flycam moves with actions and mouse.
- The camera stops at a fixture wall placed in the dynamic colliders.
- HUD: a left click on a HUD face touches it and never asks the world; a
  HUD under the cursor occludes a right-click; a click beside the HUD
  falls through to a world pick request.
