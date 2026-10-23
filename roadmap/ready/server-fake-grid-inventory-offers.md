---
id: server-fake-grid-inventory-offers
title: Fake grid — inventory offers, filing and object gives
topic: server
status: ready
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
