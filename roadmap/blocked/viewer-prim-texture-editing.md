---
id: viewer-prim-texture-editing
title: Prim texture / material editing
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07)
blocked_by: [viewer-object-edit-floater-shell]
---

Context: [context/viewer.md](../context/viewer.md).

The texture / material tab of the edit floater
([[viewer-object-edit-floater-shell]]): per-face texture, colour, transparency,
repeats / offset / rotation, bump / shiny / glow / fullbright, and assigning a
legacy or GLTF / PBR material — with face-select to target a single face.

Reference (Firestorm, read-only): `llpanelface`, `lltoolface`; messages
`ObjectImage`, `RenderMaterials`.

Builds on: `materials.rs`, `legacy_materials.rs`, `textures.rs`.
