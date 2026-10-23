---
id: server-fake-grid-inventory-offers
title: Fake grid — inventory offers, filing and object gives
topic: server
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
blocked_by: [gridspec-inventory-offers, server-fake-grid-im-relay]
---

Context: [context/server.md](../context/server.md).

## What

As measured by `inventory-offers` (`book/src/gridspec/inventory.md` §
Giving), per flavour:

- an agent's gift filed **at offer time** on both flavours, in the
  recipient's folder of its type, the offer's bucket naming the copy; the
  offer's id the giver's transaction (Second Life) or the copy's id
  (OpenSim); OpenSim announces the copy with a `BulkUpdateInventory` first;
- accept and decline: nothing on Second Life; on OpenSim the giver is sent
  `InventoryAccepted` / `InventoryDeclined` and a declined copy is moved to
  the Trash and announced;
- an offline recipient: the copy filed at once; OpenSim alerts the giver and
  keeps the offer for the next offline fetch, Second Life keeps none;
- object gives (`TaskInventoryOffered`, a one-byte bucket): filed on the
  accept in the named folder (Second Life) or at offer time (OpenSim);
  `llGiveInventoryList` a folder under the root;
- a no-copy object's `RezRestoreToWorld` (Second Life: rezzes and removes the
  item, as a rez; OpenSim: nothing).

Then add `inventory-offers` to the offline cases for both flavours and drop
the `control_` / `nocopy_` / `reverse_` exemptions of `inventory-mutations`.

## How

Implement the behaviour the discovery task measured and the book's
`gridspec` chapter records, per `ImitatedGrid` flavour (a row in
`sl-fake-grid/src/imitates.rs` for every divergence), and hold it to the
measurement with the discovery task's conformance cases run against
`FakeSl` and `FakeOpensim`. See `roadmap/context/gridspec.md`.

## Done (2026-10-23)

`sl-fake-grid/src/gifts.rs` hands an agent's gift over per flavour
(`ImitatedGrid::gift_policy`): the copy filed at the offer in the
recipient's folder of its type (a folder under the root), the next owner's
masks applied, offered under the giver's transaction (Second Life) or the
copy's id (OpenSim); OpenSim announces the copy with a UDP
`BulkUpdateInventory` (a new `SimSession::send_bulk_update_inventory`),
Second Life only a folder's, over the event queue; on OpenSim an answer is
passed on to the giver and a declined copy moved to the Trash and announced. A
gift to an avatar that is away is held by the grid and filed at its next login;
OpenSim alerts the giver and keeps the offer. A giver's no-copy original
leaves its inventory (`inventory-mutations`' `nocopy_` give on OpenSim). An
object gift carries its taken linkset along, handed over, so the recipient can
rez it; the Second Life flavour's `RezRestoreToWorld` of a no-copy item takes
it away with an event-queue `RemoveInventoryObjects`
(`SimSession::enqueue_remove_inventory_objects`).

`inventory-offers` runs offline on both flavours; `inventory-mutations`'
exemptions are gone. The stock folders' per-agent ids no longer collide
between two agents (`own_stock_folders` turns the agent id before mixing it
in): one agent's root came out as the other's Clothing.

Not done: **object gives** (`TaskInventoryOffered`, `llGiveInventoryList`) —
the fake grid runs no scripts, so the census skips that leg on a fake grid
and exempts `object_`; noted on [[test-fake-grid-lsl-offline-cases]]. The
client ignoring the event-queue `RemoveInventoryObjects` is
[[protocol-remove-inventory-objects-over-event-queue-unread]].
