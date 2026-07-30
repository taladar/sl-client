---
id: viewer-particle-water-ordering
title: Translucent content (particles) orders wrong against the water surface
topic: viewer
status: done
origin: found live-testing GPU particles on aditi (2026-07-30)
refs: [viewer-perf-gpu-particles, viewer-straddling-transparency-oit]
---

Context: [context/viewer.md](../context/viewer.md).

**Done (2026-07-30, live-confirmed on aditi — particles now draw in front of the
water as they should).** Ported the reference `LLDrawPoolAlpha` pre/post-water
split to the Bevy renderer:

- The **water surface writes depth** (`WaterMaterial::specialize`,
  `sl-client-bevy/src/water.rs`), matching the reference
  `LLGLDepthTest(GL_TRUE, GL_TRUE)`.
- A **render-world re-sort** (new `sl-client-bevy-viewer/src/transparency.rs`,
  `TransparencyOrderPlugin`) buckets **every** `Transparent3d` item — particles
  *and* prims, whoever queued them — by centre height relative to the water
  level: below-water → water → above-water, with the water pinned to its own
  bucket (a `WaterSurface` marker, since a camera-following / region-sized
  plane's mesh centre is a useless sort key). It runs once in
  `RenderSystems::PhaseSort` after Bevy's `sort_phase_system::<Transparent3d>`
  and re-sorts each view's phase by `(water_bucket, distance)`, preserving
  back-to-front order within each bucket. One interception point covers all
  transparent content without touching `queue_material_meshes`.

Below-water translucency draws first (composited, so it shows *through* the
translucent surface — the failure mode of the naive "water writes depth" fix),
the water draws next writing depth, and above-water translucency draws last with
per-pixel depth occlusion against the surface. Bucket assignment is per-object,
so a single large translucent prim straddling the waterline is still classified
whole — the per-pixel remainder is filed as
[[viewer-straddling-transparency-oit]].

The water surface (a camera-following alpha-blended plane, `WaterMaterial`,
`AlphaMode::Blend`) draws **over** particles that are in front of it — a
fountain's streams are painted out by the water behind them. Both the water and
the particles are translucent and neither writes depth, so their order is
decided by a single per-object back-to-front sort, and the huge water plane's
sort centre is always near the eye (it follows the camera), so the whole plane —
including the parts behind the fountain — sorts near and draws last (on top). A
single plane cannot be sorted per-region, so it is all-or-nothing.

More generally this is **translucent-vs-translucent ordering**, not a
particle-only issue: any translucent content near the surface (boat wakes,
waterfall bases, splashes, translucent prims under the water) needs
**per-pixel** ordering against the water, which a per-object sort cannot give.

## Why the quick fixes do not work

- **Water writes depth (+ sorts first).** Fixes the particle ordering per-pixel
  and even culls the occluded fragments (a real frame-rate win over water),
  *but* the water is **translucent** — writing depth makes it an opaque occluder
  for depth, so everything translucent behind it (underwater particles,
  translucent prims) is hidden instead of seen through. Not acceptable. (Tried
  and reverted 2026-07-30.)
- **Water sorts first without writing depth.** Then *all* translucent content
  draws after (over) the water, so underwater content paints over the surface —
  also wrong.

## The reference approach (the real fix)

`LLDrawPoolAlpha` splits the alpha pool by the region water height into
`POOL_ALPHA_PRE_WATER` → `POOL_VOIDWATER` → `POOL_WATER` →
`POOL_ALPHA_POST_WATER` (`lldrawpool.h`; the pre/post decision is the spatial
group's Z bounding box vs `water_height`, `lldrawpoolalpha.cpp:609+`). The water
**does** write depth (`LLGLDepthTest(GL_TRUE, GL_TRUE)`,
`lldrawpoolwater.cpp:120`), but underwater alpha is drawn in the **pre-water**
bucket *before* the water — so it is already composited and shows through the
translucent surface, and the water's depth-write only occludes post-water
(above-water) content, of which nothing is behind the surface. That preserves
translucency **and** orders per-pixel.

Porting this to the Bevy renderer means bucketing **every** translucent object
(particles *and* prims) pre/post-water and drawing the water between them with a
depth write — i.e. reorganising the whole `Transparent3d` queue, not just the
particle queue (`particle_render.rs` fully controls its own items and could
bucket easily; the transparent *prims* are queued by Bevy's
`queue_material_meshes` and would need interception, a custom sub-phase, or a
per-height render ordering). This is a render-architecture task, not a particle
tweak, which is why it is filed separately from [[viewer-perf-gpu-particles]].

A **particle-only sort-bias stopgap** (bucket particle clouds above/below water
by their centroid height and bias them to draw after/before the water, no water
depth-write) was considered: it preserves translucency and fixes the common
above-water case, but it is per-cloud not per-pixel (a cloud straddling the
surface is all-or-nothing), above-water particles then draw over *other*
translucent content, and it forgoes the depth-cull frame-rate win. Not adopted;
recorded in case a cheap partial improvement is wanted before the full fix.
