---
id: gridspec-lsl-persistence
title: Script state across take, rez and region restart on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
blocked_by: [gridspec-lsl-live-differential-runner]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

Unmeasured on SL; OpenSim YEngine / XEngine keep state files, lost crossing
engines.

## Discover

Globals, state, timer, queue and pending sleep across take / rez; region restart
on OpenSim (and aditi if rights allow); `CHANGED_REGION_START`.

## Document

`book/src/gridspec/lsl.md` § Persistence.

## Fake grid

Large — [[server-lsl-script-persistence]].

## Viewer

Script recovery.

## From gridspec-lsl-events (2026-10-25)

`events/attach` measured the take and rez leg (`book/src/gridspec/lsl.md`
§ Events): the runner's `rerez` drive takes prim A into the owner's Trash
and rezzes it back. Second Life and YEngine keep the script's state — a new
key, `on_rez(0)` alone, the timer and a listen carry on — and **XEngine
resets the script**: `state_entry` runs, the globals are gone, then
`on_rez(0)`. The region restart (`CHANGED_REGION_START` and what runs first
after it) is left here: aditi needs estate rights for a restart and the
runner does not survive one.
