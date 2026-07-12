---
id: viewer-p27-3
title: Legacy materials (normal / specular)
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 27 — PBR & legacy materials
---

Context: [context/viewer.md](../context/viewer.md).

**P27.3. Legacy materials (normal / specular).** Support the pre-PBR
`LLMaterial` (RenderMaterials): normal map + specular map +
environment / glossiness + alpha mode, mapped onto `StandardMaterial` normal
/ metallic where possible. Reference: `LLMaterialMgr` / `lldrawpoolmaterials`.
**Done:** the whole wire/proto/runtime half already existed (the `sl-wire`
`LegacyMaterial` / `RenderMaterialEntry` codec over the zipped binary-LLSD
`RenderMaterials` capability, `sl-proto`'s `Event::RenderMaterials`, the
`RequestRenderMaterials` command in both runtimes) — net-new was purely the
viewer application layer, a new `legacy_materials.rs` module mirroring the
P27.1 PBR pipeline but driven by the capability's **batch** fetch rather than
a per-asset `ViewerAsset` fetch. A face references a legacy material by the
16-byte `material_id` in its `TextureEntry` face (already carried on each face
entity as `FaceTextureDebug`); `register_legacy_materials` picks up each
newly-spawned face carrying one — skipping any face that also has a PBR GLTF
material, which supersedes it as in the reference — and queues the id.
`drive_legacy_material_requests` batches the outstanding ids into
`RequestRenderMaterials` commands (chunked to the reference's
50-per-transaction limit), `receive_legacy_materials` folds the decoded reply
into a cache, and `apply_legacy_materials` writes each material onto the
waiting faces + requests its normal map through the shared `TextureManager`
(`apply_legacy_normal_maps` uploads the map linear into the normal slot). The
`StandardMaterial` mapping is faithful for the **normal map**; the specular /
environment / glossiness stack is folded into the scalar `reflectance` (from
environment intensity) and `perceptual_roughness` (from the specular
exponent / glossiness), and the diffuse alpha mode maps `NONE`→opaque and
`MASK`→alpha-test (leaving `BLEND` / `EMISSIVE` to the diffuse-derived mode
so a legacy material never forces an opaque face into the z-sorted transparent
path). Documented approximations (Bevy's `StandardMaterial` cannot express
them): the specular **map texture** and the per-map (normal / specular) UV
transforms are dropped, and the specular colour tint is not applied. Scalar
conversions unit-tested.
**Live-confirmed on aditi** (like P27.2): the landing region drove a clean
round-trip of **63 legacy materials requested = 63 received** over the
`RenderMaterials` cap (which — unlike the `ViewerAsset` cap that left the
asset / bake cases aditi-partial — works on aditi) with the scene rendering
intact. OpenSim's Default Region carries no legacy-material faces, so no
on-screen confirmation there (the pipeline runs clean).
