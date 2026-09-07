---
id: test-fake-grid-imitates-upload-announcements
title: An uploaded item is announced the legacy way whichever grid this is
topic: test
status: ready
origin: the honest residue of test-fake-grid-imitates-inventory-api (2026-09-07)
points: 2
refs: [test-fake-grid-imitates-inventory-api]
---

Context: [context/testing.md](../context/testing.md).

[[test-fake-grid-imitates-inventory-api]] gave the fake grid
`InventoryAnnouncement`, and a **take** follows it: the legacy UDP
`UpdateCreateInventoryItem` on an OpenSim-flavoured grid, a
`BulkUpdateInventory` over the event queue on a Second-Life-flavoured one.

The upload paths in `sl-fake-grid/src/uploads.rs` do not. A
`NewFileAgentInventory` completion, an in-place asset save
(`UpdateAgentItem`) and the legacy `UpdateInventoryItem` transaction all still
announce with the legacy message on both flavours.

That was left deliberately rather than derived, because it is **unmeasured**.
Each of those is the reply to a capability the client called, and the HTTP
response already carries the new item id and asset id — the reference viewer
builds the item from the response body rather than waiting for a push — so it
is genuinely unclear whether a Second Life simulator announces it again over
the event queue, announces it over UDP anyway, or says nothing at all.
Deriving a guess from the flavour would be inventing behaviour, which is the
one thing `ImitatedGrid` is not for.

So this is a measurement first and a derivation second, the same shape as
[[test-fake-grid-imitates-economy]]:

- On aditi, upload a notecard through `NewFileAgentInventory` and save over an
  existing item's asset, and record what arrives besides the HTTP response —
  `asset-upload` and `script-upload` already do the uploads, so what is missing
  is watching the event stream around them rather than new machinery.
- Then either derive the announcement from `ImitatedGrid` the way the take
  does, or write down that both grids answer these the same way and delete
  this item.

Acceptance: what Second Life sends after an upload completion is recorded, and
`uploads.rs` either follows the flavour or documents why it does not need to.
