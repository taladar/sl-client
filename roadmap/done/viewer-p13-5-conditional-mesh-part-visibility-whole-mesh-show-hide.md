---
id: viewer-p13-5
title: Conditional mesh-part visibility (whole-mesh show/hide)
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 13 — Base avatar in the viewer (replace spheres)
---

Context: [context/viewer.md](../context/viewer.md).

**P13.5. Conditional mesh-part visibility (whole-mesh show/hide).** The
Firestorm `updateMeshVisibility` / `renderTransparent` mechanism, showing or
hiding whole base-avatar mesh regions from what is worn so the body renders
only the right parts. **Scope split:** narrowed at implementation to part
**(a)**; part **(b)** clothing-morph alpha masks moved to **P14.5** because it
genuinely needs the Phase-14 baked-texture alpha pipeline (Firestorm's
per-vertex `maskWeight` comes from the baked texture's alpha channel via
`onBakedTextureMasksLoaded`, not from geometry alone). **Done:** render the
skirt part (`avatar_skirt.llm`) only when a skirt is worn — the reference test
`isWearingWearableType(WT_SKIRT) && isTextureVisible(TEX_SKIRT_BAKED)`, which
for another avatar reduces to the `TEX_SKIRT_BAKED` slot holding a real,
non-`IMG_INVISIBLE` bake — and hide a whole base region (head / hair / eyes /
upper / lower / skirt) when a worn attachment face carries the matching
`IMG_USE_BAKED_*` magic UUID (a mesh body/clothing replacing that region); the
default (no skirt, no mesh body) hides the skirt and shows every other region.
Net-new library surface was in `sl-proto`'s `avatar_texture` module (already
re-exported wholesale by both runtimes, so no per-runtime export churn): the
`IMG_DEFAULT_AVATAR` / `IMG_INVISIBLE` / eleven `IMG_USE_BAKED_*`
magic-texture UUID constants, an `is_bake_visible(TextureKey)` predicate (the
`isTextureVisible` baked-slot test), and `use_baked_slot(TextureKey) ->
Option<usize>` (a sentinel → baked slot mapping); `MAX_FACES` gained a
re-export from both runtimes. In the viewer, each base part now carries a
`BodyRegion` (`avatar_assets.rs`, keyed to its baked slot — eyelashes ride
with the head, eyeballs with the eyes, matching the reference viewer),
threaded onto the `AvatarBodyPart` marker. `AvatarState` gained per-agent
skirt visibility
(computed from each `AvatarAppearance`'s `TEX_SKIRT_BAKED` slot) plus
lightweight attachment bookkeeping — a parent-scoped map and a once-scanned
per-object `IMG_USE_BAKED_*` slot set for every non-root object — and a new
`apply_avatar_part_visibility` system that each frame chases each
`IMG_USE_BAKED`-bearing attachment up its linkset chain to its avatar root and
sets each part's `Visibility` (only when it actually changed). The skirt
spawns `Hidden` so an un-worn skirt never flashes. Verified live on OpenSim:
our own
skirt-less avatar logs `skirt not worn` and the base skirt mesh is hidden on
screen (user-confirmed), the body still shaping (`shaped 8 body part(s) + 133
joint(s)`) with no skinning / wgpu errors. The `IMG_USE_BAKED_*` region-hide
cannot fire on a plain OpenSim avatar (no mesh body), so it is covered by unit
tests (chain-attribution + sentinel scan) and Firestorm parity; it exercises
live only near a mesh-body avatar (aditi / SL).
