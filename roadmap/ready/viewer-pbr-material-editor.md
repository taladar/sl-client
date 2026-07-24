---
id: viewer-pbr-material-editor
title: PBR / GLTF material editor
topic: viewer
status: ready
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

**Partly covered (2026-07-24) by [[viewer-face-materials-pbr]].** The
build-menu-reachable per-channel editing already ships in the Texture tab:
base / metallic-roughness / emissive / normal texture pickers, base + emissive
tints, metallic / roughness factors, alpha mode / cutoff, double-sided,
per-channel transforms, **New** (blank material), and **Save** to inventory
(`encode_material_asset` → `UploadAsset`), all applied live as face GLTF
overrides. What remains for this task: the **dedicated editor floater**
(`floater_material_editor` / `floater_live_material_editor` as a standalone
window rather than the inline Texture-tab section), a real **material picker**
(folded into [[viewer-inventory-materials-not-shown]]), the
material-on-a-sphere **preview** ([[viewer-material-swatch-sphere-preview]]),
and **auto-assigning** a just-Saved material back onto the selected face.
