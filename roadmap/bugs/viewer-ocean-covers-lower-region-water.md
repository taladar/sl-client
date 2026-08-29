---
id: viewer-ocean-covers-lower-region-water
title: The endless ocean is drawn through every region, so a lower sea level is hidden under it
topic: viewer
status: bugs
origin: question raised while reviewing viewer-water-surface-alpha-not-refraction (2026-08-29)
points: 3
refs: [viewer-water-surface-alpha-not-refraction]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-world-scene/src/water.rs` renders the sea as one endless-ocean plane
at the **agent region's** water height, spanning everything, plus a per-region
plane for each loaded region whose height *differs* from the agent's, biased 2
cm above the ocean so the two do not z-fight. The ocean is never cut out of a
region's footprint.

Water level is a per-region setting, so a neighbour's can sit either side of the
agent's, and the two cases do not fare the same:

- **Higher** than the agent's: the region's plane is above the ocean and wins.
  Correct, except that the ocean is still there 2 cm under it, and at a grazing
  angle the parallax between two surfaces that close is metres of displaced wave
  pattern.
- **Lower** than the agent's: the ocean is *over* that region's plane and wins.
  The region's sea is simply not visible. This got worse with
  [viewer-water-surface-alpha-not-refraction](../done/viewer-water-surface-alpha-not-refraction.md):
  the water used to be alpha-blended, so the overlap read as a faint double
  surface (which the module docs called an accepted trade), and it is now opaque
  and depth-writing, so the lower one is hidden outright.

## What the reference does

Not what the module currently assumes, and worth writing down because the
intuition ("void water should match the region next to it") is wrong in a
specific way:

- **Void water is at the agent's height**, exactly as ours is.
  `LLWorld::updateWaterObjects` says so in a comment — *"Use the water height of
  the region we're on for areas where there is no region"* — and takes
  `gAgent.getRegion()->getWaterHeight()` for every hole and edge object. So a
  neighbouring region's level does *not* propagate into the void beside it, and
  a step at that boundary is the reference's behaviour too.
- **Every loaded region has its own water surface** at its own height
  (`LLSurface::createObjects` gives each region an `LLVOWater`, placed at the
  region centre; `LLSurface::setWaterHeight` moves it).
- **The gaps are filled per cell.** `updateWaterObjects` walks a 256 m grid over
  ±512 m of the agent, and for each cell with no region creates a hole-water
  object at the agent's height; four edge-water objects then cover the rest of
  the draw distance.

The shape of it: the reference never draws two water surfaces over the same
ground. Every square metre of sea belongs to exactly one object, either a
region's or a hole's — which is why it needs no depth bias, and why a differing
neighbour reads as a clean step rather than a fight.

## Fix

Port that footprint rule. Every loaded region gets a plane at its own height
(not only the differing ones), and the endless ocean becomes hole / edge water
that stops at loaded footprints — either the reference's 256 m cell grid, or one
plane with the footprints punched out. The cell grid is the simpler port and the
subdivided ocean mesh
([viewer-water-surface-alpha-not-refraction](../done/viewer-water-surface-alpha-not-refraction.md)
cut it into ~625 m cells already) shows the triangle count is not the concern.

`OCEAN_DEPTH_BIAS` goes with it: with no overlap there is nothing to bias apart,
and the 2 cm is itself the cause of the grazing-angle parallax above.

Verify with two regions at different water heights on the local grid — the
megaregion's four regions can be given different levels through the estate /
region tools — looking along the boundary from just above the water, from both
the higher and the lower side.
