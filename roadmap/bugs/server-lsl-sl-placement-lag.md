---
id: server-lsl-sl-placement-lag
title: Imitate Second Life's late bounding box and its stop above the ground
topic: server
status: bugs
origin: test-lsl-differential-opensim (2026-11-07)
refs: [test-lsl-differential-opensim, server-lsl-lib-prim-state]
---

Context: [context/lsl.md](../context/lsl.md).

Two things `prim/placement` measured on aditi that the runtime does not
imitate yet; `placement_moves_as_each_grid_moved` lists them as deviations
and `sl-lsl-runtime/probes/accepted.toml` as known defects of `fake-sl`:

- **The bounding box follows a scale late.** In the event that set a new
  scale, `llGetBoundingBox` gives the box of the scale before (`half`
  at once: `<-0.02, -0.5, -0.5>`–`<0.03, 0.5, 0.5>`, the box of the cube
  before it; `flat` at once: the half-metre cube's), and the new one in a
  later event. OpenSim gives the new box at once.
- **A move into the ground stops 4 cm above it.** `llSetPos` and
  `llSetRegionPos` 20 m down stopped the prim's centre 4 cm above what
  `llGround` gave (`ground_below=-2.2`, `moved=-2.16`); OpenSim lets the
  centre reach the ground.

Wanted: both on the Second Life flavour (the box from the prim's physics
shape as of the last frame, the stop measured against the ground's height
plus 4 cm — or whatever rule a second measurement at another scale shows,
since one size cannot tell a fixed 4 cm from a share of the prim's size).

Acceptance: the four deviations leave `placement_moves_as_each_grid_moved`,
and the `prim/placement` entries with this task leave `accepted.toml`.
