---
id: test-ais3-folder-lifecycle
title: create / rename / move / remove / purge a folder (CAPS AIS3 on SL; gat
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 5 — Inventory (deep) `[both]`
---

Context: [context/test.md](../context/test.md).

`ais3-folder-lifecycle` — create / rename / move / remove / purge a
folder (CAPS AIS3 on SL; gate vs UDP on OpenSim). `1av`. Where
`inventory-tree-crawl` proves the *read* side, this proves the *write* side:
every structural folder mutation a viewer performs, gated on the per-grid
path — Second Life carries them over the modern **AIS3** (`InventoryAPIv3`)
CAPS REST endpoint (`Ais3CreateFolder`/`Ais3RenameFolder`/`Ais3MoveFolder`/
`Ais3PurgeFolder`/`Ais3RemoveFolder`), OpenSim over the legacy UDP messages
(`CreateInventoryFolder`/`UpdateInventoryFolder`/`MoveInventoryFolder`/
`PurgeInventoryDescendents`/`RemoveInventoryFolder`). The UDP mutations are
fire-and-forget (OpenSim sends no reply, the client caches optimistically),
so the case never trusts that optimistic cache: after every step it re-fetches
the affected parent over `RequestFolderContents` and asserts against the
grid's authoritative `InventoryDescendents` reply, polling to absorb OpenSim's
fire-and-forget descendents/purge workers. The lifecycle creates a destination
and a subject under the agent root, renames the subject, gives it a child (so
purge has something to empty), moves it under the destination (re-parent
asserted on both edges — present under the new parent, gone from the old),
then sends it to **Trash** before purging and removing. That Trash step is not
incidental: both grids only let a folder be purged or deleted once it lives
under Trash (the viewer's delete = move-to-trash-then-empty flow; OpenSim
enforces it with an `onlyIfTrash` guard in `XInventoryService`, so a purge or
remove of a folder outside Trash is silently a no-op — the bug the first run
surfaced). Purge then empties the subject (child gone, subject survives) and
remove deletes it; the destination is sent to Trash and removed at the end so
re-runs start clean. Green on OpenSim via the `udp` path; create ≈ 0.1 s,
rename ≈ 0.1 s, move ≈ 0.8 s, purge ≈ 0.8 s, remove ≈ 0.7 s, full lifecycle
≈ 2.9 s loopback. `[both]`; the AIS3 (`aditi`) path is written but not yet
run live (no aditi record produced this session).
