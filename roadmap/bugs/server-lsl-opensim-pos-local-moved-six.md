---
id: server-lsl-opensim-pos-local-moved-six
title: OpenSim moved prim/params' PRIM_POS_LOCAL prim 6 m, where it moved it 10
topic: server
status: bugs
origin: server-lsl-flood-gate-passes-spaced-lines (2026-11-05)
refs: [server-lsl-lib-prim-state]
---

Context: [context/lsl.md](../context/lsl.md).

## Observation

`prim/params`' `pos-local-east-15` puts the prim back with
`llSetRegionPos(home)`, sets `PRIM_POS_LOCAL` to `home + <15, 0, 0>` and reads
how far it moved. Both OpenSim engines answered 10 on 2026-10-27, which is
what OpenSim's code says (`GetSetPosTarget` caps a root prim's move at 10 m
from where it is) and what the runtime does. On 2026-11-05 they answered 6 in
four runs of five (YEngine once 10) — as if the prim stood 4 m west of `home`
when the rule list was read. The probe was re-run only to add a clock to its
`try` records; nothing between those lines changed. The replay names the line
a deviation for both OpenSim flavours.

## What to find out

Where the prim is just before the call (an `llGetPos` after
`llSetRegionPos(home)`), and whether `llSetRegionPos` has settled by then on
OpenSim: a race between the two moves would answer 6 or 10 by timing.

## Done when

The runtime answers what OpenSim answers, for the reason OpenSim does, and the
deviation goes.
