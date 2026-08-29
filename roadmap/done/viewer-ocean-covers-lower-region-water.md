---
id: viewer-ocean-covers-lower-region-water
title: The endless ocean is drawn through every region, so a lower sea level is hidden under it
topic: viewer
status: done
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

## Fixed (2026-08-29)

The sea is now a **grid of one 256 m square per region cell** and nothing else.
Each cell is a loaded region's own water at that region's height, or void water
at a height it inherits. There is no ocean plane any more, no depth bias, and no
place where two water surfaces cover the same ground — which is the reference's
rule, arrived at by its own route (`LLSurface::createObjects` for a region's
water, `LLWorld::updateWaterObjects` for the holes and edges).

`WaterOcean` and `WaterRegionPlane` collapse into one `WaterCell`, which is also
what the two places that ask "is this entity water" — the alt-click focus pick
and the GPU pick registry — now match on.

The grid follows the camera, 17 cells in each direction: the far plane is 4096 m
and nothing is drawn past it, so 16 cells cover everything visible and one more
absorbs the cell the camera sits inside. 35x35 squares share one mesh and one
material, so they batch, and only crossing a cell boundary spawns or despawns
anything.

### Void water inherits, rather than falling back

Deliberately better than the reference here, and the reason is a case the
reporter put: an agent region ringed by eight regions whose sea is lower, void
beyond. The reference puts every cell with no region back at the *agent's* level
(*"Use the water height of the region we're on for areas where there is no
region"*), which steps the water up again at the outer edge of that ring for no
reason visible from there.

Instead, a void cell takes the level of the **nearest** loaded regions. Looking
only at its immediate neighbours would not do: the second ring of void has no
loaded neighbour at all and would fall back to the agent's height, moving the
step outward rather than removing it. Distance is Chebyshev, so a diagonal
neighbour is one ring like an edge neighbour — "surrounded by eight regions" is
one ring, which is how it looks. Ties go to the majority and then to the
**lower** level: void water too high reads as a wall standing over the
neighbouring sea, too low only reveals a little more of the void it was
covering. With nothing loaded at all it falls back to the agent's level, which
is the reference's rule kept for the one case where it is the only answer
available.

### A seam and a jitter that came off with it

Both were precision, and both are now bounded by construction rather than
patched:

- The subdivision added to the ocean plane for
  [viewer-water-surface-alpha-not-refraction](viewer-water-surface-alpha-not-refraction.md)
  is gone with the plane. A 256 m quad has no `w` range worth losing precision
  over, so the diagonal seam cannot come back.
- The wave texcoords are built from world coordinates, which the 40 km plane
  made large enough to quantise the scroll. Measured after this change, at a
  parked camera over open water: consecutive frames differ by 14.0 to 14.5 grey
  levels in the near water, a 3% spread, which is a smooth scroll. It does not
  settle
  [viewer-water-wave-phase-jumps-far-from-origin](../bugs/viewer-water-wave-phase-jumps-far-from-origin.md)
  — a parked camera cannot show a jump that depends on moving — but the
  mechanism that task blames is much reduced, and the shader can now go
  region-local cheaply if any of it remains: each cell's model matrix *is* its
  origin.

### Tests

Five, all on the pure height rule, which is where the decisions live: a loaded
region keeps its own level (the bug — a region below the agent's used to be
covered by the ocean); the reporter's ring case, checked at the first ring, the
second, and far out in another direction; a tie going to the majority and, when
even, to the lower level; and the empty case falling back to the agent's.

### Left open

- The water haze still measures against a single water level (the agent
  region's), so a region at a different level fogs against the wrong plane. The
  reference has the same limitation — its haze takes `LLEnvironment`'s one water
  height — so this is parity, not a regression, but it is visible now that
  neighbouring levels really can differ.
- Verifying the differing-level case needs two regions at different water
  heights on the local grid, which the estate tools can set; the unit tests
  cover the decision, not the picture.
