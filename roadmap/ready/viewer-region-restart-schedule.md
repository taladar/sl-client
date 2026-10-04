---
id: viewer-region-restart-schedule
title: Region restart schedule + restart countdown
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-region-options-general]
---

Context: [context/viewer.md](../context/viewer.md).

Two small restart-related surfaces:

- **Restart schedule** (estate managers): view / set the region's scheduled
  weekly restart window over the `RegionSchedule` capability (SL-only cap —
  verify presence via the caps map and degrade gracefully); lives beside
  the restart action [[viewer-region-options-general]] owns.
- **Restart countdown**: the full-screen-adjacent countdown floater every
  resident sees when a restart is scheduled (`RegionRestart` event-queue
  message with seconds remaining), with the reference's escalating urgency
  styling; on cancel (`RegionRestartCancelled`? — verify event name in the
  EQ batches) it closes.

Reference (Firestorm, read-only): `llfloaterregionrestart`,
`floater_region_restart_schedule.xml`.

Deps: [[viewer-region-options-general]] (floater placement + estate
gating).

## Capability (triage 2026-10-04)

`RegionSchedule` (Second Life only): GET → `{restart:{type:"W"|"D",
days:"MTWRFSU subset", time:<seconds after midnight>}}` (no `restart` = none);
POST the same shape, empty `days` with type `W` resetting it. Firestorm shows
the button only with the capability.

Shapes and Firestorm references: `book/src/comms/caps-reference.md`; which grid
grants it: `book/src/gridspec/capabilities.md`.
