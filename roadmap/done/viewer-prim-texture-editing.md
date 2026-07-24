---
id: viewer-prim-texture-editing
title: Prim texture / material editing
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07)
blocked_by: [viewer-object-edit-floater-shell, viewer-edit-face-selection]
---

Context: [context/viewer.md](../context/viewer.md).

The texture / material tab of the edit floater
([[viewer-object-edit-floater-shell]]): per-face texture, colour, transparency,
repeats / offset / rotation, bump / shiny / glow / fullbright, and assigning a
legacy or GLTF / PBR material — applied to the whole selection or to the faces
picked by the Select Face tool ([[viewer-edit-face-selection]], split out).

Reference (Firestorm, read-only): `llpanelface`, `lltoolface`; messages
`ObjectImage`, `RenderMaterials`.

Builds on: `materials.rs`, `legacy_materials.rs`, `textures.rs`.

## Done (2026-07-24)

The diffuse half shipped earlier (commit `c721ffb3`: per-face texture / colour /
transparency / repeats / offset / rotation / bump / shiny / glow / fullbright /
mapping over `ObjectImage`, applied to the Select-Face selection). The remaining
piece — **assigning a legacy or GLTF/PBR material to faces**, plus the whole
normal/specular + PBR editing surface — landed with
[[viewer-face-materials-pbr]] (see its Done note): the matmedia /
material-type / pbr-type selectors, the Blinn-Phong `RenderMaterials` PUT, and
the PBR override / material-editor controls.
