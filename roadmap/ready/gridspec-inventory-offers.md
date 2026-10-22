---
id: gridspec-inventory-offers
title: Giving inventory, accepting and declining, and object gives on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-message-routing, gridspec-object-rez-derez,
  gridspec-inventory-mutations]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

OpenSim files the copy at offer time and relays `IM_INVENTORY_ACCEPTED`; SL
assigns the copy id on accept and relays nothing to the giver.

## Seen on the way (2026-10-22)

[[gridspec-inventory-mutations]] measured a no-copy give and give-back on
OpenSim (`book/src/gridspec/inventory.md` § Changing inventory, *Gives*): the
giver's copy is gone at offer time, the giver is told `InventoryAccepted`. On
aditi **the primary test account files no accepted gift at all**: B's offer
reaches it and its `AcceptInventoryOffer` is answered with nothing — no
`BulkUpdateInventory`, nothing in its Notecards or root after 30 s — for a
full-permission notecard as much as a no-copy one, while the same account's
gifts to the secondary are filed. `give-inventory` with its roles swapped
(`--avatar secondary --secondary primary`) fails the same way. Find out why —
the account (an inventory or offer setting), where it stands (the Mauve
sandbox, which already silences avatar sounds), or our accept — first by
accepting such a gift in Firestorm on that account; the census's `control_`
and `nocopy_` rows on Second Life wait on it.

## Discover

`give_inventory` plus decline, folder gives, offline recipients, object gives
(scripted prim), the SL inbox filing; the two-viewer live tests.

Once an object item has changed hands it is one its new owner may not copy
(a new prim's next-owner mask is move and transfer on both grids): rez it,
and restore it to its last position, and record whether the item goes with
the object. [[gridspec-object-rez-derez]] could only rez an item its owner
may copy, so the viewer's no-copy Restore guard
(`sl-viewer-inventory`, `restore_refused`) rests on the reference viewer's
word that Second Life can lose such an item.

## Document

`book/src/gridspec/inventory.md` § Giving.

## Fake grid

Large — [[server-fake-grid-inventory-offers]].

## Viewer

No dependence on the bucket id; no wait for a giver ack on SL.
