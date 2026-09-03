---
id: viewer-fake-grid-render-catalogue
title: The full-stack test catalogue — NPCs, attachments, teleport, crossing, parcels, environment
topic: viewer
status: ready
origin: test-harness plan (2026-08-30)
points: 8
refs: [viewer-fake-grid-render-harness]
blocked_by: [test-fake-grid-neighbours-crossing]
---

Context: [context/testing.md](../context/testing.md).

Everything the first four full-stack tests do not cover, each with the
oracle it uses: an NPC appears with its bakes and plays its animation
(bake colour in the disc; two captures a second apart differ); an
attachment follows its avatar across a scripted move; a teleport keeps
the subject prim where it is (centroid within two pixels); a region
crossing keeps the subject and shows both terrains; a teleport with
in-flight assets leaks nothing between regions (no old-region objects
unless it is a neighbour, in-flight counts zero, no old texture on new
prims); parcel properties draw the border line and `ChangeParcel` removes
it; an environment change to night halves the sky band's luminance;
hover text above its prim; the media face fetches its `ObjectMedia` and
shows the placeholder; a name tag above an NPC.

Also `Grid::Fake` in `sl-conformance` gets `region-crossing`,
`neighbour-child-circuits`, `terrain-layerdata` and
`avatar-appearance-npc` cases on the same fixture catalogue.

Unblocked 2026-09-03 by [[test-fake-grid-neighbours-crossing]], which
took two of the listed subjects with it because they *were* the check
that the crossing works at all: `a_border_crossing_keeps_the_picture_still`
(the subject's centroid within two pixels across the handover, plus no
teleport / disconnect) and `a_neighbour_region_is_rendered_across_the_border`
(the region next door drawn, on its own ground, before the crossing). What
is left of the crossing subject here is **both terrains at once** — a
framing that shows the ground either side of the border, which neither of
those asserts. The `border` scene it added is the fixture for it, and the
grid's asset store is now grid-wide (it was per region, which made a
neighbour's untextured prim read as "never streamed") — so a two-region
pixel test no longer needs to duplicate assets across regions.
