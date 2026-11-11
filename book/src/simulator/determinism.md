# Determinism: what a script may depend on

`sl-fake-grid` is reproducible on purpose. Built from a seed
(`FakeGridBuilder::deterministic`), it mints the same identifiers in the same
order, and every instant it stamps comes from one injected clock, so two runs
of a scenario can be compared line by line ([the fake
grid](../tools/fake-grid.md#determinism-the-seed-and-the-clock)). A script
engine is the largest threat that property has: not one call to a clock, but a
dozen ordinary decisions — a timer, a random number, a loop over a map — that
each look local. So the rule is written down once, here, rather than decided
again by each library function:

> **A script's observable behaviour is a function of the seed, the scenario
> and the client's own input, and of nothing else.**

"Observable" means what a viewer is told — every `ChatFromSimulator` and every
object update, in order — and what a script can read back. The rest of this
chapter is what that takes, and where each part lives.

## The tick is the clock

A script's time is the region's ticks times the heartbeat's step, never wall
time. `llGetTime`, `llGetAndResetTime` and `llResetTime` count ticks;
`llSetTimerEvent` fires on a tick number; `llSleep` and a library function's
forced delay make an instance not runnable until a tick number. The engine
(`sl-lsl-runtime`'s `vm`) is handed the step once and never reads a clock at
all.

How ticks relate to the client's input is the one place time enters. A
**stepped** heartbeat (`HeartbeatConfig::stepped`, advanced by
`FakeGrid::advance_one_tick`) makes that a function of the test's own calls; a
free-running one ticks on tokio's timer, so a run that must be comparable
either steps the region or pauses tokio's clock (`tokio_clock`).

## The budget is counted in modelled time

Each tick, each runnable instance gets at most a fixed share, and the region a
fixed total ([the
engine](lsl-engine.md#3-scheduling-an-instruction-budget-round-robin-per-tick)).
The share is counted in the script time each operation is *charged* — what the
imitated grid was measured to take for it, from a table (`vm/cost.rs`) — never
in the time the machine took. A slice measured on the host's clock would make
how far a script gets — and so the order of what it says — depend on the
machine and its load. This is the one decision most likely to be made wrongly
by reflex.

## Randomness comes from the seed

Every random number a script sees comes from `Host::random_bits`, and the fake
grid answers it from a xorshift64 stream per region (`RegionScripts`), seeded
with the region's id. On a seeded grid the region's id is itself minted from
the seed, so the draws follow from it, and drawing them does not move the
grid's own identifier stream on. Three functions draw today:

- `llFrand`: 24 bits a draw, scaled and truncated toward zero.
- `llListRandomize`: one draw for each stride it places, a Fisher–Yates
  shuffle.
- `llGenerateKey`: four draws, 128 bits, shaped as each grid shapes them —
  Second Life's are 128 random bits with no digit fixed, OpenSim's a
  version-4 uuid (`data/generate-key`, [the
  numbers](../gridspec/lsl.md#numbers)).

## Calendar time comes from the grid's wall clock

The grid stamps calendar time into a login's `seconds_since_epoch`, the
region's time message, an instant message, a group notice and its accounts'
dates, a calling card's creation date and an access pass's expiry. All of
them read one `WallClock` (`sl_fake_grid::time`): the calendar instant the
grid started at, moved on by the grid's injected clock. It is not a second
clock — paused with the clock, skewed with it — and each region's world holds
a copy (`SceneFixtures::wall`).

A script's date is the one calendar time that must not follow the clock: like
every other time a script sees, it moves by the tick. The functions that tell
a script the date (`llGetUnixTime`, `llGetTimestamp`, `llGetDate`,
`llGetGMTclock`, `llGetWallclock`, the time-and-timers tranche) are to read
the region's wall clock where the region started, advanced by its ticks times
the step — so under a stepped heartbeat the date stands still between ticks,
as `llGetTime` does.

The anchor is the machine's time when the grid starts, unless
`FakeGridBuilder::epoch` names one. A seeded grid starts at
`SEEDED_EPOCH_SECONDS` (2023-11-14T22:13:20Z, the creation date it stamps a
minted item with), because a date read off the machine would be the one stamp
two runs never agreed on.

## Order is defined

Every walk whose order a viewer could see is over an ordered collection:

- **Scripts run** in the order of their `CallerId`s, round-robin from the
  instance after the one the last tick served last. The fake grid mints the ids
  as it brings scripts in line with the world — prims in the order the store
  holds them (`Seq`), then a prim's script items in their contents' order —
  and never reuses one, so a script started later is served later.
- **Listens** fire in the order they were registered (a `Vec` per instance).
- **A line** is offered to the scripts in earshot in `CallerId` order.
- **What the scripts did** in a tick — lines said, objects changed, linksets
  linked or removed — is applied in the order they did it, and goes to the
  region's sessions as one ordered list of changes, `llOwnerSay` among them.
  What the tick changed in the region's prims comes last, as one update burst
  (`sl_fake_grid::updates`): one entry per prim, terse where only its
  placement changed, the prims in the order the tick announced them and then
  in the store's order (`Seq`). So a viewer hears a tick's lines before it
  sees what the tick did to the prims, and sees a prim changed three times in
  a tick once. What goes to one viewer alone (a
  dialog, a permission question, an instant message, a page, a map
  destination, `llRegionSayTo` to an avatar) is sent after the tick, in the
  order it was made.

A `HashMap` walk in any of these would reorder two `llSay`s between runs.

## Nothing leaves the process

No library function reaches the network. `llHTTPRequest`, `llRequestURL`,
`llEmail` and the rest of the outbound and in-world HTTP family are not
implemented yet, and when they are (the HTTP tranche) they are off by default
and answer only through a responder a scenario injects.

## What is outside the rule

- **The client's own timing.** When a viewer's message arrives relative to the
  ticks is the client's (and, on a free-running heartbeat, the machine's).
  Two runs whose input arrives at the same ticks see the same answers.
- **UDP cadence.** A client's resends, pings and `AgentUpdate`s run on its own
  timers, so a transcript compares decoded messages, never datagrams.
- **Which session a line reaches first.** Each viewer's stream is ordered;
  two viewers' streams are not ordered against each other.

## The guards

- `sl-fake-grid/tests/determinism.rs`,
  `scripts_tell_a_client_the_same_twice_from_one_seed`: a region of two
  scripted boxes — a timer, `llFrand`, `llGenerateKey`, `llListRandomize`, an
  allow-drop flag flipped each tick, and one script talking to the other on a
  hidden channel — run for 24 stepped ticks with a real client logged in. On
  both flavours, two runs from one seed tell the client the same lines and the
  same object updates, in the same order, each compared as its whole decoded
  value; a run from another seed tells it other rolls in a stream of the same
  shape. It fails when the region's draws are seeded from the machine instead.
- In `sl-lsl-runtime`,
  `llfrand_draws_the_same_from_the_same_seed_and_stays_in_range`
  and `llgeneratekey_draws_the_same_keys_from_the_same_seed`, and the three
  `llgeneratekey_shapes_its_keys_as_*_did` replays of `data/generate-key`.
- `sl-fake-grid`'s `time` tests: the wall clock follows the grid's clock from
  its anchor.
