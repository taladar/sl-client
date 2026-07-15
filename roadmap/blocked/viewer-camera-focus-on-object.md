---
id: viewer-camera-focus-on-object
title: Focus / alt-zoom on object
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-camera-system
blocked_by: [viewer-camera-third-person-orbit]
---

Context: [context/viewer.md](../context/viewer.md).

**Focus-on-object**: alt-zoom / focus the camera on a picked object or point, so
orbit and zoom pivot around it instead of the avatar. Reuses the existing `P`
crosshair-pick raycast (`pick_object`) to choose the focus target.

Reference (Firestorm, read-only): `lltoolfocus` (alt-zoom), `llagentcamera`
`setFocusOnAvatar(false)` + `setCameraPosAndFocusGlobal`.
