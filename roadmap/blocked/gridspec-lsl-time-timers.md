---
id: gridspec-lsl-time-timers
title: LSL time, timers and sleep on each grid
topic: gridspec
status: blocked
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
blocked_by: [gridspec-lsl-live-differential-runner]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

Aditi: `llGetTime` in 1/45 s frames, timer due during sleep runs once, restart
mid-sleep semantics; YEngine differs; OpenSim clamps timers to 0.5 s.

## Discover

Minimum timer interval on SL, drift / phase, `llSetTimerEvent` re-arming,
timestamp formats, XEngine restart mid-sleep.

## Document

`book/src/gridspec/lsl.md` § Time.

## Fake grid

Large — [[server-lsl-lib-time-timers]] (its 'a state change cancels the timer'
acceptance contradicts aditi; fix it).

## Viewer

None.
