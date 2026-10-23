---
id: gridspec-lsl-live-differential-runner
title: Run one probe on OpenSim YEngine, XEngine and aditi and collect the transcripts
topic: gridspec
status: ready
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
aditi through the same `RezScript` / `UpdateScriptTask` at the fixtures'
`build_location`), drive it (touch, chat,
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

## From gridspec-lsl-probe-corpus (2026-10-23)

Half of this exists. `sl-conformance run --grid aditi|opensim lsl-probes`
(and `lsl-probes-xengine`) runs the whole corpus in one login. It rezzes a
cube per prim, plants each script by `RezScript` + `UpdateScriptTask` under
its header name, with `//YEngine:` / `//XEngine:` on OpenSim, and touches a
`drive: touch` probe once. It keeps owner-say `PROBE` lines and
`DEBUG_CHANNEL` text from the probe's prims, and writes
`probes/results/<grid>/<id>.txt` (`sl-conformance/src/cases/lsl_probes.rs`).
Planting on aditi at the build location works.

Left here: drives other than one touch (chat on a channel, waiting N
seconds, a second avatar), choosing one probe to run, the tolerance model
and the comparison across result files, the fake flavours' results once
[[server-fake-grid-script-engine-wiring]] lands, and the
[[repl-lsl-script-control]] front end. Keep one rule from the first runs:
every wait inside a probe's run must feed the transcript. A `wait_for` that
drops what it rejects lost a listener's first line, and a record printed
during the compile wait was then waited for in vain.
