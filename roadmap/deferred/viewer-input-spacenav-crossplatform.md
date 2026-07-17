---
id: viewer-input-spacenav-crossplatform
title: SpaceNavigator support on Windows / macOS
topic: viewer
status: deferred
origin: split from viewer-input-spacenav-device during the camera-system pass
blocked_by: [viewer-input-spacenav-device]
---

Context: [context/viewer.md](../context/viewer.md).

The SpaceNavigator / 6-DOF read ([[viewer-input-spacenav-device]]) is currently
**Linux-only**, behind the `spacenav` feature, reading `evdev`
(`/dev/input/event*`). Add Windows and macOS backends behind the same
`SpacenavInput` resource so the flycam mapping
([[viewer-input-spacenav-camera-mapping]]) works cross-platform.

Candidates: a `hidapi` backend (the raw 3Dconnexion HID reports are
well-documented and the same across platforms), the vendor `3DxWare` SDK, or a
maintained `ndof` / `libndofdev` binding (what the reference viewer uses).
Survey and pick per the device task's own note. The device layer already
normalises axes and publishes the button edge, so only the platform read needs
adding.

Reference (Firestorm, read-only): `indra/newview/llviewerjoystick` (NDOF).
