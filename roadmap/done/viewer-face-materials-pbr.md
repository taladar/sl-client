---
id: viewer-face-materials-pbr
title: Texture tab — Blinn-Phong normal/specular maps + PBR (GLTF) materials
topic: viewer
status: done
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

## Done (2026-07-24)

Delivered the whole Texture-tab material surface, and — per the user's "cover
everything reachable from the build menu" — the PBR material *editor* controls
too (which the original text delegated to [[viewer-pbr-material-editor]]; see
that task for what still remains).

**Selectors + visibility** (`edit_texture.rs`): the `matmedia` combo
(Materials / PBR), the material-type radio (Texture / Bumpiness / Shininess) and
the pbr-type radio (Material / Base / Metallic / Emissive / Normal), plus a
`MatModeState` + `ShowWhen` per-control visibility system mirroring
`LLPanelFace::updateVisibility`. The mode auto-selects PBR for a face carrying a
render material, else Materials (`auto_select_material_mode`) — the reference's
per-object behaviour, which resolves the "which texture is shown while editing"
note above (a PBR face edits PBR; a legacy/plain face still renders from
`TextureEntry.texture_id`, so its diffuse edit shows).

**Blinn-Phong** (`edit_material.rs`): normal + specular map swatches, their
repeats/offset/rotation, glossiness, environment intensity, specular colour, and
the diffuse alpha mode / mask cutoff — all editing the face's *legacy material*
and committed over a **new `RenderMaterials` PUT** path built for this:
`sl-wire::build_render_materials_put_request` +
`sl_proto::Command::SetRenderMaterials` (`FaceMaterialPut`) + both runtimes'
HTTP PUT. The sim assigns the material id and echoes it (the reference's
`LLMaterialMgr::put`).

**PBR / GLTF** (`edit_material.rs`): per-channel texture pickers
(base/metallic/emissive/normal), base + emissive tint swatches, metallic /
roughness factors, alpha mode + cutoff + double-sided, per-channel transforms,
**New** (assign `BLANK_MATERIAL_ASSET_ID`) and **Save** (encode the effective
material as an `AT_MATERIAL` LLSD envelope, upload via
`Command::UploadAsset`/`AssetType::Material`). Every per-channel edit is a face
**GLTF override**: `sl-material::encode_override_gltf_json` builds the
`ModifyMaterialParams` `gltf_json` (and `encode_material_asset` the
full-material asset for Save). To make edits visible immediately (not only after
the sim's
echo — the fix for the "nothing happens on OK" symptom), the override is applied
**locally** at once via `MaterialManager::apply_local_override` +
`drive_local_overrides` (recomposes the face); the echo re-applies idempotently.

**Facts not in git worth carrying forward:**

- **PBR is only exercisable on aditi.** OpenSim serves no PBR content and (per
  the run) no working `ModifyMaterialParams`, so the PBR controls show but their
  sends are no-ops there; the Blinn-Phong `RenderMaterials` PUT does round-trip
  on OpenSim's `MaterialsModule`. Live-verified on aditi: per-channel edits
  round-trip and (with the local-apply) update the swatch + prim immediately.
- **Correction (2026-07-25):** an earlier claim of an aditi "`ViewerAsset` 503
  wall" (PBR base *maps* staying grey) was wrong — the cap works; the maps not
  showing on a prim was the client's assign-to-existing-prim registration gap,
  fixed in [[viewer-pbr-blinn-phong-build-preview]]. The swatch UUID / factor /
  tint / transform values reflect regardless. (One aditi login also came up in a
  transient "Connecting…"/no-objects stall unrelated to this code — a fresh
  login fixed it.)
- **The `RenderMaterials` PUT body** is `{Zipped:{FullMaterialsPerFace:[{Face,
  ID(region-local),Material}]}}`; no client-side material-id hashing (the sim
  assigns it). The PBR override JSON writes a full GLTF material where only the
  overridden fields differ from default (the sim applies non-defaults), with a
  nil-texture image allocated for a transform-only slot (the reference's
  `writeToTexture`).
- **Known follow-ups** (filed): the render-material swatch previews by albedo,
  not the reference's material-on-a-sphere render
  ([[viewer-material-swatch-sphere-preview]]); material inventory items do not
  show at all, which also blocks a proper *material* picker (title + filter) for
  the render-material swatch ([[viewer-inventory-materials-not-shown]]); Save
  does not auto-assign the new asset to the face afterwards.
