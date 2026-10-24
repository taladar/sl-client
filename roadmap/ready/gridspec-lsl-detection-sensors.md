---
id: gridspec-lsl-detection-sensors
title: LSL library behaviour on each grid — sensors and raycasts
topic: gridspec
status: ready
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
