---
id: test-lsl-probes-corpus-order-leaks-between-probes
title: A full fake-grid probe corpus run changes two probes' results
topic: test
status: bugs
origin: server-lsl-lib-physics-vehicles (2026-11-16)
refs: [server-lsl-lib-physics-vehicles]
---

Context: [context/lsl.md](../context/lsl.md).

## Observation

`sl-conformance run --grid fake-sl|fake-opensim lsl-probes` runs every probe
in one fake region, against one anchor, so what an earlier probe leaves
behind reaches a later one. A full run on both fake flavours (2026-11-15)
gave two results that a run of the probe alone does not, and the committed
results are the probe-alone ones:

- **`events/collision` on fake-sl.** After `avatar/sit` or
  `avatar/sit-animation`, the primary avatar is left standing where the
  stand-up put it, against the probe's prim at the anchor. The probe prim
  then hears 237 `collision` events in the solid phase (11 alone), the
  faller 197 in the volume-detect phase (0) with a `collision_start` it should
  not have, and 300 in the asleep phase (0). The same happens at e48f8907,
  before the physics tranche, so it is not the tranche's. Aditi's committed
  result has 11, so either the harness puts the avatar somewhere else there,
  or a Second Life avatar standing still against a prim raises no `collision`
  every frame, as the fake grid's avatar box does.
- **`vm/region-budget` on fake-opensim.** Since b00bba6f charges YEngine its
  own speed, the one- and sixteen-worker stages match the local OpenSim
  (28.2 M and 3.59 M iterations a second against 28.5 M and 3.93 M), but the
  fake grid needs real CPU for every one of those instructions: time dilation
  falls to 0.1–0.3 and the probe does not reach the 256-worker stage before
  its 300 s deadline (`complete: no`). The committed result is still the one
  from before b00bba6f.

`ossl/errors`' two `Listed` answers and `memory/stack`'s set-limit
difference are not this: they are the same in a probe-alone run.

## To do

- Find where the harness leaves the avatar after a sit probe on each grid and
  whether a standing avatar's contact raises `collision` every frame on
  Second Life (an `events/collision` phase with the avatar against the
  prim); then fix the fake grid or the harness so a full run and a probe
  alone agree.
- Give `vm/region-budget` a deadline the fake grid can meet at YEngine's
  speed, or run its workers at the modelled speed without spending the
  instructions, and commit a complete fake-opensim result.
