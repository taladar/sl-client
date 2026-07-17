---
id: viewer-input-spacenav-camera-mapping
title: SpaceNavigator → camera/flycam mapping
topic: viewer
status: done
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

## Done

`src/camera.rs` `drive_flycam` + `src/spacenav.rs`. The six axes map onto the
flycam via the reference's exact `moveFlycam` pipeline: per-axis
**soft dead-zone** (subtract), **scale**, frame-time, and **feathering** (the
`sDelta` ramp), with the translation applied in the camera-local frame and the
rotation composed local + AutoLeveled. The settings are the reference's own
(`FlycamAxisScale0..5` / `FlycamAxisDeadZone0..5` / `FlycamFeathering` /
`AutoLeveling`), defaulted to the **SpaceNavigator-on-Linux** values
(`setSNDefaults` with `platformScale = 20`), so a user's Firestorm values port
straight over; they persist via the wired `viewer-ui-settings-store`
(`crate::settings`). The device's first button toggles flycam. The settings UI
is [[viewer-input-spacenav-settings-ui]]; the persisted file's TOML/comment
format is [[viewer-settings-toml-format]].
