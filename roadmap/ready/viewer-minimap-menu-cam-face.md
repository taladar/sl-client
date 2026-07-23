---
id: viewer-minimap-menu-cam-face
title: Minimap context menu — Cam / Face towards avatar
topic: viewer
status: ready
origin: split from viewer-minimap-interactions (2026-07-23)
blocked_by: []
refs: [viewer-minimap-interactions]
---

Context: [context/viewer.md](../context/viewer.md).

The reference's minimap avatar entries **Cam** (zoom the camera onto
the avatar, `LLAvatarActions::zoomIn`) and **Face towards avatar**
(turn the own avatar to face them). Our camera already has a
point-focus mode (`camera.rs` `FocusTarget::Point`, the alt-zoom
re-pivot) — Cam is that focus set to the avatar's position plus a
dolly-in, ideally tracking the avatar entity rather than a stale
point. Face-towards turns the body heading (the movement module's
facing) toward the target, without moving.

Enable rules from the reference: Cam only when the avatar is within
zoom range; Face towards only for another (not self), living avatar
within a distance cap. Wire both through a shared avatar-action layer
so the radar and people panel can reuse them.

Reference (Firestorm, read-only): `llnetmap.cpp` (`handleCam`,
`handleFaceTowards`, `canFaceTowards`), `llavataractions.cpp`
(`zoomIn`, `canZoomIn`).
