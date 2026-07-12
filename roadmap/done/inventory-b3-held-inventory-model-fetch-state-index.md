---
id: inventory-b3
title: Held Inventory model + fetch-state + index
topic: inventory
status: done
origin: INVENTORY_ROADMAP.md
---

Context: [context/inventory.md](../context/inventory.md).

## B3. Held `Inventory` model + fetch-state + index (from A1·A2)

Folds the existing raw maps into a model module; migrates the `cache_inventory*`
folds and the accessors onto it with no behaviour change yet.

- [x] Add `sl-proto/src/session/inventory.rs`: an `Inventory` struct owning the
      folder/item stores, the `owner` discriminator (`Agent` / `Library`), the
      roots, a `FolderState` (`Unknown` / `Fetching` /
      `Loaded { version: i32 }`) per folder, and the parent→children index.
      `const fn` empty constructor. **Implementation note:** the stores are
      keyed by the **typed** `InventoryFolderKey` / `InventoryKey` (not bare
      `Uuid`). Each folder is **one `FolderEntry { folder:
      Option<InventoryFolder>, owner, state, child_folders, child_items }`** in
      a single `BTreeMap<InventoryFolderKey, FolderEntry>` — payload and
      bookkeeping live together so they cannot desync (the invariant is
      type-enforced), and the `Option` payload models a folder
      *known to exist but not yet fetched* (its bookkeeping/state/index is
      tracked while the payload is `None`, e.g. a child folded before its
      parent, or a descendents reply preceding the skeleton entry). Items carry
      no bookkeeping, so they stay a plain
      `BTreeMap<InventoryKey, InventoryItem>`. The `inventory_folders()` /
      `inventory_items()` accessors become
      `impl Iterator<Item = &InventoryFolder>` / `&InventoryItem` (folders skip
      payload-less entries); grep-confirmed no external callers, so B4's
      "deprecate the raw `&BTreeMap<Uuid, …>` accessors" item is superseded.
- [x] Move `inventory_folders` / `inventory_items` / `inventory_root` /
      `next_inventory_callback` off `Session` into the model field; migrate
      every internal use to maintain the index + fetch-state: the central folds
      (`cache_inventory`/`_folder`/`_item`), the **direct skeleton seed** (now
      routes through `Inventory::cache_folder` so it sets the authoritative
      version + index instead of a raw `insert`), the **re-parent mutations**
      that edit the parent link in place (`move_inventory_folders` →
      `Inventory::reparent_folder`, `move_inventory_items` →
      `Inventory::move_item` — unlink-old + link-new), and the removal sites
      (`Inventory::purge_descendents` / `remove_folder` / `remove_item`, the
      latter two unlinking from the parent's child set).
- [x] Set the authoritative folder version from the skeleton and from
  descendents replies (`Inventory::mark_folder_loaded`, at both the UDP and CAPS
  descendents arms); keep sub-folders (`version 0`) `Unknown`.
- [x] **Persistence guard (from A10):** add **no** inventory clear at the four
  region-boundary sites; documented beside the chat persistence guard; assert
  that in B11.
- [x] Tests: skeleton seeds roots + `Unknown`; a descendents reply flips the
  folder to `Loaded { version }` and the index lists its children; a mutation
  keeps the index consistent. (Integration tests in `lifecycle.rs` extended;
  focused model unit tests added in `inventory.rs`.)

  **B4 down-payment landed here (their readers were needed now):** the public
  `Session::folder_fetch_state()` and `Session::library_root()` accessors (both
  also listed under B4) plus a new `Session::inventory_owner()` accessor land in
  this task as the readers for the new `FolderState` / library-root / `owner`
  fields; `FolderState` and `InventoryOwner` are re-exported at the crate root.
