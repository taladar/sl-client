---
id: viewer-p27-2
title: GLTF material overrides
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 27 — PBR & legacy materials
---

Context: [context/viewer.md](../context/viewer.md).

**P27.2. GLTF material overrides.** Apply per-face
`GltfMaterialOverride` deltas delivered via the override cap / ObjectUpdate
extended data, layered on the base material. Reference:
`LLGLTFMaterialList::applyOverride`. **Done:** the simulator pushes per-face
overrides in a GLTF material-override `GenericStreamingMessage` (method
`0x4175`), already surfaced by `sl-proto` as
`Event::GltfMaterialOverride { local_id, faces, overrides }` with each face's
override document left as raw notation-LLSD bytes. Net-new decoding: a new
**`parse_llsd_notation`** in `sl-llsd` (the textual counterpart of the binary
parser — every LLSD kind, mirroring Firestorm's `LLSDNotationParser`), and in
`sl-material` a **`MaterialOverride`** sparse-delta type with
`parse_material_override` (decodes one `od[i]` notation map — the shaved
`tex`/`bc`/`ec`/`mf`/`rf`/`am`/`ac`/`ds`/`ti` keys) and `apply_to` (folds the
delta onto a base `GltfMaterial`, mirroring `applyOverrideLLSD` +
`applyOverride`: the `GLTF_OVERRIDE_NULL_UUID` sentinel clears a texture slot,
a present factor replaces the base's, per-slot transforms fold on). Both
re-exported from the two runtimes. In the viewer, `materials.rs` gained a
scoped-object + face-index key on each registered PBR face
(`ObjectRenderMaterials` now carries the object's `scoped_id`) and a
**recompose** model: the base material and any stored override are re-applied
together whenever either changes (base decode, or a new
`apply_material_overrides` system that decodes + stores/clears the per-face
overrides and reverts faces the message omits). The face's diffuse
`uv_transform` is captured at registration so recomposition never
double-composes the base-colour `KHR_texture_transform`. Decoders unit-tested
(`sl-llsd`, `sl-material`). **Live-confirmed on aditi** (unlike P27.1): the
landing region pushed real overrides (two objects, 4 + 1 faces) that flowed
through the pipeline cleanly — though the base maps could not be shown because
aditi's `ViewerAsset` service 503s (the same flakiness that left the asset /
bake cases aditi-partial). OpenSim's Default Region carries no PBR/override
content, so no on-screen confirmation there.
