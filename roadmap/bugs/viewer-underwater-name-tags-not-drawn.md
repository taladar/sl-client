---
id: viewer-underwater-name-tags-not-drawn
title: Name tags are not drawn at all while submerged
topic: viewer
status: bugs
origin: user report while verifying viewer-nametags-occluded-by-clouds (2026-08-29)
refs: [viewer-nametags-occluded-by-clouds, viewer-particle-water-ordering]
---

Context: [context/viewer.md](../context/viewer.md).

Underwater, an avatar's name tag does not render **at all**. Above water the
same tag renders correctly, including against the cloud layer.

## What is established

- **Not the underwater fog.** Re-run with `SL_VIEWER_DISABLE_UNDERWATER_FOG=1`,
  which forces the fog density to zero (a shader no-op) for both the above-water
  haze and the submerged pass: the tag is still absent. So it is not being drawn
  and then washed out — it is never drawn.
- **Pre-existing.** Reproduced identically on a build with
  [[viewer-nametags-occluded-by-clouds]]'s backdrop bucket stashed out. The
  backdrop bucket does not touch it either way: name tags are not backdrops, so
  their bucket is `BELOW_WATER` both before and after that change.

## Leading hypothesis: the pre-water suppression is not per-view

`sl-viewer-world-scene/src/transparency.rs` draws below-water translucency in
`pre_water_transparent_pass_3d` and then empties those items' batch ranges in
`suppress_pre_water_items`, so Bevy's own transparent pass does not draw them a
second time.

But Bevy 0.19 runs the `Core3d` schedule **once per view** — its own
`main_transparent_pass_3d` takes a `ViewQuery`, exactly as our pre-water pass
does. `suppress_pre_water_items` is a plain system that iterates **every**
view's phase in `ViewSortedRenderPhases`, so the first view's run zeroes the
batch ranges for *all* views. Any view rendering after that finds the pre-water
items already emptied: `render_range` skips them, so its pre-water pass draws
nothing, and its main transparent pass skips them too. The content is suppressed
without ever having been drawn.

The viewer has several views — the main camera, the HUD camera, and the
reflection-probe capture cameras, which cycle every frame — so which view loses
its below-water translucency depends on schedule order.

That matches the symptom: submerged, the tag's centre is below the waterline, so
it is bucketed below-water and goes through the pre-water pass; above water it
never enters that path, which is why only the submerged case breaks.

## Fix sketch

Make the suppression per-view: clear only the phase belonging to the view whose
pass just ran, by giving `suppress_pre_water_items` the same `ViewQuery` the
pass uses (or by folding the suppression into the pass itself, which already has
the view in hand). Then re-check with a probe-heavy scene, since the probe
cameras are the views most likely to be racing the main one.

## Also unverified

Whether **hover text** on a prim disappears the same way underwater — it shares
the billboard renderer, so it should behave identically if the diagnosis is
right, and differently if the cause is specific to avatar tags. Worth checking
first, since it is a one-minute test that either strengthens or kills the
hypothesis above.

The same reasoning predicts that **any** below-water translucent content
(particles, translucent prims below the surface) intermittently vanishes, which
would be a much bigger symptom than name tags alone — check that too before
settling on the diagnosis.
