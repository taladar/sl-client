---
id: viewer-custom-face-material-shader
title: Custom face material shader — PBR per-map transforms + legacy Blinn-Phong specular
topic: viewer
status: in-progress
origin: user request (2026-07-25) — full material fidelity after the FIRE-35138 work
refs: [viewer-pbr-blinn-phong-build-preview, viewer-face-materials-pbr, viewer-legacy-material-exact-port, viewer-tonemap-auto-exposure, viewer-bevy-material-inplace-reprepare, viewer-perf-texture-anim-pause, viewer-perf-prim-tessellation-cache]
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

- **Phase 0** ✅ — inert retype (no visual change): introduce the type, register
  the plugin once, mechanically retype the whole face pipeline (textures /
  materials / legacy_materials / bump / texture_anim / objects / avatars /
  media_prim / edit_* / hud + test harnesses). Verify screenshots unchanged.
- **Phase 1** ✅ — PBR per-map UV transforms.
- **Phase 1.5** ✅ — **GPU-side texture animation** (perf, see below).
- **Phase 2** — legacy Blinn-Phong specular + BP preview + non-PBR faces (fetch
  legacy materials for PBR faces too; specular map into the extension slot).
- **Phase 3** — revert-to-Blinn-Phong on
  `RemovedComponents<ObjectRenderMaterials>`.
- **Phase 4** — tonemapper preferences (`RenderTonemapType`/`Mix`/`Exposure`).

## Performance (Phase 1.5)

Going bindless kept draw-call batching, but a **per-frame material mutation** is
much costlier for an `ExtendedMaterial` than a bare `StandardMaterial`: Bevy has
no in-place update path — it frees and **fully recreates** a material's bind
group on *any* change (`bevy_pbr` `material.rs`, an explicit TODO), and the
extension carries the base's whole binding set plus its own. The dominant source
was `drive_texture_animations` writing every animated face's `uv_transform`
**every frame** — a busy region has ~1200 animated faces, so ~1200 full
re-prepares/frame (~90 ms, profiled via a since-removed `SL_VIEWER_DIAG`
per-system dirty attribution).

Fix (Phase 1.5, done): **texture animation now runs on the GPU**. The
`SlFaceParams` `anim_*` fields carry the `llSetTextureAnim` params + the face's
static fall-back placement + a `start_time`; the shader's `sl_animated_uv` (a
port of the Rust `animate`, kept test-gated as the reference) evaluates the
animation from `globals.time` per fragment and re-samples base colour at the
animated UV. The CPU driver writes the params **once** (on start / change), so a
running animation dirties **no** material per frame. Result:
`drive_texture_animations` dirtying 1217→~2/frame; at full-load standstill
material dirtying is ~0/frame. Known limit: `globals.time` wraps hourly, so a
**non-looping** animation held past an hour replays once per wrap (looping/most
content unaffected).

Residual, out of scope here (steady-state FPS still below the pre-migration
baseline with periodic dips): material dirtying is ~0 at rest, so this is
**not** the material re-prepare — it is **pre-existing object-rebuild churn**
(mesh tessellation + face spawning as the region streams/re-tessellates in
waves, a main-world cost), tracked by [[viewer-perf-prim-tessellation-cache]].
The material cost of *any* churn (streaming, media, LoD) is what the systemic
Bevy in-place-reprepare fast-path would remove —
[[viewer-bevy-material-inplace-reprepare]].

## Approximations (honest)

The legacy specular is an **analytic** normalized Blinn-Phong lobe, not SL's
`lightFunc` LUT; environment intensity scales an ambient specular (no reflection
probe in the headless path); highlight *shape* and reflected *content* differ
from Firestorm. The pixel-closer exact port is tracked in
[[viewer-legacy-material-exact-port]].

Full design: `~/.claude-personal/plans/greedy-booping-storm.md`.
