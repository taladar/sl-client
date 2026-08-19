---
id: viewer-input-spacenav-settings-ui
title: SpaceNavigator settings panel
topic: viewer
status: ready
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-space-navigator
blocked_by: [viewer-input-spacenav-camera-mapping, viewer-ui-settings-binding]
---

Context: [context/viewer.md](../context/viewer.md).

The joystick / 6-DOF **device-axis** settings panel: per-axis mapping,
sensitivity, dead-zone and invert for [[viewer-input-spacenav-camera-mapping]],
bound to the settings store via [[viewer-ui-settings-binding]]. This configures
the input *device*; it is distinct from [[viewer-camera-flycam-floater]], which
controls the camera *mode*.

Reference (Firestorm, read-only): `llfloaterjoystick.cpp/h`.

## Parity-audit addendum (2026-08-19)

Three reference joystick behaviours belong in this panel beyond the
already-committed per-axis settings (`spacenav.rs` has axes 0-5 with
dead-zone / scale / feathering): the zoom axis itself (`JoystickAxis6` —
a seventh mappable axis driving camera zoom) with its behaviour pair
`ZoomDirect` / `ZoomTime` (direct zoom versus time-smoothed), and
`JoystickBuildEnabled` — enabling 6-DOF device input for object
manipulation in build/edit mode (move the selected object with the
device), a third consumer next to avatar motion and flycam.
