---
id: viewer-avatar-alignment-tools
title: Face-nearest and avatar fine-alignment tools
topic: viewer
status: ready
origin: main-menu survey (2026-07-23)
refs: [viewer-attachment-align, viewer-p31-5]
---

Context: [context/viewer.md](../context/viewer.md).

Two Firestorm posing helpers under Avatar ▸ Movement: "Face nearest
avatar" rotates the own avatar to face the closest avatar, and "Avatar
alignment" opens a nudge floater that applies fine incremental
position/rotation offsets to the own avatar for lining up precisely with
another (posing, photography, couples animations).

Scope:

- Face-nearest action: pick the nearest rendered avatar and issue the
  body rotation to face them.
- Alignment floater: small step buttons (±position on each axis,
  ±rotation) sending incremental own-avatar adjustments the way the
  reference does, with a step-size control.
- Distinct from [[viewer-attachment-align]] (attachment alignment).

Reference (Firestorm, read-only): `Avatar.FaceNearest`,
`Avatar.AlignToggle` (`menu_viewer.xml` Avatar ▸ Movement) and the FS
alignment floater implementation.

Builds on: avatar movement controls ([[viewer-p31-5]], done).
