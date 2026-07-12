---
id: test-library-tree-fetch
title: fetch the read-only Library tree
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 5 — Inventory (deep) `[both]`
---

Context: [context/test.md](../context/test.md).

`library-tree-fetch` — fetch the read-only Library tree. `1av`. Where
`inventory-tree-crawl` walks the agent's *own* tree, this walks the grid-owned
read-only **Library** — a second inventory tree owned by a distinct Library
owner, surfaced alongside the agent root by `QueryInventoryRoots`. The crawl
is the same breadth-first descent over `RequestFolderContents` /
`InventoryDescendents`, but every Library folder is filed under the Library
owner, so the library routes each fetch to `FetchLibDescendents2` where the
region advertises it (Second Life) or the legacy UDP
`FetchInventoryDescendents` addressed to the Library owner where it does not
(OpenSim) — automatically, a single `[both]` path. It asserts the Library is a
*separate* tree (its root is distinct from the agent root) and that the
descent went beyond the root (folders > 1, depth ≥ 1, since a stock Library
holds the standard system sub-folders). A grid with no Library is recorded
`partial` rather than failed. **Surfaced & fixed a real client bug**
(behavioural, in `sl-proto` so both runtimes get it): OpenSim emits a single
nil-id placeholder `FolderData` block for an empty folder (an LLUDP "stuffing"
quirk a real viewer ignores), which the `InventoryDescendents` fold passed
through verbatim — so the crawl tried to fetch the phantom nil folder (OpenSim
never answers) and hung, and the background crawl would have marked it
`Fetching` forever, never reaching `fully_loaded(Library)`. The UDP and CAPS
folds now drop nil-id folders/items, matching the existing
`bulk_update_inventory_from_llsd` filter; regression test
`inventory_descendents_drops_nil_placeholder_subfolder` in
`sl-proto/tests/lifecycle.rs`. Every OpenSim Library leaf carries this
stuffing block (the agent tree did not, which is why `inventory-tree-crawl`
never hit it). Green on OpenSim: 7 folders (root + 6), 17 items, depth 1,
crawl ≈ 0.5 s
loopback. `[both]`; the aditi run is deferred with the batch (no aditi record
produced this session).
