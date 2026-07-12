---
id: inventory-b4
title: Idiomatic public model API
topic: inventory
status: done
origin: INVENTORY_ROADMAP.md
---

Context: [context/inventory.md](../context/inventory.md).

## B4. Idiomatic public model API — read + write (from A7)

*Read side:*

- [x] Add the `Child` enum + `FolderInfo` / `ItemInfo` view types (typed keys,
  resolved `FolderType` / `AssetType` / `InventoryType` / `SaleType` enums) in
  `sl-proto/src/types/inventory.rs`; add the **new** `FolderType` enum (LL
  `LLFolderType::EType`, folder-only codes — **not** `AssetType`, which collides
  at `8`) and the opaque `InventoryCursor` (mirror `MessageCursor`).
- [x] On `Session`: keep the already-present `inventory_folder` /
      `inventory_item` by-key lookups (`methods.rs:7848` / `:7855`) verbatim;
      **change** `inventory_children(folder)` (`:7876`) from the `Vec` tuple to
      `impl Iterator<Item = Child<'_>>` (no external callers); add the **new**
      `library_root()` (from `login_account().library_root`),
      `folder_fetch_state(folder) -> Option<FolderState>` (B3's type), and a
      paged `inventory_folder_page(folder, before, limit)` returning the owning
      view-type window + next `InventoryCursor` (the `history_page` cursor
      precedent — owning `Vec`s, not borrowed iterators).
- [x] ~~Deprecate (`#[deprecated]`) the raw `inventory_folders()` /
  `inventory_items()` map accessors.~~ **Superseded by B3:** the stores were
  re-keyed to typed keys in B3, so these accessors already return
  `&BTreeMap<InventoryFolderKey, InventoryFolder>` /
  `&BTreeMap<InventoryKey, InventoryItem>` (no longer raw `Uuid`); nothing to
  deprecate. B4 may still add the `Child`-iterator / view-type read surface
  alongside them.

*Write side (symmetric with the read side — coherence mechanics stay in
B3/A10):*

- [x] Re-type the mutation params from raw `i8`/`u8` to the resolved enums:
      folder create/update (`methods.rs:7962`/`:7995`) take `FolderType`;
      `NewInventoryItem` (`inventory.rs:79`) takes `AssetType` / `InventoryType`
      / `WearableType` (reuse the existing `appearance.rs:19` enum);
      `NewInventoryLink` (`:121`) takes `AssetType` / `InventoryType`;
      `.to_code()` at the wire builder. Propagate the same enum typing to the
      matching write `Command` variants so tokio / bevy / REPL stay at parity.
- [x] Make `create_inventory_folder` (`:7962`) return the new
      `InventoryFolderKey` and **error** on a nil or already-present id (the
      caller still mints the v4 id — `sl-proto` is sans-IO, no UUID generation);
      document the inherent client-folder-id / sim-item-id asymmetry.
- [x] Add cycle / unknown-parent guards to `move_inventory_folders` (`:8049`,
      the in-place re-parent at `:8062`) using the B3 index — return an `Error`
      before the wire send rather than corrupting the tree.
- [x] Add clobber-free convenience helpers — `rename_inventory_folder` /
      `rename_inventory_item` (and a re-type / set-permissions equivalent) —
      that read the untouched fields from the cached folder/item and submit the
      full `UpdateInventory*`, so a single-attribute edit can't accidentally
      re-parent or reset permissions; keep the raw all-fields
      `update_inventory_*` for power users.
- [x] Tests: tree-walk over a seeded tree; pagination cursor across a large
      mixed folder (folders then items); view types carry typed keys + resolved
      enums (incl. `FolderType`); a write helper takes/returns the resolved
      enums; `create_inventory_folder` errors on a nil/duplicate id and returns
      the new key; a cycle-forming `move_inventory_folders` errors and leaves
      the tree unchanged; `rename_inventory_*` preserves the other fields; an
      optimistic create is overwritten by the authoritative
      `InventoryItemCreated` / `BulkUpdateInventory` fold (reconciliation).

**Landed (notes for later tasks).** `FolderType` (LL `LLFolderType::EType`,
`None`/`Other(i8)` + `from_code`/`to_code`), `Child<'a>`, owning `FolderInfo` /
`ItemInfo`, and the opaque `InventoryCursor` live in
`sl-proto/src/types/inventory.rs`, re-exported at the `sl-proto` /
`sl-client-tokio` / `sl-client-bevy` crate roots (so B8's pull-bridge can carry
the view types + cursor). `inventory_children` now yields
`impl Iterator<Item = Child<'_>>` (zero-copy, for bevy's `&Session` reader) and
`inventory_folder_page(folder, before, limit)` returns the owning
`(Vec<FolderInfo>, Vec<ItemInfo>, Option<InventoryCursor>)` window over the
combined folders-then-items sequence (one page can span the boundary). Write
side: `create_inventory_folder` / `update_inventory_folder` and
`NewInventoryItem` / `NewInventoryLink` (+ the matching `Command` variants and
the tokio/bevy/REPL dispatch + the REPL `parse_folder_type` helper) now take the
resolved `FolderType` / `AssetType` / `InventoryType` / `WearableType` enums,
`.to_code()`-narrowed at the wire builder; `create_inventory_folder` returns the
new `InventoryFolderKey` and rejects a nil/duplicate id;
`move_inventory_folders` rejects an unknown-parent or cycle (O(1) via the index,
`Inventory::contains_folder` / `is_self_or_descendant`) before sending; and the
clobber-free `rename_inventory_folder` / `retype_inventory_folder` /
`rename_inventory_item` / `set_inventory_item_permissions` helpers read the
untouched fields from the cache. New error: `Error::InvalidInventoryOperation`.
