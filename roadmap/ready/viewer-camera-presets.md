---
id: viewer-camera-presets
title: Saveable camera presets
topic: viewer
status: ready
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-camera-system
blocked_by: [viewer-camera-third-person-orbit, viewer-ui-settings-store]
---

Context: [context/viewer.md](../context/viewer.md).

Saveable **camera presets**: named camera offset/angle configurations (e.g. rear
view, front view, group-photo) the user can store and recall, persisted in
[[viewer-ui-settings-store]].

Reference (Firestorm, read-only): the camera presets in `llfloatercamera` /
`llpresetsmanager`.

## Parity-audit addendum (2026-08-19)

Include the **advanced camera-position editor**
(`floater_preferences_view_advanced.xml`, FS "prefs_view_advanced"): raw
XYZ vector fields for `CameraOffset` and `FocusOffset` — the values
camera presets are built from. Our
`sl-client-bevy-viewer/src/preferences_camera_move.rs` only exposes the
scalar CameraOffsetScale today; the full offset/focus vector editor
belongs with the presets work.
