---
id: viewer-underwater-name-tags-not-drawn
title: Name tags are not drawn at all while submerged
topic: viewer
status: done
origin: user report while verifying viewer-nametags-occluded-by-clouds (2026-08-29)
refs: [viewer-nametags-occluded-by-clouds, viewer-particle-water-ordering]
---

Context: [context/viewer.md](../context/viewer.md).

Underwater, an avatar's name tag did not render **at all**. Above water the same
tag rendered correctly, including against the cloud layer.

## Cause (confirmed)

Not a name-tag bug: **the pre-water suppression was not per view**, so whole
views lost their below-water translucency.

`sl-viewer-world-scene/src/transparency.rs` draws below-water translucency in
`pre_water_transparent_pass_3d` and then empties those items' batch ranges in
`suppress_pre_water_items`, so Bevy's own transparent pass does not draw them a
second time. But Bevy 0.19 runs the `Core3d` schedule **once per view** — that
is what `ViewQuery` resolves against, and Bevy's own
`main_transparent_pass_3d` takes one exactly as our pre-water pass does. Every
view owns a *separate* phase with its own item list and its own batch ranges.

`suppress_pre_water_items` iterated **every** view's phase, so the first view to
run zeroed the ranges of views whose pre-water pass had not run yet. Those views
then drew nothing early (`render_range` skips an empty range) *and* skipped the
items in their transparent pass, so the content was suppressed without ever
having been drawn. The viewer has several views — the main camera, the HUD
camera, and the reflection-probe capture cameras, which cycle every frame — so
which view lost its below-water content came down to schedule order.

Name tags were merely the visible symptom: submerged is the only time a tag is
bucketed below the water at all, which is why the bug looked specific to them
and specific to being underwater.

Ruled out along the way:

- **Not the underwater fog.** With `SL_VIEWER_DISABLE_UNDERWATER_FOG=1` (which
  forces the density to zero, a shader no-op, for both the above-water haze and
  the submerged pass) the tag was still absent — never drawn, not drawn and then
  washed out.
- **Not the sky-backdrop bucket.** Reproduced identically on a build with
  [[viewer-nametags-occluded-by-clouds]] stashed out. Name tags are not
  backdrops, so their bucket is `BELOW_WATER` before and after that change.

## Fix

`suppress_pre_water_items` takes a `ViewQuery<&ExtractedView>` and clears only
its own view's phase, through a `suppress_view_pre_water_items` helper split out
so a test can drive it for a chosen view without a render app.

Two regression tests build two views' phases with non-empty batch ranges:
`suppression_touches_only_its_own_view` asserts that suppressing view 0 empties
view 0's below-water prefix **and leaves view 1's items drawable**, and
`a_view_without_a_split_is_untouched` covers the no-split case.

Verified live on the local grid: submerged name tags and prim hover text both
render.

## Not verified

That below-water translucency seen **from above** the water is not now drawn
twice (which would read as too dense) — the third case the change could have
affected.

It could not be checked on the local grid, for a benign reason: with the OpenSim
regions' water settings the sea reads as solidly blue rather than see-through,
so there is nothing visible under it to judge. That is **correct** behaviour and
not a regression: the sea only looked unnaturally see-through while it was
alpha-blended, which let the underwater fog colour show from above, and
`a3beaf44` ("the sea shows the water behind it, not a tint standing in for it")
made the surface opaque as the reference's is. No translucent prim was to hand
either.

So this wants a purpose-made translucent prim, or a look on aditi, before the
double-draw case is called clear.
