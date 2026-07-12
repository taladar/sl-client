---
id: test-inventory-tree-crawl
title: background/full-tree fetch beyond the root; record folder/item totals
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 5 — Inventory (deep) `[both]`
---

Context: [context/test.md](../context/test.md).

`inventory-tree-crawl` — background/full-tree fetch beyond the root;
record folder/item totals. `1av`. Where `inventory-fetch` proves the single
root folder answers, this proves the recursive descent into the whole tree.
The case crawls breadth-first from the agent root, issuing a
`RequestFolderContents` per folder and following every sub-folder reported in
the `InventoryDescendents` reply (deduping folders and items by id, bounded by
a `MAX_FOLDERS` safety cap), until the queue drains. It is the same per-folder
fetch the client's automatic background crawl issues, here pumped explicitly
by the test so completion is deterministic (the background-crawl flag is a
client-construction option, not a `Command` the harness can drive); the
library still routes each fetch to the modern CAPS
`FetchInventoryDescendents2` where the region advertises it (Second Life) or
the legacy UDP
`FetchInventoryDescendents` where it does not (OpenSim). It records the
folder/item totals and the deepest level reached, and asserts the crawl went
*beyond* the root — more than one folder and depth ≥ 1 — since a stock agent
inventory's root holds the standard system sub-folders. Green on OpenSim: 26
folders, 30 items, max depth 3, crawl ≈ 2.6 s loopback. `[both]`.
