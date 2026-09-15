---
id: viewer-wasd-moves-flycam-in-world
title: WASD appears to drive the flycam during normal play (debug-camera leftover?)
topic: viewer
status: done
origin: user report (2026-08-07), noticed during double-click-teleport live testing
refs: [viewer-camera-flycam, viewer-input-action-map]
---

Context: [context/viewer.md](../context/viewer.md).

During ordinary (third-person) play the **WASD** keys seem to move the
**flycam** rather than (or in addition to) walking the avatar — the user
suspects a **leftover from the early debug fly-camera**.

By design the per-[`InputMode`] binding profiles
(`input_action.rs` `InputBindings::default`) bind WASD to the *avatar* in
`ThirdPerson`/`Mouselook` and to the *camera* only in `Flycam`
([[viewer-camera-flycam]]).

Reference (Firestorm, read-only): the avatar-driving vs. flycam key split in
`keys.xml` / the movement controller.

## What it turned out to be

**Not a leftover debug camera.** Nothing outside the mode-gated `drive_flycam`
reads `KeyCode::KeyW`/`A`/`S`/`D` or the `Action::Move*` actions for a camera:
the only writers of the `ViewerCamera` transform are `camera.rs`'s own drivers,
`terrain.rs`'s region-crossing rebase, and two env-gated debug framings
(`SL_VIEWER_PARTICLE_FOCUS`, `SL_VIEWER_VOLUME_FOCUS`). `drive_flycam` is gated
on the mode twice over — a `resource_equals(CameraMode::Flycam)` run condition
*and* its own first line — and `orbit_third_person` never reads the keyboard for
anything but the `Alt` / `Ctrl` modifiers.

**The mode was wrong, and nothing said so.** In the flycam the same keys
legitimately fly the camera, and in an ordinary session there was exactly *one*
way into that mode: the **6-DOF device's first button**
([[viewer-input-spacenav-camera-mapping]], on by default since 2026-07-21).
`Action::ToggleFlycam` existed but no binding profile bound it, no menu entry
raised it, and no log line was written when the mode changed — so a stray press
of a puck on the desk silently reassigned the whole movement cluster from the
avatar to the camera. The only exits were `Escape` and the *Stop flycam* state
button, whose layout was itself broken until 74d2f78f (2026-08-15, a week after
this report).

## Done

- **Advanced ▸ Shortcuts ▸ Joystick Flycam**, checked while in the flycam, on
  the reference's own `Alt+Shift+F`. That is where the reference keeps it:
  Firestorm has no *View* menu, and everything the Linden viewer files there —
  Mouselook, Joystick Flycam, Reset View, the Zooms — lives in
  `Advanced ▸ Shortcuts`. The submenu is new, holds this one entry, and the rest
  of the reference's eighteen is [[viewer-menu-advanced-shortcuts]].

  It is the keyboard way in *and* out, and its check mark is the one place the
  viewer states which mode the movement keys are driving. It writes a
  `ToggleFlycam` message rather than assigning `CameraMode` directly, so
  `switch_camera_mode` keeps owning the rig seeding (entering) and the resnap
  (leaving) that make the switch continuous one way and a warp the other.
- **Every camera-mode change is logged with the input that caused it** — the
  flycam key, the menu entry, the 6-DOF button, `Escape`, the Stop flycam button
  — so the next report of this shape is answerable from the log.
- Regression tests:
  `camera_tests::the_movement_keys_do_not_touch_the_camera_outside_the_flycam`
  (the whole cluster held in third person leaves the transform untouched and the
  mode unchanged, with the same keystroke in flycam as the control that says the
  key arrived),
  `camera_tests::a_held_movement_key_keeps_the_camera_on_its_orbit` (a walking
  avatar's camera follows without leaving the orbit — the "in addition to" half
  of the report), and
  `camera::tests::a_toggle_flycam_request_enters_and_leaves_the_flycam`.
