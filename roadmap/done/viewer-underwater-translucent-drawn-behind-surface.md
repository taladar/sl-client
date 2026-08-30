---
id: viewer-underwater-translucent-drawn-behind-surface
title: A submerged camera sees translucent objects as if beyond the water surface
topic: viewer
status: done
origin: user report while verifying viewer-transparency-all-faces-skips-top (2026-08-30)
refs: [viewer-transparency-all-faces-skips-top, viewer-underwater-name-tags-not-drawn]
---

Context: [context/viewer.md](../context/viewer.md).

With the camera **underwater** and a **translucent** object also underwater,
looking at the object with the water surface behind it draws the whole object as
if it were on the *far* side of the surface — the sea's own shading covers it.
Seen on the local OpenSim with a 50 %-transparent box, camera below it, surface
above and beyond.

## Cause

`sl-viewer-world-scene/src/transparency.rs` splits the `Transparent3d` phase
into the reference's `POOL_ALPHA_PRE_WATER` / `POOL_WATER` /
`POOL_ALPHA_POST_WATER` order. The below-water bucket is drawn early
(`pre_water_transparent_pass_3d`, before the transmissive pass) so it lands in
the screen copy the water surface refracts; the water then renders
**opaque and depth-writing** in `Transmissive3d`.

`classify_bucket` decides that bucket from the item's centre height alone:

```text
if mesh_center.y >= level { ABOVE_WATER_BUCKET } else { BELOW_WATER_BUCKET }
```

That is right only while the eye is above the surface. Submerged, everything
below the plane is **between the camera and the surface**, not behind it — so a
translucent object is drawn pre-water, writes no depth (it is alpha-blended),
and the water surface, drawn afterwards with a depth write, paints straight over
it. The object comes back only as part of what the sea refracts, which is
exactly the "it looks like it is on the other side of the surface" the report
describes.

## What the reference does

The reference splits the *same* two pools, but clips them **per fragment against
a plane whose sign follows the eye** (`lldrawpoolalpha.cpp:151`):

```text
F32 water_sign = 1.f;
if (getType() == LLDrawPool::POOL_ALPHA_PRE_WATER) water_sign = -1.f;
if (LLPipeline::sUnderWaterRender)                 water_sign *= -1.f;
```

so `waterSign` / `WATER_WATERPLANE` make the pre-water pool draw whatever is on
the **far side of the water plane from the camera**. `LLPipeline::updateCull`
flips its cull plane the same way (`pipeline.cpp:2627`, *"camera is below water,
cull above water"*).

## Fixed (2026-08-30), live-verified

`classify_bucket` now takes the eye's side of the surface and picks the
pre-water bucket by *far side of the water plane from the eye*, inverting the
test when submerged — the port of `water_sign *= -1`. The buckets are renamed
`PRE_WATER_BUCKET` / `POST_WATER_BUCKET`, since below/above was only ever their
above-water meaning, and `sort_transparent_by_water` resolves each view's eye
height from its `ExtractedView` (the main camera can be submerged while a
reflection probe's capture camera is not).

Verified on the local grid: a submerged camera now draws the translucent cube in
front of the sea instead of behind it. Guarded by
`a_submerged_eye_swaps_the_sides`,
`a_dry_eye_buckets_what_is_under_the_surface_pre_water` and
`the_eye_counts_as_submerged_at_the_surface`.

Also added, since it is what isolated this: `SL_VIEWER_DISABLE_PRE_WATER_PASS=1`
records no pre-water split at all, so an artifact caused by the split can be
told from one in the drawn item itself (the sibling of
`SL_VIEWER_DISABLE_UNDERWATER_FOG`).

## The fix

Give `classify_bucket` the eye's side of the water plane and invert the
below/above test when submerged — the port of `water_sign *= -1`. The buckets
themselves keep their meaning (pre-water = far side of the surface, post-water =
the camera's side), so nothing downstream changes: the pre-water pass still
feeds the refraction copy, and the camera-side translucency is drawn after the
water and depth-tests against the depth the water wrote.

The eye position is per **view**, not global (probe capture cameras are their
own views), so `sort_transparent_by_water` — which today reads only
`ViewSortedRenderPhases` keyed by `RetainedViewEntity` — needs each view's
`ExtractedView` to resolve its camera height, the same join
`pre_water_transparent_pass_3d` already makes.

The sky backdrops must stay exempt whichever way the test runs: their mesh
centre *is* the camera, which is why `classify_bucket` tests the backdrop marker
first.
