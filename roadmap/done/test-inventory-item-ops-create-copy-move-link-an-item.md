---
id: test-inventory-item-ops
title: create / copy / move / link an item
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 5 — Inventory (deep) `[both]`
---

Context: [context/test.md](../context/test.md).

`inventory-item-ops` — create / copy / move / link an item. `1av`. Where
`ais3-folder-lifecycle` proves the write side for *folders*, this proves
it for the *items* inside them. All four operations ride the UDP messages
on both grids (the reference viewer still creates, copies, moves, and
links items over UDP even where AIS3 exists — AIS3 carries folder
mutations and item *metadata* edits), so this is a single `[both]` path
with no per-grid branching. Create, copy, and link each draw a direct
reply that allocates the new item's server id: `CreateInventoryItem` /
`LinkInventoryItem` answer with an `UpdateCreateInventoryItem`
(`Event::InventoryItemCreated`), and a copy answers the same way on
OpenSim (its `CopyInventoryItem` routes through the same
`CreateNewInventoryItem` → `SendInventoryItemCreateUpdate` path) or as a
`BulkUpdateInventory` (`Event::InventoryBulkUpdate`) elsewhere, so the
copy predicate accepts either. The case captures the new id from that
reply, then — never trusting the optimistic local cache — re-fetches the
affected folder over `RequestFolderContents` and asserts against the
grid's authoritative `InventoryDescendents` item list, polling to absorb
OpenSim's fire-and-forget descendents worker. The lifecycle creates a
`src` and a `dst` folder under the agent root, then: **create** a notecard
in `src`; **copy** it to a second name in `src` (the original must survive
— a copy, not a move); **move** the original to `dst` (asserted on both
edges — present under `dst`, gone from `src`, copy untouched in `src`);
**link** to the moved original, filing the link in `src` (asserting the
link's target still lives in `dst` — a pointer, not a relocation). Created
items are deleted (item deletion is not Trash-gated) and both working
folders sent to Trash + removed at the end so re-runs start clean. Green
on OpenSim; create ≈ 0.1 s, copy ≈ 0.1 s, move ≈ 0.8 s, link ≈ 0.2 s, full
lifecycle ≈ 1.2 s loopback. `[both]`; the aditi run is deferred with the
batch (no aditi record produced this session). Required re-exporting
`NewInventoryLink` from both runtime crates (the only API addition).
