---
id: viewer-pbr-material-editor
title: PBR / GLTF material editor
topic: viewer
status: blocked
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-texture-picker, viewer-ui-color-picker]
refs: [viewer-prim-texture-editing, viewer-image-upload]
---

Context: [context/viewer.md](../context/viewer.md).

The GLTF material editor: create and edit **material assets**
(`AssetType::Material` — base colour + texture, metallic/roughness,
normal, emissive, alpha mode/cutoff, double-sided) and save them to
inventory via the material-upload cap, plus the **live editor** variant
that edits the material override on selected in-world faces directly
(`RenderMaterials` override path, `protocol-64` pairing). Texture slots
pick via [[viewer-ui-texture-picker]], colours via
[[viewer-ui-color-picker]]; assigning a saved material to faces belongs to
[[viewer-prim-texture-editing]].

Reference (Firestorm, read-only): `llmaterialeditor`,
`floater_material_editor.xml`, `floater_live_material_editor.xml`,
`llgltfmateriallist`.

Builds on: `protocol-25` GLTF materials + `protocol-64` materials service,
`sl-material`.

Deps: [[viewer-ui-texture-picker]], [[viewer-ui-color-picker]].
