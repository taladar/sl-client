---
id: viewer-movement-camera-input-tests
title: Movement keys and camera modes, headless
topic: viewer
status: done
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

**What was left (2026-08-31).** Everything above except the first bullet
had landed elsewhere, through real messages rather than injected
resources: the camera gestures and the mouselook / flycam halves of the
mode machine in [[viewer-camera-input-interaction-tests]]
(`world_test::camera_tests`), and the WASD-swallowing and per-mode action
resolution in [[viewer-ui-keyboard-text-harness]]
(`input_context::typed_tests`, `input_action::typed_tests`). What nobody
tested was the **outbound** half — the `ControlFlags` a held movement key
puts on the wire — which is `movement::drive_avatar_controls` and the
agent-update it feeds, not the camera.

Landed 2026-08-31 as `world_test::movement_tests` (seven), over the
existing `world_app_with_input` fold plus a `movement_app` fixture: the
own avatar streamed in as the grid would send it, a rigged third-person
camera over it, and — optionally — a land patch under it with the
parcel's fly permission granted.

## Why this is a wire test and not a transform test

Avatar motion is simulator-authoritative: the viewer never moves the
body, it advertises intent. So "does `W` walk?" is not a question about a
`Transform`, it is a question about the outbound `SetControls` /
`SetRotation` stream — which is also the half a scripted vehicle, a sit
target and every locomotion animation actually read. `movement.rs`'s own
unit tests cover its four pure helpers (`should_take_off`,
`should_auto_stop_flying`, `double_tap_run`, `rotation_from_yaw`); the
system that assembles their answers into a control word had no test at
any tier.

Everything goes through the real action map — `W` resolves under the
active binding profile — so a rebinding that stopped resolving fails
these rather than passing on an injected `Action`.

## What each check pins

- **A held walk key is advertised once, and releasing it clears the
  intent.** The silence in the middle is the load-bearing half: the
  simulator holds the last control set through its own keep-alive
  re-sends, so a viewer that re-advertised every frame would put 60
  `AgentUpdate`s a second on the circuit for a key that is merely still
  down.
- **Run needs a walk key, and a double tap latches one.** `Shift` alone
  advertises nothing (the run bit is a modifier of a walk, not a state of
  its own); `Shift` + `W` adds `FAST_AT`; and tap-tap-hold runs with no
  `Shift` at all, the latch ending with the key rather than with a timer.
- **Left / right turn the body in third person and strafe in mouselook.**
  The same key, two modes, two different things on the wire — a turn is a
  `SetRotation` at the tuned rate with no control bit, a strafe is
  `LEFT_POS` with no change of heading. Each half is asserted against the
  other's expectation, because what makes it a mode split rather than two
  behaviours is that neither leaks.
- **A seated agent steers its vehicle and its body never turns.** The
  guard on the reference's arrow-keys-orbit-the-vehicle bug: seated, left
  / right send `YAW_POS` / `YAW_NEG` and the viewer advertises *no* body
  rotation at all. Standing up again, the very same key turns and
  advertises the body — so "no rotation" is the seat, not the fixture.
- **Holding ascend takes off only where flying is permitted.** `PageUp`
  puts `UP_POS` on the wire either way; the hold-to-fly rule adds `FLY`
  only with a known floor under the avatar and the parcel's permission.
  The negative — the same key, the same 0.66 s hold, no land patch and no
  permission — is what keeps a no-fly parcel from launching someone who
  leans on the key.
- **Flycam parks the body and keeps a hovering avatar up.** Switching to
  the spectator camera drops every motion bit but keeps an advertised
  `FLY`, because clearing it would land a hovering avatar the instant the
  view changed; the paired negative makes the same switch while walking on
  the ground, where the parked set is empty. Afterwards the still-held
  walk key reaches the avatar no more.
- **The away bit rides along with the movement bits.** Away lives in the
  same control word as the walk flags, so going away while walking must
  re-advertise both — a second writer that owned the away bit alone would
  clear the walk every time it fired.

## Teeth

Three mutations of `drive_avatar_controls`, each reverted:
`controls_changed = true` (emit every frame) fails all seven; `seated =
false` fails only the vehicle test; disabling the mouselook branch fails
only the left/right mode split.

## One harness fact worth knowing

The testkit's `record::<M>` copying system is an unordered `Update`
system, so a command written by the movement driver this frame reaches
the drain on the *next* one. `hold` / `release` therefore always step at
least one frame past the press; a test that drained immediately after
`key_down` would read an empty stream and look like "the key did
nothing".
