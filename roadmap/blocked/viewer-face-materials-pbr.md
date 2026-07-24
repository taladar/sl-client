---
id: viewer-face-materials-pbr
title: Texture tab — Blinn-Phong normal/specular maps + PBR (GLTF) materials
topic: viewer
status: blocked
origin: user request (2026-07-24) while reviewing the build-tool Texture tab
blocked_by: [viewer-prim-texture-editing, viewer-ui-texture-picker]
refs: [viewer-prim-texture-editing]
---

Context: [context/viewer.md](../context/viewer.md).

The Texture tab shipped with [[viewer-prim-texture-editing]] edits only the
**diffuse** legacy channel (the `TextureEntry` `texture_id`, tint, repeats /
offset / rotation, bump / shiny / glow / fullbright / mapping). The reference's
`LLPanelFace` also edits, via the **"combobox matmedia"** mode selector and the
**`radio_material_type`** map-channel radio:

- **Blinn-Phong (legacy `LLMaterial`)**: a **normal (bumpiness)** map and a
  **specular (shininess)** map, each with its own texture picker + repeats /
  offset / rotation, plus glossiness, environment intensity, specular colour,
  and the diffuse **alpha mode** / mask cutoff. These live in the attached
  `LLMaterial` (not the `TextureEntry`), sent via `RenderMaterials` — the
  `material_id` on the face's `TextureFace`.
- **PBR (GLTF `LLGLTFMaterial`)**: base-colour, metallic-roughness, emissive,
  and normal textures with per-channel transforms, applied through the GLTF
  render-material path (`ObjectExtraParams` render material / the material cap).

Wire each map channel to its own texture swatch (the reusable
[[viewer-ui-texture-picker]]) and colour swatch, and add the matmedia + map-type
selectors (the reusable [[viewer-ui-combo-widget]] / radio).

**Note (likely bug link):** the display channel and the edited channel must
match. A face that renders from a legacy `LLMaterial` diffuse or a GLTF
render-material is **not** shown from the `TextureEntry` `texture_id`; editing
`texture_id` (or live-previewing it on the `StandardMaterial`'s
`base_color_texture`) on such a face has no visible effect — or is overwritten
by `apply_legacy_materials` / `apply_pbr_textures` each frame. This task must
route an edit to the channel the face actually displays from (or detect and
clear the overriding material), which likely resolves the "no live preview /
solid on commit" symptoms seen on material'd faces.

Reference (Firestorm, read-only): `llpanelface.cpp` (matmedia / material-type /
the `LLMaterial` + GLTF setters), `llmaterialmgr`, `llgltfmaterial`.
