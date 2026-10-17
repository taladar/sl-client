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

## The countdown, from gridspec-estate (2026-10-17)

Both grids' restart notices now read into `Event::RegionRestart`, and the
viewer shows one countdown card for either (`sl-viewer-notices/src/
region_restart.rs`), in place of the notice's toast. What remains of the
reference's `region_restarting` floater: its landmark combo and Teleport
button, the `UISndRestart` sound (and OpenSim's `UISndRestartOpenSim`), the
screen shake, and the nearby-chat line Firestorm adds.

## The capability, from gridspec-region-info (2026-10-17)

The protocol half is done: `Command::RequestRegionSchedule` and
`Command::SetRegionSchedule { schedule }` (a write is read back) answer
`Event::RegionSchedule(Option<RegionSchedule>)`, both runtimes; the fake grid
serves it on `FakeSl`. Measured on aditi as a resident: an empty schedule reads
`undef`, a write is refused `403`. What remains is the floater and its button
(shown only where the capability is granted, as the reference's).
