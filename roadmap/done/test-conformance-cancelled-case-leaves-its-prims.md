---
id: test-conformance-cancelled-case-leaves-its-prims
title: A conformance case cancelled by its timeout or a panic leaves its rezzed prims in the region
topic: test
status: done
origin: gridspec-lsl-detection-sensors (2026-10-25)
refs: [gridspec-lsl-detection-sensors, gridspec-lsl-compile]
---

Context: [context/test.md](../context/test.md).

## Observation

The first `detection/sensor` run on the local OpenSim (2026-10-25) found
three prims of the test avatar's at exactly prim A's place — the build
location every rezzing case uses — each holding a script (`Probe`, and two
`Compile`). Their `CreationDate`s put them at two runs of `script-compile`
on 2026-10-24 and one probe run before that: runs that ended without their
derez. They sat in every sweep and every ray of the new probes until they
were deleted by id (`delete object id …`, REST console).

## Cause

`isolate::run_isolated` wraps a case body in `tokio::time::timeout` and
`CatchUnwind`. A body that runs out of its overall timeout, or panics, is
dropped where it stands, so the derez that `script-compile`'s
`backend_legs`, `lsl_probes`'s `clear_away` and every other rezzing case
does on its way out never runs. A failed leg (`Err`) is cleaned up; a
cancelled one is not. On aditi the prims are sandbox debris until
auto-return; on OpenSim they stay for good and skew every later
measurement at the build location.

## Fix

Record what a case rezzes outside its future (on the `TestContext`, by
scoped id) and derez whatever is still recorded after `run_isolated`
returns, whatever it returned — before the sessions log out. A unit test:
a body that rezzes on the fake grid and then sleeps past its timeout
leaves no object behind.

## Resolution (2026-11-19)

The record lives on each `Session` rather than the `TestContext`, so it
survives a relogin and covers the secondary and tertiary too, and it is
kept by key (with the current scoped id beside it, cleared when the object
leaves the scene) because a scoped id dies with its circuit. Cases do not
record anything themselves: `Session::send` notes every command that asks
for a new object, and the session's event forwarder — which outlives a
cancelled body — matches each new root object of the agent's own
(`OBJECT_YOU_OWNER`) to a pending command aimed within 32 m of it (global
metres) in the last 60 s. A kill or an ownership change drops it.
`isolate::run_and_clear`, used by both the runner and the offline harness,
then derezzes whatever is still standing into the Trash (`ObjectDelete` is
ignored by OpenSim) and waits for the kills, logging a passing case's
leftovers as a warning and anything still standing as an error; the case's
outcome is unchanged. Objects a script rezzes are not counted.

Tests: matching (nearest aimed rez, unaimed fallback, out of reach, across a
border), one pending rez per object asked for, and on the fake grid a body
that rezzes a cube and then hangs past its timeout (fake SL) or panics (fake
OpenSim, which ignores `ObjectDelete`) leaves no object; both fail with the
clearing disabled. Live: `script-compile --timeout 25` on OpenSim and
`--timeout 45` on aditi were cancelled with a prim standing, and each prim
was derezzed and killed before the logout (the OpenSim region's prim count
unchanged); a full `object-rez-derez` pass on OpenSim left nothing to clear.
