---
id: gridspec-lsl-probe-corpus
title: A committed LSL probe corpus and per-grid result files
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

No probe script is committed; results live as prose in done files and doc
comments.

## Discover

Create `sl-lsl-runtime/probes/<area>/<probe>.lsl` printing machine-parsable
`PROBE <id> key=value` lines, and committed results per grid
(`aditi`, `opensim-yengine`, `opensim-xengine`, later `fake-sl`,
`fake-opensim`) stating date, runner, region and engine settings
(`MinTimerInterval`, `ScriptDelayFactor`, sensor limits). Port the existing
inline probes (`server-lsl-runtime-errors`, the event / timer probes).

## Document

`book/src/tools/lsl-probes.md` (writing probes: the 1/45 s frame, warm-up runs,
one state, the 64-event queue).

## Fake grid

Not applicable.

## Viewer

Not applicable.
