---
id: viewer-input-spacenav-settings-ui
title: SpaceNavigator settings panel
topic: viewer
status: blocked
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
