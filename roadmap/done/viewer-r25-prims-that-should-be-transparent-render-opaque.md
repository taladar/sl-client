---
id: viewer-r25
title: Prims that should be transparent render opaque
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Known rendering issues (to fix)
---

Context: [context/viewer.md](../context/viewer.md).

**R25. Prims that should be transparent render opaque.** On aditi, some
plain prims that are transparent in Firestorm render fully opaque in the
viewer. Picked two on a live region — the **Mauve sign** and the **fence
around King Kong** — both plain **box** prims (`asset=None`,
`path_curve=16`/`profile_curve=1`), large and flat (`scale≈10×0.26×8` and
`0.24×9.77×2.92`), so this is a **prim**-face transparency path, not a
mesh/sculpt or an avatar bake.

**Root cause (source archaeology, 2026-07-22).** The tint path itself is
correct at build time: `face_material` sets
`alpha_mode: face_alpha_mode(face.color)`
(`sl-client-bevy-viewer/src/textures.rs:736`), and `face_alpha_mode`
(`textures.rs:897`) returns `AlphaMode::Blend` whenever the TE tint alpha
`color[3] < 255`. The R22d texture-alpha resolution
(`apply_prim_textures`, `textures.rs:830`) only *upgrades* an `Opaque`
face and never downgrades a tint-derived `Blend`, and fullbright only
sets `unlit` (`bump.rs:177`). The defect is the **legacy-material apply
step clobbering the tint-derived Blend**: when a face's `LLMaterial`
arrives over the `RenderMaterials` cap, `apply_legacy_scalars`
(`legacy_materials.rs:304`) **unconditionally** overwrites
`standard.alpha_mode` with `legacy_alpha_override`
(`legacy_materials.rs:175`), which maps diffuse alpha mode `NONE` (0, the
default — also what `byte_field` in `sl-wire/src/material/legacy.rs:117`
yields for a missing field) and `EMISSIVE` (3) to `AlphaMode::Opaque`. So
any translucent-tinted face that also carries a non-nil `material_id`
whose material's alpha mode is `NONE` is forced opaque. The reference
viewer has the **opposite precedence**
(`phoenix-firestorm/indra/newview/llvovolume.cpp:6927` and `:6995`): for
every non-GLTF face it computes
`blinn_phong_transparent = te->getColor().mV[3] < 0.999f`, ORs it into
`is_alpha`, and registers `PASS_ALPHA` *before* the material-pass
dispatch — a translucent TE tint wins over the LLMaterial's
`DiffuseAlphaMode`; the material mode only matters when the tint is
opaque. This is exactly the common content pattern of tinted transparent
prims that also carry shiny/bump legacy materials (setting specular or
normal maps in the build tool creates an `LLMaterial` with alpha mode
"None"). The PBR/GLTF path (`materials.rs:587`) overwrites the alpha mode
too, but that *matches* the reference ("all other parameters ignored if
gltf material is present"), so it is not a divergence.

**Fix direction.** Apply `legacy_alpha_override` only when the face tint
is opaque: on the legacy path `StandardMaterial.base_color` still holds
the TE tint (`textures.rs:720`; only the PBR path replaces it), so
`apply_legacy_scalars` can keep `AlphaMode::Blend` when the base-colour
alpha is < ~0.999, mirroring `blinn_phong_transparent`. Note the
reference sends a translucent-tinted face to the alpha pass for *all*
material modes (including fullbright + `MASK`), so the guard belongs
around the whole override, not just the `NONE`/`EMISSIVE` arms.

**Runtime confirmation (do while fixing).** Pick both repro prims with
`P` and confirm `color[3] < 255` plus a non-nil `material_id`
(`objects.rs:934` dumps both, but not the fetched `diffuse_alpha_mode`
or the resolved `alpha_mode` — worth extending the dump). Fallback
hypothesis if `color[3] == 255`: the transparency would instead come
from a `BLEND`-mode legacy material that was never fetched/applied
(`RenderMaterials` cap fetch failure) — check the fetch logs in that
case.

**Fixed (2026-07-23).** `apply_legacy_scalars`
(`sl-client-bevy-viewer/src/legacy_materials.rs`) now applies
`legacy_alpha_override` only when the face's TE tint is opaque
(`base_color.alpha() >= 0.999`, the reference's `blinn_phong_transparent`
threshold) — a translucent tint keeps its `AlphaMode::Blend` for every
material mode, matching the reference's pre-dispatch `is_alpha` OR. The
guard wraps the whole override per the note above. Unit-tested (translucent
tint survives `NONE` and `EMISSIVE`; opaque tint still honours the material
in both directions). The pick dump (`P`) now also prints the face's
resolved `alpha_mode` + base-colour alpha and the fetched legacy material's
`diffuse_alpha_mode` / `alpha_mask_cutoff` / map ids — or a "not
fetched/decoded" marker — so the runtime confirmation on the two aditi
repro prims can be read straight off the pick output.
