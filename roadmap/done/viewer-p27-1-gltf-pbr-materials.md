---
id: viewer-p27-1
title: GLTF PBR materials
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 27 — PBR & legacy materials
---

Context: [context/viewer.md](../context/viewer.md).

Faces use a diffuse-only `StandardMaterial` today. This phase adds the modern
GLTF PBR materials and the pre-PBR legacy material stack, both of which Bevy's
`StandardMaterial` can largely express.

**P27.1. GLTF PBR materials.** Fetch `LLFetchedGLTFMaterial` assets and
map to Bevy `StandardMaterial` (base colour, metallic-roughness, normal,
emissive, occlusion, alpha mode / cutoff, double-sided), with each map
supplied by the texture pipeline. Reference: `LLGLTFMaterial`. **Done:** a new
pure crate **`sl-material`** decodes the `AT_MATERIAL` asset (an LLSD envelope
`{ version, type, data }` wrapping a glTF 2.0 document) into a
renderer-agnostic `GltfMaterial` — base-colour / metallic / roughness /
emissive factors, four texture slots with `KHR_texture_transform`, alpha
mode + cutoff, double-sided — re-exported from both runtimes. A new viewer
module `materials.rs` owns a `MaterialManager` (its own `AssetStore` over the
`ViewerAsset` cap, mirroring the animation / wearable pipelines): a face's
base PBR material id comes from the object's `render_material` extra-params
(`LLRenderMaterialParams`), attached to the geometry-holder entity as
`ObjectRenderMaterials` so `register_pbr_materials` joins each spawned face to
it; the manager fetches + decodes the asset, patches the face
`StandardMaterial`'s scalar fields, and requests each map through the shared
`TextureManager` (base colour / emissive uploaded sRGB, normal /
metallic-roughness linear; the ORM map drives both the metallic-roughness and
occlusion slots). Bevy carries a single `uv_transform`, so the base-colour
`KHR_texture_transform` composes onto the face's texture-entry placement and
stands in for every slot (an approximation of the reference's per-slot
transforms). Decoder unit-tested (`cargo test -p sl-material`). Live check:
the pipeline runs clean on both OpenSim and aditi with no regression, but
neither reachable login point had a GLTF-PBR-material prim in view, so an
on-screen PBR render is not yet confirmed against real content (OpenSim's
Default Region carries none; the aditi landing region showed none). Per-face
**overrides** are P27.2; **terrain** PBR (the R15 single-colour-terrain
suspect) is a separate path, not this prim/mesh-face material.
