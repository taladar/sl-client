---
id: server-fake-grid-object-rez-time
title: Keep each object's rez time, and key Second Life's box beat to it
topic: server
status: ready
origin: server-lsl-sl-placement-lag (2026-11-08)
refs: [server-lsl-sl-placement-lag, server-lsl-lib-prim-state]
---

Context: [context/lsl.md](../context/lsl.md).

The fake grid keeps no rez time for an object: `ObjectDetails::rez_time`
is empty, so `OBJECT_REZ_TIME` (`llGetObjectDetails`) reads `""` on both
flavours, where Second Life and OpenSim give the moment the object was
rezzed as `llGetTimestamp` writes it (`prim/placement-settle`'s `home`
record: `rez=2026-11-08T13:52:23.319659Z` on aditi, OpenSim without the
fraction).

The same moment seems to decide when Second Life's `llGetBoundingBox`
catches up with a new scale. `server-lsl-sl-placement-lag` measured a beat
about a second apart at which every rescale since the last beat shows at
once. On the one run that printed the rez time, beats counted a whole
second apart from the rez fit every box read of the first 20 s (the
`settled` reads, half a second apart, alternate between a fresh box and a
stale one); 30 s on, the `cadence` records saw beats about 1.006 s apart
(1.009, 1.010 and 1.013 s across three runs), a frame before whole seconds
of the script's clock. The runtime keeps a beat of exactly one second of
**region** time (`SECOND_LIFE_BOX_REFRESH` in
`sl-lsl-runtime/src/vm/engine.rs`), the same for every prim. The replay in
`vm/tests/prim_state.rs` starts the script as long after the rez as the
runner did and still names three `lag` reads as deviations, and
`accepted.toml` holds fake-sl's `lag params-fast` against this task.

Wanted:

- every object the fake grid rezzes (fixtures, viewer rezzes, rez from
  inventory, `llRezObject`, a re-rez of a take) keeps its rez time, read
  back by `OBJECT_REZ_TIME` in each flavour's format;
- the Second Life flavour's box beat counted from each object's own rez
  time, so a stale box is one rescaled since that object's last beat.

Settle first, by one more aditi run of `prim/placement-settle` per idea,
whether the phase is the object's rez or something else (two prims rezzed
half a second apart, each rescaled and polled in the same frames, tell it
apart), and whether the period is a second of wall time or 45 of the
region's frames (the 1.01 s says frames run a little long).

Acceptance: `OBJECT_REZ_TIME` gives the rez time on both fake flavours;
the box replay in `vm/tests/prim_state.rs` holds with no deviation, and
the fake-sl `lag params-fast` entry in `accepted.toml` is gone.
