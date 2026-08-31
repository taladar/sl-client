---
id: viewer-camera-input-interaction-tests
title: Camera, movement, mouselook and HUD picking under a synthetic pointer
topic: viewer
status: done
origin: test-harness plan (2026-08-30)
points: 5
refs: [viewer-ui-interaction-harness, viewer-ui-keyboard-text-harness]
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

Landed 2026-08-31 as `world_test::camera_tests` (seven) and
`world_test::hud_click_tests` (two), over a new `world_app_with_input`
fold — the input group on top of the world fixture — and its UI
companion `world_app_with_ui_and_input`.

## What it took to stand the input group up headlessly

Three things the world fold never needed, each of which reads as "the
feature is broken" rather than "the harness is short" when missing:

- **`CursorOptions` on the window.** The testkit spawns its window with
  `primary_cursor_options: None`, so `drive_cursor_grab`'s query is empty
  and *every* assertion about the pointer being captured passes as "not
  grabbed", whatever mouselook did.
- **A `CameraRig` on the camera.** `install_camera` gives the pick paths a
  projection and nothing else; all three mode drivers query
  `&mut CameraRig`, so a camera without one is simply invisible to them.
  `install_camera_rig` spawns the pair the viewer's `setup_scene` does.
- **No 6-DOF device read.** `SpacenavPlugin` enumerates the device straight
  off `evdev` — it is the one input seam that does not arrive through the
  window — so on a machine with a SpaceNavigator plugged in (this one) the
  fixture world would be steered by an idle puck, and its first button
  would drop the camera into flycam mid-assertion. The plugin now carries a
  `DeviceRead`, `ViewerInputPlugins::without_devices()` takes the
  publish-only half, and the fixture world uses it — the same "a headless
  app takes the ECS half of a split plugin" shape as `PickStack::Cpu`.

## What each check pins

- **Alt arms the orbit and nothing else does** — an `Alt`-held horizontal
  drag swings the azimuth by the reference's 0.003 rad/px and touches
  neither zoom nor tilt; the same pixels with no modifier (the *touch* and
  rubber-band gesture) leave the whole rig alone.
- **Ctrl swaps the vertical drag from zoom to elevation** — both
  directions of the swap, so it is one test rather than two halves.
- **The wheel zooms, and zooming in crosses into mouselook** — the
  zoom-through has no key of its own; this gesture is the only way a user
  reaches first person by mouse.
- **A wheel over a blocking panel leaves the camera alone**, with the
  control run second so "the camera did not move" cannot be the notch
  never having arrived.
- **Mouselook aims from raw motion and third person does not** — one
  `MouseMotion` with no `CursorMoved`, two modes, opposite verdicts.
- **Mouselook grabs the cursor and leaving it frees it**, driven through
  the real `M` toggle both ways; and **a focused field frees the pointer**
  without leaving mouselook (the grab follows focus, the view does not).
- **The flycam flies on the movement keys and aims on a right-drag** —
  `W` translates along the camera's own forward, a right-held drag turns
  it, the same motion with no button does not. The mode is set directly:
  `Action::ToggleFlycam` has no key in any binding profile and its only
  real source is the device button this fixture deliberately does not read.
- **The camera pulls in for a wall and not for one behind it** — a
  `FLAGS_USE_PHYSICS` prim (so its collider is in the per-frame moving set,
  with no wait on the off-thread static BVH) placed on the head→eye
  segment pulls the eye in, and the same prim past the eye — outside the
  segment the cast is bounded to — does not. The rig's own `distance` is
  unchanged either way: the pull is the pose, not the zoom.
- **A left click touches the HUD and a click beside it reaches the
  world** — the HUD's orthographic ray answers in the same frame and
  nothing follows it, and the click beside resolves through the pick queue
  a frame or two later onto the prim.
- **A HUD occludes the right-click of the prim behind it** — the object
  pie never opens and the HUD pie does. The control is a *second* world
  rather than the same one with the HUD removed, because an open pie draws
  its own blocking ring over the cursor.

Two of the bullets above were already answered by
[[viewer-ui-keyboard-text-harness]] and are not repeated here: per-mode
action resolution from real `KeyboardInput`
(`input_action::typed_tests`) and a focused text entry releasing the
world's actions (`input_context::typed_tests`).

Three supporting moves, each small: `FLAGS_USE_PHYSICS` joined the other
`PrimFlags` bits in `world_api` (its own comment already said the bits
live there rather than in the module that parses the word);
`seed_attachment_scaled` lets a HUD fixture wear a 10 cm cube, since the
HUD camera shows one world unit vertically and the shared 2 × 3 × 4 m
seed covers the screen several times over; and the HUD tests aim at
`scene_position_of(prim)` rather than the first tagged face, which with an
avatar and a HUD in the scene is not the prim's.
