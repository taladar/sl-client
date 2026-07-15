---
id: viewer-input-spacenav-device
title: SpaceNavigator / 6-DOF device input
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-space-navigator
blocked_by: [viewer-input-action-map]
---

Context: [context/viewer.md](../context/viewer.md).

Read a 6-DOF spacemouse (3Dconnexion SpaceNavigator / SpaceMouse and similar
joysticks). **Use a third-party device library** — candidates: an `ndof` /
`libndofdev` binding (what the reference viewer uses), `hidapi`, SDL joystick,
or a dedicated spacenav crate. Surveying and picking one (cross-platform reach,
hot-plug, maintenance) is the first step; we do not talk to the HID device by
hand. Deliver hot-plug detection and the raw 6-axis state as a Bevy resource.

Mapping the axes onto the camera/flycam is
[[viewer-input-spacenav-camera-mapping]]; the settings UI is
[[viewer-input-spacenav-settings-ui]].

Reference (Firestorm, read-only): `llviewerjoystick.cpp/h` (the NDOF flycam).
