---
id: server-lsl-sl-placement-lag
title: Imitate Second Life's late bounding box and its stop above the ground
topic: server
status: done
origin: test-lsl-differential-opensim (2026-11-07)
refs: [test-lsl-differential-opensim, server-lsl-lib-prim-state,
  server-fake-grid-object-rez-time]
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

## Outcome (2026-11-08)

A new probe, `prim/placement-settle`, measured both finely enough to tell
the rules apart, four times on aditi and on both OpenSim engines:

- **There is no stop above the ground.** Every move into the ground —
  prims of 0.01 to 8 m, flat, tall, lying, tilted, between the terrain's
  samples, straight after a rescale, by `llSetPos` and `llSetRegionPos` —
  left the centre exactly on what `llGround` gave, as on OpenSim. The
  "4 cm" was `prim/placement` printing the ground to the decimetre
  (`-2.2`) and the move to the centimetre (`-2.16`) over a ground 2.165 m
  down. `home_of` in `vm/tests/prim_state.rs` now has aditi's ground to the
  millimetre, the replay rigs `llGround` to three decimals, and both
  deviations are gone; the two `accepted.toml` entries stay, re-worded,
  because the fake region's terrain is not aditi's.
- **The box follows a scale at a beat a second apart**, not at the next
  event: a scale set anywhere in the second shows at the next beat and not
  sooner — not after a frame's sleep, `llSetLinkPrimitiveParamsFast`, or a
  link message to itself; a second rescale before the beat leaves the
  first one's box. The runtime now keeps, on the Second Life flavour, the
  scale each prim had at the last beat (`Raised::Rescaled`, the engine's
  `stale_boxes`, `ScriptCtx::box_scale`), refreshed each second of region
  time.
- **A box has a least extent of 0.05 m** per axis on Second Life (a prim
  0.01, 0.02 or 0.04 m thin measures 0.05; 0.1 m its own); OpenSim has
  none. `llGetBoundingBox` imitates it.

Left over, in `server-fake-grid-object-rez-time`: the beat's phase and
period. Beats counted a whole second apart from the object's rez fit every
box read in the first 20 s of aditi's run; 30 s on, the beats came about
1.006 s apart, a frame before whole seconds of the script's clock. The fake
grid keeps no rez time, so the runtime counts the beat from whole seconds of
region time. The replay of `prim/placement-settle` models the runner's
rez-to-save delay and holds but for three `lag` reads about 20 s in, where
the runtime's beat has fallen a little before aditi's; `accepted.toml`
holds the one such read on fake-sl against that task.
