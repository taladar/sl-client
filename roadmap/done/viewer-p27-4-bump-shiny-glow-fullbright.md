---
id: viewer-p27-4
title: Bump / shiny / glow / fullbright
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 27 — PBR & legacy materials
---

Context: [context/viewer.md](../context/viewer.md).

**P27.4. Bump / shiny / glow / fullbright.** The legacy per-face
bump / shiny / fullbright / glow flags → Bevy emissive / normal / metallic
approximations. Reference: `lldrawpoolbump` / `LLFace::getGeometryVolume` /
the `SHININESS_TO_ALPHA` shiny packing. **Done:** a new `bump.rs` module maps
the four legacy surface effects a `TextureEntry` face carries (in its
`bump_shiny_fullbright` byte plus the separate `glow` scalar — the pre-PBR
per-face controls, distinct from the P27.1 GLTF and P27.3 `LLMaterial`
materials). The scalar three fold onto each face's `StandardMaterial` as it is
built, by `apply_surface_flags` called from `face_material`, so they cover
prims, sculpts, meshes, and rigged attachments uniformly: **fullbright** →
`unlit` (exact); **glow** (0..1) → an additive `emissive` tinted by the face
colour (the viewer has no bloom pass, so a glowing face simply reads brighter,
and the glow is uniform rather than texture-following — a documented
approximation); **shiny** (none / low / medium / high) → an *analytic-light*
highlight, not a cube-map reflection, since the viewer has no reflection
probe (a metallic surface would read black) — `reflectance` is raised and
`perceptual_roughness` lowered with the level (driven by the reference's
`SHININESS_TO_ALPHA = [0, .25, .5, .75]` environment-intensity table), leaving
metallic at zero so the sun/moon directional light throws a progressively
sharper, brighter specular. **Bump** needs the decoded diffuse, so it runs as
a small fetch/generate pipeline like the P27.3 normal path: a `BumpManager`
resource, `register_bump_faces` (parks each newly-spawned bumped face on its
diffuse texture id, skipping a face with no diffuse, a legacy `LLMaterial` id
— P27.3 supplies its normal — or a PBR GLTF material, which supersedes the
legacy flags as in the reference), and `apply_bump_normals` (once the diffuse
decodes, generates a tangent-space **normal map** from its luminance as a
height field — Sobel central differences, wrapping to match the repeating face
sampler — and drops it into `normal_map_texture`). The normal's **source**
matches the reference: the brightness / darkness codes derive it from the
face's own diffuse (darkness inverts the height field), while the 15 standard
emboss codes (≥ 3 — woodgrain, bark, bricks, …) fetch their fixed Linden bump
texture (the reference viewer's `std_bump.ini` UUID table) through the shared
texture manager and derive the normal from that. Runs after the legacy
material path so a real `LLMaterial` normal wins. Scalar mappings + normal
encoding + the standard-code lookup unit-tested. **Live-confirmed on aditi**
(like P27.2 / P27.3): the landing region drove real bump content — dozens of
faces across many textures generated normal maps cleanly (6 / 8 / 16 / 116 …
faces per texture), including the real standard emboss textures (woodgrain,
gravel, siding fetched by UUID), with the scene rendering intact.
OpenSim's Default Region carries no bump/shiny faces, so no on-screen
confirmation there (the pipeline runs clean).
