---
id: viewer-input-spacenav-avatar-motion
title: SpaceNavigator drives avatar walking / turning outside flycam
topic: viewer
status: done
origin: user request during the R23 verification session (2026-07-23)
refs: [viewer-input-spacenav-device, viewer-input-spacenav-settings-ui]
---

Context: [context/viewer.md](../context/viewer.md).

The 6-DOF device ([[viewer-input-spacenav-device]]) currently only flies the
camera in flycam mode. When **not** in flycam, the reference viewer maps the
device onto **avatar motion**: push forward/back = walk forward/back, twist
(yaw) = turn, pull up = jump / fly up, push down = crouch / fly down — see
`LLViewerJoystick::moveAvatar` (`llviewerjoystick.cpp`), including its
dead-zone / axis-scale settings and the run threshold.

Scope:

- Map the spacenav axes onto the existing avatar-movement control state
  (the same agent-update flags the keyboard drives), active whenever the
  device is present and flycam is off.
- Respect the per-axis enable/scale/dead-zone settings surface planned in
  [[viewer-input-spacenav-settings-ui]] (reference `JoystickAxis*`,
  `AvatarAxisScale*`, `AvatarAxisDeadZone*`).
- Keyboard/spacenav compose the way the reference composes them (either
  source can move the avatar; neither blocks the other).

## Done

`spacenav.rs` `avatar_nav_drive` + its consumer `movement.rs`
`drive_avatar_controls`. When flycam is off, the device's forward axis walks
(sign past the dead-zone → `AT_POS` / `AT_NEG`), its up axis flies up / down —
the same intent PageUp / PageDown express, so it composes with the existing
hold-to-take-off / auto-land (`UP_POS` / `UP_NEG`) — and its twist turns the
body (feathered per frame via the reference `sDelta[RY]` ramp). A forward push
past the run threshold runs (`FAST_AT`), with the reference's one-frame
hysteresis. Everything OR-composes with the keys (neither source blocks the
other). While **seated on a vehicle** the same axes send the vehicle control
bits — forward / back, up / down, and the twist as the `YAW_POS` / `YAW_NEG`
steer — so a scripted vehicle can be flown with the device through the very
`AgentUpdate` a script reads from the keys.

Settings are the reference's own (`AvatarAxisScale0..5` /
`AvatarAxisDeadZone0..5` / `AvatarFeathering` / `JoystickRunThreshold`),
defaulted to the SpaceNavigator-on-Linux values and persisted under
`[spacenav.avatar]`, so a Firestorm user's values port over; the settings UI is
[[viewer-input-spacenav-settings-ui]]. The mapping is the walk / turn / fly-up
/ down the roadmap describes; strafe and pitch keep their reference defaults for
a later, fuller mapping. Client-side unit tests cover the sign / dead-zone /
feathering / run-hysteresis of `avatar_nav_drive`; the full 6-DOF injection-seam
test is [[viewer-spacenav-input-tests]] (blocked on the world test harness).
