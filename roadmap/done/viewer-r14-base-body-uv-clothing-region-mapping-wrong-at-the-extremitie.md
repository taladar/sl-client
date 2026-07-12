---
id: viewer-r14
title: Base-body UV / clothing region mapping wrong at the extremities
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Known rendering issues (to fix)
---

Context: [context/viewer.md](../context/viewer.md).

**R14. Base-body UV / clothing region mapping wrong at the extremities**
(`sl-client-bevy` / `sl-bake`). Against a Firestorm side-by-side the baked
clothing (the blue upper / red lower body layers) covered the **hands and
feet**, which Firestorm leaves as bare skin, and there was a visible **gap /
seam** in the coverage. **Localised** (offline screenshot vs the user's
Firestorm shot): neither the base-mesh UVs nor the composite bounds — the
fault was the **missing garment-shape masking**. A clothing layer's
`local_texture` (the shirt / pants fabric) covers the *whole* body-region UV,
including the hand and foot texels; the reference viewer bounds each garment
layer to its garment extent by a stack of `avatar_lad.xml` `<param_alpha>`
masks — sleeve length, shirt bottom, collar, pants length / waist, glove /
sock / shoe / jacket bounds — driven by the wearable's shape params
(`LLTexLayerParamAlpha` / `LLImageTGA::decodeAndProcess`). Our compositor
blended each garment fabric across the whole region, so a solid-fabric
shirt/pants painted the bare hands and feet. **Fix:** modelled the masks in
`sl-bake` — a `ShapeMaskSpec` on each garment `PlannedLayer` (the static
alpha-TGA, the driving param id, `multiply_blend`, `domain`), resolved by
`region_layers` into compositor `ShapeMask`s (static TGA via the runtime's
`static_image` closure + a new `mask_weight` closure); `composite_region` now
multiplies each `LayerKind::Blend` texel's alpha by the combined mask
coverage, reproducing the reference's per-`param_alpha` LUT (domain ramp /
hard threshold) and additive-then-multiplicative accumulation
(`renderMorphMasks`). The runtime preloads the mask TGAs (`shape_mask_files`)
alongside the existing static layers. **The shape params are *driven*, not
stored:** a garment stores only its group-0 driver (Sleeve Length 800, Pants
Length 815, …), which drives the group-1 mask params (600 / 615 / …), so
`mask_weight` runs the wearable's stored params through
`ResolvedParams::from_values` (P13.4's driver→driven propagation, fed by a new
`AppearanceValues::from_weights`) and reads the resolved driven weight — using
the raw stored value instead left the sleeves/legs at the wrong length.
Confirmed live (own avatar, local OpenSim, offline screenshot): hands and feet
are now bare skin, the shirt sleeves are bounded, the pants end at the ankles,
and the upper/lower waist seam is clean — matching the Firestorm ground truth.
Surfaced by the R13 Firestorm side-by-side.
