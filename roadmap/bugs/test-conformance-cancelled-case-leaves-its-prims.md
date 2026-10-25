---
id: test-conformance-cancelled-case-leaves-its-prims
title: A conformance case cancelled by its timeout or a panic leaves its rezzed prims in the region
topic: test
status: bugs
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
