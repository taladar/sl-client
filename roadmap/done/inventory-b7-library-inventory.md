---
id: inventory-b7
title: Library inventory
topic: inventory
status: done
origin: INVENTORY_ROADMAP.md
---

Context: [context/inventory.md](../context/inventory.md).

## B7. Library inventory — hold + fetch + separate cache (from A9)

- [x] Capture the library root + owner into the model: `library_root`
      (`Option<InventoryFolderKey>`) and `library_owner` (`Option<OwnerKey>`,
      set from the login `Option<AgentKey>` via `OwnerKey::Agent` — the uniform
      owner type the fetch path wants). Fold the already-emitted
      `Event::LibraryInventory` skeleton into the model under the
      `InventoryOwner::Library` tag (seeding `FolderState` from the skeleton
      version like the agent path). Added the `Inventory::library_owner` field +
      accessor and the public `Session::library_owner()`
      (`Session::library_root()` already landed in B3/B4).
      **No new login parse or skeleton event** — both already on master.
      Per-owner-merge against the library cache reuses the existing B5
      `merge_inventory_skeleton(owner, …)`.
- [x] Added `CAP_FETCH_LIBRARY = "FetchLibDescendents2"` in
      `sl-proto/src/session.rs` beside `CAP_FETCH_INVENTORY` (registered in
      `REQUESTED_CAPABILITIES`, re-exported at the crate roots). The descendents
      folds (CAPS `CAP_FETCH_INVENTORY | CAP_FETCH_LIBRARY` arm + the UDP arm)
      route a reply into the tree its **target folder** belongs to (new
      `inventory_reply_owner`), so a Library fetch stays in the Library tree;
      the AIS3 arm splits `CAP_LIBRARY_API_V3` → `Library`. Both runtime crawl
      ticks partition the (cross-tree) batch by `inventory_owner`: agent folders
      → `FetchInventoryDescendents2` + agent owner, Library folders →
      `FetchLibDescendents2` + `library_owner` when the grid serves the cap,
      else the **UDP** path (so OpenSim — which does not serve the cap — is
      exercised and folders never stay stuck `Fetching`). Parameterized the UDP
      `send_fetch_inventory_descendents` + `request_folder_contents` with an
      owner id (computed from the folder's owner + `library_owner`). The
      separate `<agent-uuid>.lib.inv.llsd.gz` persistence reuses the owner-keyed
      B5 bytes; its file shell is B10.
- [x] Tests: `library_inventory_holds_fetches_and_caches_apart` (lifecycle.rs) —
  distinct roots/owners, on-demand Library fetch addressed to the Library owner,
  a descendents reply folds under `InventoryOwner::Library`, and the Library
  cache round-trips through its own owner-keyed bytes; extended
  `login_emits_account_and_library_and_stores_them` to assert the skeleton folds
  under `Library`; model unit `agent_and_library_trees_query_apart` (per-owner
  cacheable snapshot + completion). Mutations stay agent-only by construction
  (every mutation site folds under `InventoryOwner::Agent`).
