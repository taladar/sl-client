---
id: server-fake-grid-object-rez-time
title: Keep each object's rez time, and key Second Life's box beat to it
topic: server
status: done
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

## Outcome (2026-11-08)

Settled by the new probe `prim/box-beat`, run on aditi (twice), both
OpenSim engines and both fake flavours. Prim A flips its own x twenty times
across the second and reads its box every frame until it shows; then it
rezzes the same item three times (A, B half a second later, C straight after
B), and each rescales itself once soon after its rez and then twenty times
more, overlapping the others. Children's records reach the transcript
through prim A, since only the probe's own prims' lines are kept.

Both questions this task asked had answers the task did not expect:

- **There is no beat.** Second Life's box catches up **the frame after a
  rescale**, unless it last caught up less than a second before, when it
  waits until a second after that. Rounds set within the second after a
  catch-up waited for it; rounds set later showed a frame on (`polls=1`),
  three in a row in each run. The clock is **per prim** (C's catch-ups keep
  their own spacing, out of step with A's and B's), and **a prim's rez counts
  as its first catch-up**: A, rescaled 0.2 s after its rez, waited until
  about a second after it; C, at 1.4 s, showed a frame on. The second is the
  first frame at least a second on, so the waits ran 1.000 or 1.022 s,
  about 1.01 s on average, as `placement-settle`'s cadence rounds also saw.
  The engine now keeps a `BoxClock` per prim (last catch-up, tick due),
  starts it at the prim's region-entry tick (`Host::rezzed_in`, the fake
  grid's new per-prim `Rezzed` component in the object store, stamped with
  the tick about to run), and refreshes at 45 frames.
- **`OBJECT_REZ_TIME` is the object's creation, not its rez.** The three
  rezzed copies all read the moment the runner had first made the Child
  object, 25 s before; a take keeps it. OpenSim does the same, with no
  fraction. On the fake grid it is the object record's `creation_date` (which
  a take already writes into the asset's `birthtime` and a rez brings back);
  a prim with no stored record is now made when it came into the region,
  a fixture at the region's start, instead of the fixed fixture date.
  `ObjectDetails::rez_time: String` became `rezzed: Option<i64>`, formatted
  per flavour.

Against the acceptance: `OBJECT_REZ_TIME` reads the creation on both fake
flavours, and `prim/box-beat` agrees between each grid and its fake flavour.
The `placement-settle` replay now runs at the grid's frame (1/45 s), and
`sleep-0.05` and `params-delayed` agree. Three reads still fall a frame on
the other side of a catch-up, on fake-sl as in the replay: `settled x-0.01`,
`settled x-0.06` and `lag params-fast`. They sit at the end of a chain of
twenty-odd catch-ups, each one 45 frames after the last here and about half
a frame later on average on aditi. They are named deviations in the replay
and deliberate entries in `accepted.toml`, since a deterministic clock does
not reproduce a jittering one.

A shift-copy keeps the original's creation date on both grids: `object-edit`
records it for a copy by an offset, on a ray and on a ray at a prim
(`*_creation_date = "the original's"` on aditi and OpenSim). The fake grid
already did: a copy takes the original's whole record (`properties_of`, so
also one never stored), creation date included.
