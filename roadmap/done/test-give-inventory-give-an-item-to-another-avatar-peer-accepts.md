---
id: test-give-inventory
title: give an item to another avatar; peer accepts
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 5 — Inventory (deep) `[both]`
---

Context: [context/test.md](../context/test.md).

`give-inventory` — give an item to another avatar; peer accepts.
`2av` (OpenSim now; Aditi deferred → Phase Z). The cross-avatar hand-off:
where `inventory-item-ops` proves a single avatar manipulates its *own*
items, this proves one avatar gives an item to another. A give is an
`ImprovedInstantMessage` with the `IM_INVENTORY_OFFERED` dialog whose binary
bucket carries the offered asset's type byte and id, routed by the grid's IM
service to the named recipient (not broadcast like local chat); the recipient
decodes the offer and replies `IM_INVENTORY_ACCEPTED`, which the grid relays
back to the giver. Sequence: the primary creates a transferable notecard in
its own Notecards folder, `GiveInventory`s it to the secondary with a fresh
correlation id, the secondary observes the matching `Event::InstantMessage
Received` (`InventoryOffered`, attributed to the primary) and decodes the
`InventoryOffer`, then `AcceptInventoryOffer`s filing it into its Notecards
folder; the primary observes the matching `InventoryAccepted` IM (the
round-trip confirmation `calling-card`'s no-op accept lacks), and the case
re-fetches the recipient's Notecards folder and asserts the item's copy is
present — never trusting the optimistic cache. OpenSim's `InventoryTransfer
Module` files a *copy* into the recipient at offer time (in the default folder
for the asset type) and rewrites the offer bucket to carry the new copy's id,
so the decoded offer's `item_id` is the recipient's copy, not the giver's
original; the original keeps its Copy permission so the grid leaves it behind.
Green on OpenSim; offer RTT ≈ 7.9 ms, accept RTT ≈ 0.6 ms loopback.
`[opensim]` only.
