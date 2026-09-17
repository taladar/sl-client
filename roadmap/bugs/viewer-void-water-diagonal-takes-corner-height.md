---
id: viewer-void-water-diagonal-takes-corner-height
title: Void water on a block's diagonal takes the corner region's sea level
topic: viewer
status: bugs
origin: user report on aditi while verifying viewer-horizon-thin-line-flashes
  (2026-09-17)
refs: [viewer-sea-grid-edge-visible-from-height, viewer-p23-1]
---

Context: [context/viewer.md](../context/viewer.md); code
`sl-viewer-world-scene/src/water.rs` (`cell_height`, `cell_distance`,
`majority_height`).

Seen on aditi, in a block of four loaded regions whose **south-east** region's
sea sits higher than the other three. Looking out from there, a run of void
("endless ocean") cells going off **diagonally to the south-east** was drawn at
that higher level, while the rest of the void around the block was at the
lower level shared by the other three. So a raised strip of sea runs out to the
horizon along the diagonal, with nothing there to explain it.

## Why the algorithm does this

A void cell takes the level of its **nearest** loaded regions, by **Chebyshev**
distance, with ties going to the majority and then to the lower level. That rule
was chosen so that a region ringed by eight neighbours counts as one ring. But
it leaves exactly one set of cells with a single nearest region: the cells on a
corner region's outward diagonal.

Take a void cell `(dx, dy)` cells east and south of the south-east region
(`dx, dy >= 1`). Its distance to each region is:

- south-east: `max(dx, dy)`;
- north-east (one cell north): `max(dx, dy + 1)`;
- south-west (one cell west): `max(dx + 1, dy)`.

Away from the diagonal (`dx != dy`), one of the other two regions ties with the
south-east one. The vote is then 1:1, and the lower level wins. **On** the
diagonal (`dx == dy`), both of the others are one ring further out. So the
south-east region is the only nearest region, and every cell along that
diagonal takes its height, however far out it goes.

The same holds at any convex corner of a loaded area, in any direction, whenever
the corner region's level differs from its neighbours'. It was only noticeable
here because the corner region was the odd one out.

## What a fix has to decide

What level the void past a corner *should* have is a design question, not
something to port from the reference. The reference uses the agent region's
height for all void (`LLWorld::updateWaterObjects`), which the nearest-region
rule deliberately improved on (see the `cell_height` doc). Options worth
weighing:

- A distance that does not single out the diagonal (Euclidean, or Manhattan), so
  the cells past a corner see the edge neighbours as near as the corner.
- Vote over every loaded region within some radius, weighted by distance, rather
  than only over the nearest ring.
- Keep the rule, but let a lone nearest region win only when nothing else is
  within one extra ring.

Whatever is chosen, the existing tests in `water.rs` pin behaviour that must
survive: an agent region ringed by lower regions
(`void_beyond_a_ring_inherits_the_ring_not_the_agent`), and a tie going to the
lower level
(`a_tied_void_cell_takes_the_lower_level`). This case is a pure function of the
loaded-cell map, so it gets a unit test beside them: a 2x2 block with one
corner higher, asserting that no void cell on that corner's diagonal is raised.
