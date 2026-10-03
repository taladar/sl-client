---
id: gridspec-lsl-persistence
title: Script state across take, rez and region restart on each grid
topic: gridspec
status: blocked
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
