---
id: test-fake-grid-imitates-inventory-api
title: The fake grid is OpenSim about inventory whichever grid it says it is
topic: test
status: ready
origin: auditing the divergences while doing test-fake-grid-object-asset-id-divergence (2026-09-07)
points: 5
refs:
  [test-fake-grid-object-asset-id-divergence, protocol-ais3-nested-embedded]
---

Context: [context/testing.md](../context/testing.md).

`ImitatedGrid` picks which live grid the fake one is, and inventory is the
loudest divergence it does **not** yet decide. Both halves are OpenSim's, on a
grid that says it is Second Life:

- **The fetch.** OpenSim still serves the deprecated UDP
  `FetchInventoryDescendents`; Second Life silently drops it and a viewer must
  use AIS3. The fake grid serves it on neither — `LegacyUdpInventory` picks
  between *refusing* it with a `FeatureDisabled` and *ignoring* it, which are
  the two roads a grid that does not serve it has. So a viewer that still
  reached for the UDP path would be failed here and would work on OpenSim,
  which is the wrong way round for a grid claiming to be OpenSim.
- **The announcement.** A take is answered with the legacy
  `UpdateCreateInventoryItem` on both flavours. Second Life moved inventory to
  AIS3 and delivers the new item as a `BulkUpdateInventory` over the event
  queue instead. `object-asset-format`'s take leg already waits for *either*,
  precisely because the two grids differ — the comment there records it.

Neither side is a flag away, which is why the flavour does not claim to decide
them: `SimSession` has **no sender for either message**. It has
`send_inventory_item_created` (the legacy one) and nothing else. So this is two
senders in `sl-proto` first, then the derivation:

- `SimSession::send_inventory_descendents`, answering a UDP
  `FetchInventoryDescendents` out of `SimInventoryTree` — which already has
  `descendents` for the AIS3 side to build on.
- `SimSession::enqueue_bulk_update_inventory`, the event-queue form, which is
  what a Second-Life-flavoured take should hand back.

Then `ImitatedGrid` grows the pair of them — how a folder is fetched, and how a
new item is announced — and `LegacyUdpInventory` stops being a builder knob a
caller has to reason about separately: an OpenSim-flavoured grid serves the UDP
fetch, a Second-Life-flavoured one drops it.

Worth doing because it is the divergence a *viewer* is most likely to trip
over: an inventory implementation that quietly depends on the legacy path
passes every offline case today.

Acceptance: `ImitatedGrid` decides both the fetch and the announcement; a
conformance case fetches a folder over UDP against `Grid::FakeOpensim` and is
refused against `Grid::FakeSl`, and a take is announced as a
`BulkUpdateInventory` on the one and an `UpdateCreateInventoryItem` on the
other.
