---
id: inventory-a2
title: Design the fetch-state machine & the parent→children index
topic: inventory
status: done
origin: INVENTORY_ROADMAP.md
---

Context: [context/inventory.md](../context/inventory.md).

**A2. Design the fetch-state machine & the parent→children index.**
Specify a per-folder fetch state — `Unknown` (in the tree from skeleton/parent
but contents not fetched) / `Fetching` (request in flight) / `Loaded { version
}` (contents present, known version) — and the authoritative-version rule: a
folder's version comes from the skeleton or a descendents reply, and `Loaded`
is only entered with that version. Design the parent→children index (folder id
→ child folder ids + child item ids) maintained incrementally by every fold
site so children/tree-walks are O(children), not O(whole tree). Define how the
existing `cache_inventory*` folds update the index + fetch-state.

## Fetch-state & index reference (from A2)

`pub enum FolderState { Unknown, Fetching, Loaded { version: i32 } }`.
Authoritative-version rule: a folder enters `Loaded` only with a version from
the login skeleton or a descendents reply *for that folder*; a sub-folder that
appears merely as a child in some other folder's descendents reply (wire
`version 0`) stays `Unknown` until fetched in its own right. The parent→children
index is the child-key sets on `FolderEntry`
(`child_folders: BTreeSet<InventoryFolderKey>`,
`child_items: BTreeSet<InventoryKey>`), maintained at every fold:
`cache_inventory_folder` inserts the folder, links it under its `parent_id`'s
entry, and (if new) creates its entry `Unknown`; `cache_inventory_item` inserts
the item and links it under its `folder_id`; removals unlink. So
`inventory_children(folder)` is O(children), not O(tree).

**Fold / unlink sites verified against the code (anchors for B3).** Every site
that mutates `inventory_folders` / `inventory_items` must maintain the index +
fetch-state. Inserts/updates flow through `cache_inventory_folder`
(`methods.rs:7897`) and `cache_inventory_item` (`:7920`) — both via
`cache_inventory` (`:7910`) — the natural index hooks. **But two classes of site
bypass them and B3 must route through the model too:** (1) the login **skeleton
seed inserts directly** (`:1223-1224`, a raw `inventory_folders.insert`, *not*
`cache_inventory_folder`) — this is the one site carrying the authoritative
per-folder `version`, so it must set `Loaded`/`Unknown` + index *there*, not
lose it; (2) the **re-parent mutations edit the parent link in place** —
`move_inventory_folders` mutates `folder.parent_id` (`:8062`) and
`move_inventory_items` mutates `item.folder_id` (`:8193`) — so the index must
*unlink from the old parent and link to the new* at these two sites (an
insert/remove pair does not model a move). Unlink/removal sites:
`purge_cached_descendents` (`:7926`, recursive), `remove_inventory_folders`
(`:8085-8086`), `remove_inventory_items` (`:8248`),
`purge_inventory_descendents` (`:8288`), and `remove_inventory_objects`
(`:8310-8314`) — each unlinks the
dropped keys from their parent's child-sets. The optimistic creates
(`create_inventory_folder` `:7978` version `1`, the AIS folder create `:8011`)
already route through `cache_inventory_folder`, so they index for free once the
hook is there.

Authoritative-version anchor: `cache_inventory_folder` **already** preserves an
existing version when a fold carries `0` (`:7898-7903`) — the seed of the
`Loaded` rule; B3 promotes that ad-hoc guard into `FolderState` so a `version 0`
child fold leaves the entry `Unknown` rather than fabricating `Loaded { 0 }`.
