---
id: test-handover-distant-and-vehicle-aditi
title: Live-test distant teleport (world_reset) and vehicle corner crossings, incl. on aditi
topic: test
status: ready
refs: [protocol-teleport-deferred-teardown-handover, viewer-caps-event-queue-stops-after-teleport-spree]
---

Context: [context/test.md](../context/test.md).

The deferred-teardown handover, the region-handle adjacency world-reset
classification, and the single-worker CAPS event queue were validated on the
local OpenSim grid — but that grid's four regions are all **adjacent**, so two
paths remain unexercised live:

- **Distant teleport (`world_reset == true`).** Only a genuinely non-adjacent,
  non-child destination takes the world-reset branch (purge objects / terrain /
  neighbours, keep the own avatar). The local grid can never reach it. Needs a
  real multi-region grid — **aditi** — to confirm the scene resets correctly and
  the own avatar + attachments survive, and that a distant→distant sequence
  and a distant-then-adjacent sequence behave.
- **Vehicle corner crossings.** The "double crossing on a fast vehicle near a
  region corner" is unit-tested (`handover_corner_double_crossing`) but not
  driven live. Rez / ride a vehicle across the four-region corner (and on aditi)
  and confirm the single event-queue worker keeps re-targeting cleanly at
  sub-second cadence with no freeze.

Also re-confirm on aditi: the "Connecting..." region-name resolution
([[viewer-region-name-connecting-after-crossing]]) and the arrival-orientation
snap ([[viewer-arrival-orientation-snap]]), since both may behave differently
against Second Life's simulators than the local OpenSim.

Do **not** take known-bad behaviour to aditi — land the open follow-up fixes
first where they affect the SL path.
