---
id: viewer-custom-face-material-shader
title: Custom face material shader — PBR per-map transforms + legacy Blinn-Phong specular
topic: viewer
status: in-progress
origin: user request (2026-07-25) — full material fidelity after the FIRE-35138 work
refs: [viewer-pbr-blinn-phong-build-preview, viewer-face-materials-pbr, viewer-legacy-material-exact-port, viewer-tonemap-auto-exposure]
---

Context: [context/viewer.md](../context/viewer.md).

Replace Bevy's `StandardMaterial` for SL prim/mesh faces with a custom
`ExtendedMaterial<StandardMaterial, SlFaceExt>` (`type FaceMaterial`) so faces
render **all** set material fields faithfully — which `StandardMaterial` cannot,
because it has one shared UV transform for all maps and no Blinn-Phong specular
workflow.

Goals:

- **PBR**: per-map UV transforms (base-colour / normal / metallic-roughness /
  emissive each with their own `KHR_texture_transform`) plus every set factor.
- **Legacy Blinn-Phong**: the specular map + specular colour + glossiness
  (exponent) + environment intensity + normal map, each with its own per-map
  transform — for non-PBR faces **and** in the FIRE-35138 Blinn-Phong build-tool
  preview.
- **Revert to Blinn-Phong** when a PBR render material is cleared in-world.
- **Tonemapper** matched to the reference (already ported in `tonemap.rs`;
  remaining work is exposing `RenderTonemapType` / `RenderTonemapMix` /
  `RenderExposure` as preferences).

## Architecture

One unified `FaceMaterial = ExtendedMaterial<StandardMaterial, SlFaceExt>` for
every prim / mesh / rigged / avatar-BoM / media face (the extension is **inert**
where unused, so the face keeps its one stable handle and every in-place mutator
just gains a `.base.` hop). `pbr_input_from_standard_material` samples all maps
with one UV, so the extension **re-samples** the base's normal/MR/emissive
textures at per-map UVs and overwrites the `PbrInput`; the legacy specular map
(no `StandardMaterial` slot) moves into an extension texture binding. Legacy
adds an analytic normalized Blinn-Phong lobe over a matte base.

New: `sl-client-bevy-viewer/src/face_material.rs` + `face_material.wgsl`
(template `sl-client-bevy/src/water.rs`).

## Phases

- **Phase 0** — inert retype (no visual change): introduce the type, register
  the plugin once, mechanically retype the whole face pipeline (textures /
  materials / legacy_materials / bump / texture_anim / objects / avatars /
  media_prim / edit_* / hud + test harnesses). Verify screenshots unchanged.
- **Phase 1** — PBR per-map UV transforms.
- **Phase 2** — legacy Blinn-Phong specular + BP preview + non-PBR faces (fetch
  legacy materials for PBR faces too; specular map into the extension slot).
- **Phase 3** — revert-to-Blinn-Phong on
  `RemovedComponents<ObjectRenderMaterials>`.
- **Phase 4** — tonemapper preferences (`RenderTonemapType`/`Mix`/`Exposure`).

## Approximations (honest)

The legacy specular is an **analytic** normalized Blinn-Phong lobe, not SL's
`lightFunc` LUT; environment intensity scales an ambient specular (no reflection
probe in the headless path); highlight *shape* and reflected *content* differ
from Firestorm. The pixel-closer exact port is tracked in
[[viewer-legacy-material-exact-port]].

Full design: `~/.claude-personal/plans/greedy-booping-storm.md`.
