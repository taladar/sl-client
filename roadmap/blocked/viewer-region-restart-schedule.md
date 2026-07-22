---
id: viewer-region-restart-schedule
title: Region restart schedule + restart countdown
topic: viewer
status: blocked
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
