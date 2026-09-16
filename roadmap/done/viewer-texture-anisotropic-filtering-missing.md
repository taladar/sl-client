---
id: viewer-texture-anisotropic-filtering-missing
title: A texture's level-of-detail change never reached the faces sampling it (seen as soft checker edges, blamed on anisotropy)
topic: viewer
status: done
origin: seen while verifying viewer-sculpt-sphere-fixture-divergence (2026-09-16)
refs:
  - viewer-sculpt-sphere-fixture-divergence
  - viewer-antialiasing-sharpen-aniso
  - viewer-texture-mip-chain-missing
---

Context: [context/viewer.md](../context/viewer.md).

In `sl-crosscheck --scenario catalogue --look-at pbr-box --look-from 3
--look-above 4`, the `sculpt-sphere`'s checker edges are visibly **soft** here
and **crisp** in Firestorm, although both viewers now build the same geometry
with the same texture coordinates. The boxes in the same frame, seen nearly
face-on, are equally sharp in both.

The sphere is where the two texture axes disagree: V runs pole to pole over
half the circumference, U once around, so a texel is about twice as long in
one screen direction as in the other. Isotropic trilinear filtering picks the
mip level for the longer one and blurs the other; anisotropic filtering does
not.

The reference filters anisotropically by default: `RenderAnisotropic` defaults
to `true` in `settings.xml` and sets `LLImageGL::sGlobalUseAnisotropic`
(`llappviewer.cpp`), with a settings listener to change it live. This viewer
sets no `anisotropy_clamp` on any sampler: `materials.rs`,
`legacy_materials.rs` and `bump.rs` all build theirs from
`ImageSamplerDescriptor::linear()`.

## To find out

- Whether anisotropy alone closes the gap on the sphere (set a clamp on the
  face samplers and re-run the cross-check above).
- The anisotropy level the reference ends up with
  (`TFO_ANISOTROPIC` → `GL_TEXTURE_MAX_ANISOTROPY` from the driver's maximum?)
  and whether wgpu's `anisotropy_clamp` constraints (all filters linear) hold
  for every face sampler.
- Wiring `RenderAnisotropic` as a preference, as the reference exposes it.

## Answer

**Not anisotropy.** Three measurements settled it:

1. Firestorm's harness run *does* filter anisotropically — the stock
   `settings.xml` default is `0`, but the GPU feature table sets
   `RenderAnisotropic 1` for every level from Mid up, and the run's saved
   `settings.xml` says `1`. But zoomed in, its sphere edges are hard stair
   steps: a 512² checker minified, not a filtered one.
2. Ours had a 2–3 px red-green blend band along every edge — a texel
   *magnified*. Logging `record_decoded` showed the checker first decoded at
   128² (`INITIAL_MANAGED_DISCARD`, discard 2) and upgraded to 512² a second
   later (the NPC's worn box boosts it). `refresh_lod_image` ran for it and
   touched 23 materials.
3. With the first managed fetch forced to discard 0, the sphere was
   pixel-identical to Firestorm's in the edge crops; a whole-frame diff against
   the normal run lit up **every** face wearing the checker, boxes included —
   they only hid it better, being nearer to their texel size.

So every face kept sampling the image it was first draped with. The cause was
the "touch" meant to re-prepare its materials:
`materials.get_mut(id).is_some()`. In Bevy 0.19 `Assets::get_mut` returns an
`AssetMut` that queues `AssetEvent::Modified` **only when mutably
dereferenced** — the lookup alone raises nothing, so no material was ever
re-prepared and its bind group kept the old GPU texture view. That made the
whole P21.1 upgrade path (`set_lod_for_area`, `upgrade_to_full`) invisible:
the store fetched and decoded the finer level, and no prim face ever showed it.
`media_engine.rs` touched its materials the same way on a surface resize.

A second path had the same outcome by a different route: the PBR map, legacy
normal / specular map and generated bump-map caches each upload an image from
whatever decode the store holds when the first face asks, and never look again.
For a texture an ordinary face is also showing, that is the coarse first level
(the `pbr-box` top face still showed the blend band after the touch was fixed);
for a bump map it is whatever level the diffuse was at, which then follows the
camera in both directions without the map following it.

Also found on the way: Bevy 0.19 prepares `MeshMaterial3d<M>` with no
ordering after `prepare_assets::<GpuImage>` (its 2D and UI materials keep that
edge), so a material and an image replaced behind its handle, prepared in the
same render frame, can bind the old view. The executor's current arbitrary
order happens to put the image first — the fixed viewer rendered identically
with and without the edge — but nothing keeps it there, so the edge is carried
in the Bevy fork. It costs no parallelism: a material's bind-group param reads
`RenderAssets<GpuImage>`, which image prepare writes, so the two never ran
concurrently anyway.

## Fix

- Bevy fork (`4598842b`): `ErasedRenderAssetDependency` for `GpuImage`, and
  `MaterialPlugin` registers `ErasedRenderAssetPlugin::<MeshMaterial3d<M>,
  GpuImage>`.
- `textures.rs` / `media_engine.rs`: the touch is
  `get_mut(id).map(AssetMut::into_inner)`.
- `textures.rs`: `DerivedImage` — a cached handle plus the size of the decode it
  was built from — and `refresh_derived_images`, which rebuilds a stale entry in
  place under the shared image budget (the rest waits for a later frame) and
  marks every face material sampling a rebuilt image in any slot modified.
- The PBR (`refresh_pbr_textures`), legacy map (`refresh_legacy_map_images`)
  and bump (`refresh_bump_normals`) caches hold `DerivedImage`s, and each
  refresh runs right after its cache's first-use builds in the face-material
  pipeline.
- Unit tests: a LOD refresh raises `Modified` for each live material (and fails
  with the old touch); a derived image is rebuilt from a new-size decode, leaves
  a current one alone, respects the budget, and re-prepares only its samplers.

## Verified

`sl-crosscheck --only sl-client --scenario catalogue --look-at pbr-box
--look-from 3 --look-above 4`: the fixed viewer's frame is identical (no pixel
differing by more than 24) to a run whose managed textures were fetched at full
resolution from the start, where the unfixed one differed on the sphere, the
boxes and the PBR box's top face; the sphere's edges now match Firestorm's.

Anisotropic sampling is still missing and remains
[[viewer-antialiasing-sharpen-aniso]]; this viewer also builds no mip chain for
face textures, filed as [[viewer-texture-mip-chain-missing]].
