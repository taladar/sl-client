---
id: server-fake-grid-inventory-udp-mutations
title: Fake grid — the UDP inventory mutations, Trash gating and pushes
topic: server
status: blocked
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
blocked_by: [gridspec-inventory-mutations]
---

Context: [context/server.md](../context/server.md).

## What

About eight UDP inventory mutation messages are dropped. Decode and apply them,
gate purge / remove on Trash, push what each grid pushes, refuse what SL
refuses.

## How

Implement the behaviour the discovery task measured and the book's
`gridspec` chapter records, per `ImitatedGrid` flavour (a row in
`sl-fake-grid/src/imitates.rs` for every divergence), and hold it to the
measurement with the discovery task's conformance cases run against
`FakeSl` and `FakeOpensim`. See `roadmap/context/gridspec.md`.

## Since done (2026-10-21)

`gridspec-inventory-fetch` serves the four UDP folder messages —
`CreateInventoryFolder`, `UpdateInventoryFolder`, `MoveInventoryFolder`,
`RemoveInventoryFolder` — with the version steps both grids were measured
giving, per `InventoryDialect` (a removal steps the Trash on Second Life and
not on OpenSim), because the client now moves folders and items over UDP on
every grid: Second Life's AIS3 refuses a move (`book/src/gridspec/inventory.md`
§ Fetching). Still dropped: `CopyInventoryItem`, `ChangeInventoryItemFlags`,
`RemoveInventoryItem`, `RemoveInventoryObjects`, `PurgeInventoryDescendents`.
The `inventory-fetch` census's item legs need the copy (its `FAKE_EXEMPT`
rows name this task). Trash gating and pushes are unmeasured.
