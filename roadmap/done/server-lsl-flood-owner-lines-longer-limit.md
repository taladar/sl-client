---
id: server-lsl-flood-owner-lines-longer-limit
title: Second Life dropped prim/params' last complaints the flood gate passes
topic: server
status: done
origin: server-lsl-flood-gate-passes-spaced-lines (2026-11-05)
refs: [server-lsl-flood-gate-passes-spaced-lines, server-lsl-lib-prim-state]
---

Context: [context/lsl.md](../context/lsl.md).

## Observation

The flood gate counts an object's error lines over fifteen seconds and lets
a complaint told the owner alone through while fewer than nine passed in
that window (`vm::report::FloodGate`). That holds every timed probe:
`errors/flood`, `errors/flood-kinds`, `errors/flood-paths`, `prim/effects`.

`prim/params` (whose `try` records carry the script's clock since
2026-11-05) is the exception. Aditi passed its first thirteen complaints,
all told the owner alone, over about thirty seconds — twelve on the run of
2026-10-27 — and then dropped the last three, which came at 44.5 to 45.8
seconds, eighteen seconds after the thirteenth. No window short enough for
`errors/flood-paths`' `W` (a ninth line passed eighteen seconds after the
first of eight) drops them; and no slow refill passes `prim/effects`' fifteen
lines in under thirty seconds. The replay names the three lines a deviation.

The likeliest reading: the owner-only lines have a longer limit of their own
on top of the shared one — `prim/effects` carries a single owner-only line,
`prim/params` nothing else.

## What to find out

A probe that tells the owner twelve rule-list complaints spaced two seconds
apart, then one more every five seconds for a minute, then the same with
complaints said in the open: when the owner-only run stops, and whether the
open one does.

## Done when

The gate drops what aditi dropped of `prim/params`, and the deviation in
`params_round_trip_as_each_grid_stored_them` goes.

## Findings so far (2026-11-05)

Two new probes ran on aditi and both OpenSim engines:
`errors/flood-window` and `errors/flood-epoch`.

- `PRIM_PROJECTOR` is set without a word on Second Life, and only a get
  complains (`flood-window`'s `P`). So prim/params dropped one line, the
  projector's get, not three. The runtime now takes the set silently.
- Eight owner-only complaints in a row, then three more 16 seconds after the
  first: the ninth passes, and the tenth and eleventh are dropped. At 17, 18,
  19 and 20 seconds all three pass. So the owner-only line that reaches the
  limit passes and starts a silent flood.
- The same with missing sounds: at 16 seconds all three pass, but at 17 the
  ninth is the notice. No window that slides with the lines fits both this
  and the owner runs.
- One owner-only complaint every 1.5 seconds: ten pass, then nothing for 585
  seconds. The flood never ends while lines keep coming. One every 3.2
  seconds after a burst (`flood-window`'s `F`) passed again 33 to 36
  seconds after the flood began.
- A count the region clears at fixed times fits `flood-window` and
  `flood-epoch` (period about 23.7 or 25 seconds), but not `errors/flood-paths`'
  five `X` phases, which always flood on the ninth line. A sliding window
  fits those.

None of the models tried fits every run: a sliding window, a window anchored
at the first line, a leaky bucket, or a periodic count with a flood that
outlasts busy periods. The scripts' clock (`llGetTime`) may also drift from
the region's under time dilation, which would blur the 16 and 17 second
results.

`errors/flood-repeat` (2026-11-05) ran each 16 and 17 second phase twice:
the outcome near 16 to 17 seconds is not repeatable. Owner-only bursts at
16 seconds passed all three late lines both times, where `flood-window`'s
`O16` flooded; open lines at 16 seconds made the notice once and passed
once; at 17 seconds open lines passed once and made the tenth the notice
once; owner lines spread a second apart at 17 seconds passed all three
once and dropped the eleventh once. Read as a window that slides with the
lines, its length wanders between about 15.9 and 17.1 seconds from one
phase to the next, whatever the audience.

The flood's end wanders too. After twelve owner-only lines in a row, one
line every 2.2, 2.7 and 3.2 seconds passed again about 56, 36 and 49
seconds after the flood began (`flood-window`'s 3.2 seconds: about 36), and
one every 1.5 seconds never did.

`errors/flood-records` (2026-11-05) settled the last question: a script's
own `llOwnerSay` lines change nothing. Twelve complaints 1.5 seconds apart
passed nine, eleven, nine, eleven and ten across five phases, with records
between them or not, and two copies of prim/params' pace passed every line,
the late one included.

## Outcome

There is no separate limit for owner-only lines. Second Life's limit turns
about a second either way from one phase to the next, and prim/params' drop
and flood-epoch's ten passes are such turns. The gate now:

- counts over sixteen seconds instead of fifteen, which gets the fewest
  measured lines wrong;
- keeps a flood going past its 34 seconds until two seconds pass with no
  line (`FLOOD_GAP_SECONDS`), as flood-epoch's flood never ended;
- passes a `PRIM_PROJECTOR` set silently on Second Life.

The replays of the four new probes name the lines that fall in the turns,
and prim/params keeps one named deviation, its last line. The deviation
the task wanted gone stays, explained rather than removed: no count holds
every run.
