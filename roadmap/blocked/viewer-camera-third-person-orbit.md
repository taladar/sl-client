---
id: viewer-camera-third-person-orbit
title: Camera mode machine + third-person orbit
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-camera-system
blocked_by: [viewer-input-action-map]
---

Context: [context/viewer.md](../context/viewer.md).

Introduce the **camera-mode state machine** and the **third-person** mode:
orbit / pan / zoom around the avatar. The camera consumes input **actions**
([[viewer-input-action-map]]) rather than raw keys, and sets the viewport that
render priority / LOD already report against. This is the root of the camera
cluster — mouselook, flycam, collision, focus and presets all extend the mode
machine it introduces.

The debug fly-camera in `camera.rs` becomes a first-class **flycam** mode in
[[viewer-camera-flycam]]; this task carves out the mode-machine scaffolding it
plugs into.

Reference (Firestorm, read-only): `llagentcamera.cpp/h` (mode machine, orbit /
pan / zoom), `llfloatercamera`.

Builds on: the existing `camera.rs` fly-camera and `session.rs` viewport
reporting.

Clean-up when this lands: the debug fly-camera grabs and hides the cursor for
mouse-look at all times, which is why P35.3 ([[viewer-p35-3]]) had to add an
`H`-toggled "HUD cursor mode" (`hud_pick::HudCursorMode`) just to free a pointer
to click a HUD with. The real SL model is the inverse — the
cursor is free by default and mouselook is the special mode — so once
third-person / mouselook exist the toggle is redundant: remove `HudCursorMode`
(and its `fly_camera` gate) and let clicks pick the HUD directly,
HUD-before-world, whenever the cursor is free.
