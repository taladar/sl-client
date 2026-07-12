---
id: viewer-p14-5
title: Clothing-morph alpha masks
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 14 — Server-published baked texturing (incl. alpha)
---

Context: [context/viewer.md](../context/viewer.md).

**P14.5. Clothing-morph alpha masks.** The second half of the original
P13.5, split out here because it needs the baked-texture alpha pipeline built
in P14.1–P14.3. Firestorm `LLPolyMorphTarget::applyMask` /
`mIsClothingMorph`: the flared sleeve / pant-leg / long-cuff / loose-body
geometry is driven by `clothing_morph="true"` params (`Shirtsleeve_flair`,
`Leg_Pantflair`, `Leg_Longcuffs`, `Displace_Loose_Upper/Lowerbody`, the
`skirt_*` morphs) whose `<mask layer="upper_clothes/lower_pants/skirt">`
associates them with a clothing layer. In the reference viewer the per-vertex
`maskWeight` fed into the morph (and the resulting clothing alpha) comes from
the **baked texture's alpha channel** (`onBakedTextureMasksLoaded` sampling
the baked upper/lower/skirt image) — so it can only land once the baked
textures
are fetched and decoded (P14). Apply that per-vertex clothing alpha through
the base-mesh shared-vertex remap table (`SharedVertex`, already decoded) and
render those vertices with `AlphaMode::Blend` / `Mask`, so an un-clothed body
shows no stray flared cuffs.

**Done — realised as a per-vertex *geometry* mask, not an alpha render.** The
reference mechanism (`LLPolyVertexMask::generateMask` +
`LLPolyMorphTarget::applyMask`) does not draw the clothing morph with a
transparent alpha; it scales each clothing morph's per-vertex position/normal
delta by the baked-region alpha sampled at that vertex's UV, so the flare
geometry itself vanishes where there is no fabric — that is what "no stray
flared cuffs" needs, and what shipped. The `<mask layer="skirt">` case from
the roadmap text does not exist in `avatar_lad.xml` (its `<morph_masks>` table
has seven entries, all `head` / `upper_body` / `lower_body`), so no skirt
morph is masked. **Library (`sl-avatar`):** a new `masks` module —
`MorphMasks::from_xml` parses the `<morph_masks>` table (`morph_name` /
`body_region` / `layer` / `invert`); `MaskTexture` samples a decoded bake's
alpha (nearest + clamp, last-component, mirroring `generateMask`);
`MorphMasks::sample_part` walks a base part's masked morphs, sampling each
delta vertex's UV through the shared-vertex remap into a `PartMorphMask` of
per-delta weights; and `MorphWeights::apply_masked` (a thin variant of
`apply`) scales each masked delta by `weight * maskWeight`. All re-exported
through `sl-client-bevy`. **Viewer:** `AvatarAssetLibrary` also parses
`MorphMasks` from the one `avatar_lad.xml` read;
`BodyRegion::morph_mask_region` maps the head / upper / lower regions to their
`<morph_masks>` names; `apply_avatar_appearance` now masks each masked part's
morphs by its region's decoded bake (`part_clothing_mask`) and re-shapes the
body when a masked-region bake decodes (a second `TextureDecoded` reader
re-dirties the wearing avatar) — so the morphs apply at full flare until the
bake arrives, then snap to the masked shape, exactly as the reference viewer
does before/after `onBakedTextureMasksLoaded`. Unit-tested end-to-end (mask
parse, nearest-sample, `sample_part` full/zero-alpha, `apply_masked`
per-vertex scaling, region↔slot mapping). Like P14.3/P14.4 the trigger (a
decoded clothing bake carrying a coverage-alpha channel) is outfit-driven and
cannot be forced deterministically, so the unit-tested Firestorm-faithful path
is the guarantee; it is exercised live only near an avatar wearing flared
system-layer clothing.
