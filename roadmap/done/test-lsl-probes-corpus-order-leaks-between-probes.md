---
id: test-lsl-probes-corpus-order-leaks-between-probes
title: A full fake-grid probe corpus run changes two probes' results
topic: test
status: done
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

## Outcome (2026-11-20)

- **The harness left the avatar where the stand put it.** On every grid the
  owner stands about 4 m from the anchor the probes' prims rez at (fake grid:
  arrival 128,128 against the anchor 128,124, just inside the build walk's
  4 m arrival radius), and the stand that ends `avatar/sit` puts it right
  beside prim A, where the next probe's prim A stands. The runner only walked
  an avatar back after a script's in-region teleport, and only to within 4 m of
  the build location. Now each session's event forwarder keeps where its own
  avatar was last seen (`sl_conformance::whereabouts`,
  `Session::whereabouts`, standing or seated), and after every probe the runner
  stands up an avatar still seated and walks one more than a metre off back
  onto the spot it held before the first probe (`support::return_to`: walking
  within 32 m, flying first beyond, then waiting for it to come to rest and
  steering again; on aditi an avatar out of a flight coasted 5–10 m past the
  spot). `avatar/sit` then `events/collision` gives 11/0/0 on fake-sl, as the
  probe alone and aditi do, and on aditi both avatars walked back on the ground
  in 5 and 9 s and stayed.
- **A standing avatar does keep colliding on Second Life, but not every
  frame.** The new `events/collision-standing` probe slides prim A, made a
  platform, under the owner's feet for 8 s: aditi one `collision_start` and
  59 `collision` (6–8 a second), the local OpenSim 145 under YEngine (17–19 a
  second: a scripted prim's actor reports at most every 50 ms on ubODE's
  20 ms step) and 104 under XEngine, where the fake grid raised one every tick
  (45 and 110 a second). The fake grid now paces an avatar's ongoing contact by
  a per-flavour `PhysicsPolicy::agent_collision_interval` (133 ms, 50 ms) and
  gives each prim one event per kind per tick detecting everything it touched
  (an unfiltered run showed one `collision_start` with both avatars on all
  three engines; fake-opensim had split it in two). Both fake flavours now
  match: 59 and 146.
- **`vm/region-budget` on fake-opensim** spends real CPU on every one of the
  some 1.9 billion iterations at YEngine's speed: a run took 418 s. Its
  deadline is 720 s, as the call-cost probes', and the committed fake-opensim
  result is complete and agrees with the local OpenSim's.
- Not modelled: when the platform moves away, all three live engines raise one
  last `collision` before the `collision_end`; the fake grid ends the contact
  at once (a measured field, so the comparison tolerates it).
