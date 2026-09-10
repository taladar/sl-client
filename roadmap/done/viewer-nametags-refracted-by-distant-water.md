---
id: viewer-nametags-refracted-by-distant-water
title: Water draws over a name tag that stands in front of it, smearing it
topic: viewer
status: done
origin: user report during the hold-to-fly live test on OpenSim (2026-09-10)
refs:
  - viewer-nametags-occluded-by-clouds
  - viewer-underwater-translucent-drawn-behind-surface
  - viewer-particle-water-ordering
---

Context: [context/viewer.md](../context/viewer.md).

Observed live (OpenSim): with the **sea behind** an avatar, the water is drawn
**over** that avatar's name tag. The tag is not merely dimmed — it picks up the
water surface's own distortion, and comes out barely readable.

The sea is *further from the eye* than the tag, so this is not a case of
looking through water at a tag beyond it. Something draws the tag first and the
water second.

## The mechanism

The water is not in [`Transparent3d`] at all: it renders in Bevy's
`Transmissive3d` phase, **opaque and depth-writing**, sampling the screen copy
Bevy takes at the start of that pass (`transparency.rs`, the
[[viewer-particle-water-ordering]] work). Translucent content on the far side of
the surface is deliberately drawn *before* it, in the pre-water pass, so it is
inside that copy.

A name tag is `AlphaMode::Blend`, so it is depth-**tested** but writes **no
depth**. If it lands in the pre-water bucket, both halves of the symptom follow
at once: it is painted into the screen copy (so the water refracts it — the
distortion), and the later water fragments, testing depth against the opaque
geometry behind the tag rather than against the tag, pass and paint over it (so
it ends up behind). That is exactly what was reported.

`classify_bucket` buckets by the item's **mesh centre relative to the eye**, and
a tag's centre is where its anchor is: `TAG_UNITS_PER_PIXEL` keeps the mesh AABB
tiny (px ÷ 1024) precisely so the sort key stays on the avatar's head, and the
entity transform is that head's world position. So the two states that hand a
tag to the pre-water pass are:

- **the eye submerged and the tag above the surface** — the far side flips with
  the eye (the reference's `waterSign`), and a third-person camera dips under
  the sea readily while the avatar it follows stands on the shore;
- **the tag below the surface with the eye above it** — an avatar low enough at
  the waterline, and every `llSetText` on a prim under the sea.

## Fix, part one: ordering

A `WorldTextOverlay` marker
(`sl-viewer-world-objects/src/name_tag_billboard.rs`) on every world-anchored
text billboard — the tag entity, each extra atlas page child of one, and object
floating text — mirrored into the render world by
`extract_world_text_overlays` and read by `classify_bucket`, which puts such an
item in a bucket of its own, `TEXT_OVERLAY_BUCKET`, **whatever its centre and
whatever the eye is doing**. The buckets now ascend pre-water → backdrop →
post-water → text overlay → always-on-top, so this text is drawn after all
world translucency (the reference's own order) and, because the early passes
draw a *prefix* of the sorted phase, never early: never in the screen copy the
sea refracts, never painted over by the sea.

Its own bucket rather than always-on-top so tags keep Bevy's back-to-front
distance order among themselves: `TransparentSortingInfo3d::AlwaysOnTop` sorts
at `f32::NEG_INFINITY`, which would tie every tag in the scene and leave
overlapping bubbles to composite in insertion order.

## Fix, part two: the depth test, which is the reference's

Ordering alone traded one artifact for another, and the live run showed it: with
the text drawn *after* the sea, the sea — opaque and depth-writing — began
**cutting it out** instead of smearing it. An avatar on the shore lost its tag
entirely to a camera under the surface, and floating text above the water
vanished when looked at through the surface from below.

The premise that was wrong is the one this module was built on: that the
reference depth-tests its text against the world. It does carry
`LLGLDepthTest(GL_TRUE, GL_FALSE)` (`llhudnametag.cpp:231`,
`llhudtext.cpp:136`), but `LLHUDObject::renderAll()` runs from `render_ui`
**after `gPipeline.renderFinalize()`** (`llviewerdisplay.cpp:1689`), drawing
into the *default framebuffer* — and the one call that would put the scene's
depth there is commented out in `pipeline.cpp` (`copyContentsToFramebuffer(…
GL_DEPTH_BUFFER_BIT | GL_STENCIL_BUFFER_BIT …)`). The test therefore passes
everywhere. That is why a name tag and `llSetText` floating text are famously
readable **through a wall** in Second Life.

So `NameTagMaterial::specialize` now sets `depth_compare: Always` (it still
writes no depth). Tags and floating text are occluded by nothing — reference
parity — and the sea cannot cut them out. This is a deliberate behaviour change
beyond the letter of this bug: tags that used to disappear behind a prim or a
hill now show through it, as they do in the reference.

## Diagnostics

`sort_transparent_by_water` counts, and logs on change at `debug`, how many
overlays the carve-out kept out of the early passes and how many views have a
submerged eye — the numbers that say whether a "the sea drew over my tag" report
is this ordering at all.

## Tests

Unit: an overlay is in the text-overlay bucket in all four (height × eye)
combinations, the same centres *without* the marker still bucket by the water
(so the test cannot pass with the carve-out deleted), a phase of overlays draws
nothing early, the five buckets sort in draw order, and each of the three render
bundles carries the marker.

Live (OpenSim, `RUST_LOG='warn,sl_viewer_world_scene::transparency=debug'`),
first run — ordering only, before the depth-test half:

- tag with the sea *behind* it: crisp, no smear;
- tag above water, camera below the surface: tag **gone** → part two;
- floating text above water seen from below the surface: gone → part two;
- floating text below water seen from above, and both below: fine.

The `water ordering: N …` line read `N = 1` exactly while the camera was under
the surface with a tag above it, and `N = 0` otherwise.

Second run, with both halves: **all four (text × eye) combinations readable, for
name tags and for `llSetText` alike** — confirmed live by the user.

## What this did NOT explain, and the trail if it comes back

The report's own state — **flying high above the sea, nothing submerged**, the
sea filling the background behind the tag — is not this bug's mechanism, and
saying otherwise would be a fiction the diagnostic contradicts:

- in that state `kept_out` is **0**: an above-water tag with an above-water eye
  is post-water, and was before this change too. The first verification build
  was therefore behaviourally identical to master for that scene, and the tag
  was already crisp in it;
- re-flown in the finished build, no smear either.

So the original observation did not reproduce. If it returns, two things are
already ruled out — glow bleeding from the sea (`water.wgsl` returns glow-mask
alpha `0.0`, and `glow_extract.wgsl` extracts `rgb * a`) and temporal ghosting
(the main camera is `Msaa::Sample4`; there is no temporal pass) — and what is
left is *post*-water and legibility-shaped: the 50 % `BubbleOpacity` backdrop
over a bright, high-contrast, wavy sea (the wave pattern read *through* the
bubble looks exactly like the tag picking up the water's distortion), and the
auto-exposure ([[viewer-tonemap-auto-exposure]]) pulling a white tag down
against a near-blown sea. `SL_VIEWER_DISABLE_GLOW=1` and
`SL_VIEWER_DISABLE_DYNAMIC_EXPOSURE=1` are the A/B knobs for the second.
