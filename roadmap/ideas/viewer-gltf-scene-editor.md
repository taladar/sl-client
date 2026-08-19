---
id: viewer-gltf-scene-editor
title: GLTF scene / asset editor floater (LL experimental)
topic: viewer
status: ideas
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-pbr-material-editor, viewer-mesh-gltf-import]
---

Context: [context/viewer.md](../context/viewer.md).

Linden Lab's experimental GLTF scene editor
(`llfloatergltfasseteditor.cpp`, `floater_gltf_asset_editor.xml`,
"GLTF Scene Editor"): a hierarchy view over a GLTF asset attached to an
in-world object, with per-node transform editing — part of LL's
in-progress GLTF-objects project.

Grid-side support is still experimental/beta, so this is a tracking
item: revisit when LL ships the GLTF-object pipeline for real. Our PBR
material editor ([[viewer-pbr-material-editor]], done;
`sl-client-bevy-viewer/src/edit_material_asset.rs`,
`edit_material.rs`) already covers the materials half, and
[[viewer-mesh-gltf-import]] covers GLTF as an import format.

Reference (Firestorm, read-only):
`indra/newview/llfloatergltfasseteditor.cpp`,
`indra/newview/skins/default/xui/en/floater_gltf_asset_editor.xml`.
