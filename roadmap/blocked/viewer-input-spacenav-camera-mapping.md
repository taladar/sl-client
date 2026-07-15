---
id: viewer-input-spacenav-camera-mapping
title: SpaceNavigator → camera/flycam mapping
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-space-navigator
blocked_by: [viewer-input-spacenav-device, viewer-camera-flycam, viewer-ui-settings-store]
---

Context: [context/viewer.md](../context/viewer.md).

Map the six axes from [[viewer-input-spacenav-device]] onto camera motion —
primarily the **flycam** ([[viewer-camera-flycam]]), and the third-person camera
— with per-axis **sensitivity**, **dead-zone** and **invert** settings persisted
in [[viewer-ui-settings-store]]. The flycam is the natural 6-DOF surface (free
translate + rotate), which is why this maps there first.

Reference (Firestorm, read-only): `llviewerjoystick` (axis → flycam delta).
