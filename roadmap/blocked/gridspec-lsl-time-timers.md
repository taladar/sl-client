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

## From gridspec-lsl-probe-corpus (2026-10-23)

Measured by the corpus (`time/*`, `events/timer-no-stack`):

- `llGetTime` moves in 1 ms steps on both OpenSim engines, against Second
  Life's 0.0219–0.0227 s frames.
- XEngine's timer floor is 0.5 s (no `MinTimerInterval` in its section);
  YEngine's is OpenSimDefaults' 0.1 s.
- A stopped script's sleep counts on while stopped on Second Life and
  YEngine alike. On XEngine, stopping a sleeping script ends that event:
  it never wakes. So the "YEngine differs" above is not borne out by
  `time/sleep-while-stopped`.
