---
id: viewer-input-spacenav-avatar-motion
title: SpaceNavigator drives avatar walking / turning outside flycam
topic: viewer
status: ready
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
