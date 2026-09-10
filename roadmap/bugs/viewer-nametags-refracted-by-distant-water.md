---
id: viewer-nametags-refracted-by-distant-water
title: Water draws over a name tag that stands in front of it, smearing it
topic: viewer
status: bugs
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

## The mechanism to check first

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

So the question is why a tag above the waterline is bucketed pre-water. Look at
how `split_transparent_by_water` classifies an item — it buckets by the item's
**mesh centre** relative to the eye — and at what centre a camera-facing
billboard actually reports. The tag entity is anchored at the avatar's head, but
the billboard mesh and its pull-toward-camera (`NameTagPullRadius`) may leave
the sorted centre somewhere the classifier reads as the far side.

## Where a fix probably belongs

Name tags and hover text are **overlays**, not world translucency: the reference
draws its `LLHUDText` in its own late pass, after the alpha pools, not inside
them. The sibling bug [[viewer-nametags-occluded-by-clouds]] already carved out
one ordering exception for them (the sky-backdrop bucket, so camera-anchored
backdrops stay behind world-anchored overlays). This looks like the same shape
of exception against the other neighbour: whatever the classifier decides, a
name tag must never be drawn into the pre-water pass, and so never into the
screen copy the water refracts.

Check hover text (`hover_text.rs`) at the same time — it shares the billboard
machinery and would take the same exception.

## Verify

Stand an avatar on land with open sea behind it, at a camera angle where the sea
covers the screen space the name tag occupies. The tag must be crisp and fully
in front. Repeat submerged (the `waterSign` flip case) and with hover text over
a prim.
