---
id: test-e2e-environment-parcel-layer
title: End-to-end test of the parcel environment layer when walking between parcels
topic: test
status: blocked
origin: test-e2e-sweep-environment (2026-10-01)
points: 3
blocked_by: [server-fake-grid-parcel-on-movement]
refs: [test-e2e-sweep-environment, viewer-environment-personal-lighting]
---

Context: [context/automation.md](../context/automation.md).

The one check of [[test-e2e-sweep-environment]] left over: the parcel layer
of [[viewer-environment-personal-lighting]] — a parcel's own environment drawn
over the region's while the agent stands on it, dropped the moment the agent
steps off, and the local layer still winning over both.

On a fake-grid region of two parcels, one with an environment of its own
(a sky frame with a name the environment probe reports): log in on the plain
parcel, walk onto the other and read the parcel's sky, walk back and read the
region's; pin a preset and walk again, and the preset stays.

Two things are missing:

- the grid's side, [[server-fake-grid-parcel-on-movement]]: the parcel pushed
  as the agent walks onto it, and a parcel environment served;
- a way to walk: the driver plays a key as a press and a release in
  consecutive frames, which moves an avatar nowhere. A driver verb that holds
  a key (or a chord) down for a duration, through the synthetic input, is
  what walking needs — `press` with a hold, or a `hold` request.
