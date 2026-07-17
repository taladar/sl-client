---
id: viewer-camera-focus-on-object
title: Focus / alt-zoom on object
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-camera-system
blocked_by: [viewer-camera-third-person-orbit]
---

Context: [context/viewer.md](../context/viewer.md).

**Focus-on-object**: alt-zoom / focus the camera on a picked object or point, so
orbit and zoom pivot around it instead of the avatar. Reuses the existing `P`
crosshair-pick raycast (`pick_object`) to choose the focus target.

Reference (Firestorm, read-only): `lltoolfocus` (alt-zoom), `llagentcamera`
`setFocusOnAvatar(false)` + `setCameraPosAndFocusGlobal`.

## Done

`src/camera.rs` `focus_on_object` + the `FocusTarget::Point` path. **Alt + left
click** in third person picks the world point under the cursor (reusing the pick
ray) and pivots orbit / zoom around it instead of the avatar — storing the
world-space offset from the point to the current eye so the camera **does not
jump** (the reference's `setFocusGlobal`). Orbit rotates that offset around the
point; moving the avatar returns the focus to it. The **endless ocean / water is
excluded from this pick only** (it covered every ray, so alt-clicks landed on a
distant sea-level point) — the touch / `P` / rez picks still hit water. The
avatar's actual gaze at a focused object (and the own-avatar-by-entity case) is
part of [[viewer-lookat-faithful]].
