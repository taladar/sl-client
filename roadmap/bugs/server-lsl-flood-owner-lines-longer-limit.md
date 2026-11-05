---
id: server-lsl-flood-owner-lines-longer-limit
title: Second Life dropped prim/params' last complaints the flood gate passes
topic: server
status: bugs
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
