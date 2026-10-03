---
id: gridspec-lsl-live-differential-runner
title: Run one probe on OpenSim YEngine, XEngine and aditi and collect the transcripts
topic: gridspec
status: blocked
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, test-lsl-differential-opensim, repl-lsl-script-control]
blocked_by: [gridspec-lsl-probe-corpus]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

Planting works on OpenSim; a `//YEngine:` / `//XEngine:` first line picks the
engine per script; [[test-lsl-differential-opensim]] is blocked only for its
fake-grid leg.

## Discover

The live half of the differential runner: plant a probe (OpenSim directly,
aditi via [[gridspec-lsl-aditi-script-carrier]]), drive it (touch, chat,
time) through the automation / `sl-client-tokio`, collect `ChatFromSimulator`
lines into the result files; a tolerance model (ids, timestamps, last float
digit, merging / ordering). Unblock the live verbs of
[[repl-lsl-script-control]] as its front end.

## Document

`book/src/tools/lsl-probes.md` § Running.

## Fake grid

The fake-grid leg stays [[test-lsl-differential-opensim]].

## Viewer

Not applicable.
