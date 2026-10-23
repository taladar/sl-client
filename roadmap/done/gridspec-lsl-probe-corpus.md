---
id: gridspec-lsl-probe-corpus
title: A committed LSL probe corpus and per-grid result files
topic: gridspec
status: done
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

## Done (2026-10-23)

- **Corpus**: nine probes in `sl-lsl-runtime/probes/` — `vm/throughput`,
  `time/get-time-frames`, `time/sleep-while-stopped`, `events/queue-limit`,
  `events/timer-no-stack`, `events/state-change`, `events/state-self`,
  `events/detected-cleared` and `errors/runtime-errors` (the six scripts of
  [[server-lsl-runtime-errors]], plus a second listener at 10 m). The event,
  timer and throughput probes behind the prose of [[server-lsl-vm-execution]]
  and [[server-lsl-state-and-events]] were never committed; they are
  rewritten from that prose.
- **Format**: `sl_lsl_runtime::probe` — the script header (`probe`, `drive`,
  `script`, `prim`, `order`, `distance`, `skip-on`), the `PROBE` line, the
  result file — and `tests/probe_corpus.rs`, which compiles every probe and
  holds every result file to the format.
- **Results**: all nine on `aditi`, `opensim-yengine` and `opensim-xengine`,
  every one complete. The reading is the table in
  `book/src/tools/lsl-probes.md`.
- **Scope beyond the task**: the results were taken by our own client, not by
  hand. The `lsl-probes` / `lsl-probes-xengine` conformance cases rez the
  prims, plant the scripts by `RezScript` + task upload under their names
  with the engine line, touch a `touch` probe, collect owner-say and
  `DEBUG_CHANNEL` lines, and write the result files. That is the planting,
  touching and collecting half of
  [[gridspec-lsl-live-differential-runner]]; what is left there is recorded
  on it. The `lsl_settings` fixture states OpenSim's engine settings in the
  headers.

Found on the way: unbounded recursion under XEngine overflows the simulator's
own stack and the OpenSim process aborts (systemd restarted it), so `Recurse`
is `skip-on: opensim-xengine`.
