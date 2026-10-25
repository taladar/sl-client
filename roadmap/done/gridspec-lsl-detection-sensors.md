---
id: gridspec-lsl-detection-sensors
title: LSL library behaviour on each grid — sensors and raycasts
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-lsl-lib-detection-sensors]
blocked_by: [gridspec-lsl-live-differential-runner]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

No aditi or OpenSim measurements for this tranche beyond the library table's
documented values.

## Discover

Probes for: Sensor ordering and cap, `llCastRay` throttle and result shapes,
touch UV and face. Run on OpenSim YEngine / XEngine and aditi.

## Document

`book/src/gridspec/lsl.md` § Sensors and raycasts.

## Fake grid

Large — [[server-lsl-lib-detection-sensors]].

## Viewer

None.

## From gridspec-lsl-live-differential-runner (2026-10-24)

A probe can now ask for a line of chat on a channel or a touch, from the
owner or a second avatar (`rec=waiting drive=chat|touch by=other`;
`book/src/tools/lsl-probes.md`). `events/other-avatar` measured on all three
engines: an open listener on 7 hears the owner and the other avatar alike
(the name is `llKey2Name`'s), a listener filtered to the owner does not hear
the other, and the other's touch reads `llDetectedType` 3 on Second Life and
1 on OpenSim; `llDetectedGroup` is the object's group against the toucher's
active group (both unset on aditi: 1).

## Done (2026-10-25)

- **Discover.** Three probes, `sl-lsl-runtime/probes/detection/`:
  `sensor` (twenty-four sweeps over a row of scripted and bare targets east
  of and above prim A — the cap, the order, every mask, name and key
  filters, the range and arc edges, a target 100 m up — then the repeating
  sensor: its pace, `no_sensor` repeats, `llSensorRemove`, intervals of
  0.01 and 0, and an `llSensor` during a repeat), `cast-ray` (every
  option, the data flags' layout, rays from inside a prim, down to land,
  of length 0, out of the region, bad options, at the owner, and 500 in a
  row) and `touch-surface` (five touches with chosen surface blocks, none
  among them, one from the other avatar). All complete on aditi (the
  sensor three times), YEngine and XEngine; the two engines agree.
- **Runner.** A `bare` header key for target prims that hold no script,
  and a touch drive that carries a surface block (`face uv st at normal
  binormal`), noted as sent. The first OpenSim run found three prims left
  at the build location by cancelled runs; they were deleted, and the
  cause is [[test-conformance-cancelled-case-leaves-its-prims]].
- **Document.** `book/src/gridspec/lsl.md` § Sensors, raycasts and
  touches; the drive and the header key in `book/src/tools/lsl-probes.md`.
  The striking rows: Second Life returns 32, OpenSim 16; on Second Life
  `SCRIPTED` narrows a mask, an idle scripted prim drifts between the
  active and passive answers, a range of 0 raises no event and an
  `llSensor` ends a running repeat; OpenSim's default `llCastRay` returns
  positions as strings and land past the ray's end, and a malformed option
  list stops the script. Neither grid checks a touch's surface block.
- **Fake grid.** Large, as planned: [[server-lsl-lib-detection-sensors]]
  carries the measured rows a flavour splits on.
- **Viewer.** None needed: nothing of a sweep or a ray reaches it, and the
  surface block a script reads is the viewer's own pick, which
  `world_test`'s `a_world_touch_states_its_position_in_the_region` holds.
