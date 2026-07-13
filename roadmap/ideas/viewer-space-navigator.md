---
id: viewer-space-navigator
title: 3Dconnexion SpaceNavigator / 6-DOF input
topic: viewer
status: ideas
origin: reference-viewer feature-cluster survey (2026-07)
---

Context: [context/viewer.md](../context/viewer.md).

Support a 6-DOF spacemouse (3Dconnexion SpaceNavigator / SpaceMouse and similar
joysticks) for camera and avatar flycam control, with per-axis mapping,
sensitivity, dead-zone, and invert settings.

**Use a third-party device library** — candidates: an `ndof` / `libndofdev`
binding (what the reference viewer uses), `hidapi`, SDL joystick, or a dedicated
spacenav crate. Surveying and picking one (cross-platform reach, hot-plug,
maintenance) is a first fleshing-out step; we do not talk to the HID device by
hand.

Reference (Firestorm, read-only): `llviewerjoystick.cpp/h` (the NDOF flycam),
`llfloaterjoystick.cpp/h` (the settings UI).

Deps: [[viewer-input-system]], [[viewer-camera-system]].
