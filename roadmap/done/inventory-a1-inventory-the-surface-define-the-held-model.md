---
id: inventory-a1
title: Inventory the surface & define the held model
topic: inventory
status: done
origin: INVENTORY_ROADMAP.md
---

Context: [context/inventory.md](../context/inventory.md).

**A1. Inventory the surface & define the held model.** Enumerate what
    exists (the wire layer, the raw maps, the `cache_inventory*` folds, the
    four events) and define the unified held model: an `Inventory` value
    owning the folder store, the item store, the **roots** (agent root +
    library root), and an **owner discriminator** (`Agent` vs `Library`) so
    the two trees share one model but stay queryable apart. State the
    boundary: this roadmap holds + caches **structure/metadata** (folders +
    items); **asset bytes** (textures, meshes, notecard/script contents) are a
    separate concern, OUT of scope (a future `shared_cache_dir` asset cache).
    Confirm the typed keys (`InventoryKey`, `InventoryFolderKey`,
    `InventoryItemOrFolderKey`) and domain types (`InventoryFolder`,
    `InventoryItem`) are reused unchanged underneath.

## Held-model reference (from A1)

A single `Inventory` value (new `sl-proto/src/session/inventory.rs`) replaces
the three loose `Session` fields (`inventory_folders` / `inventory_items` /
`inventory_root`) and both trees live in it. It owns a folder store keyed by
`InventoryFolderKey` and an item store keyed by `InventoryKey` (typed keys, not
the raw `Uuid` maps of today), the two roots (`agent_root` /
`library_root: Option<InventoryFolderKey>`), the
`library_owner: Option<OwnerKey>` (for library fetches),
`next_inventory_callback`, and a `dirty: bool` (A10). Each folder is wrapped in
a `FolderEntry` carrying the `InventoryFolder` payload, an `InventoryOwner`
(`Agent` / `Library`), its `FolderState` (A2), and its child-key sets (A2); each
item in an `ItemEntry` carrying the `InventoryItem` and its owner. The domain
types `InventoryFolder` / `InventoryItem` and the typed keys (`InventoryKey` /
`InventoryFolderKey` / `InventoryItemOrFolderKey`) are reused unchanged as
payloads. Boundary: structure/metadata only — **asset bytes** (textures, meshes,
notecard/script contents) stay out of scope (a future `shared_cache_dir` asset
cache).

**Surface verified against the code (anchors for B3).** The three loose fields
are `session.rs:1002` (`inventory_root: Option<InventoryFolderKey>`),
`session.rs:1082` (`inventory_folders: BTreeMap<Uuid, InventoryFolder>`),
`session.rs:1088` (`inventory_items: BTreeMap<Uuid, InventoryItem>`), plus
`session.rs:1091` (`next_inventory_callback: InventoryCallbackId`). The folds
are
`cache_inventory_folder` (`methods.rs:7897`), `cache_inventory`
(`methods.rs:7910`), `cache_inventory_item` (`methods.rs:7920`), reached from
the `InventoryDescendents` arm (`methods.rs:2414`), the
`UpdateCreateInventoryItem`
arm (`methods.rs:2432` → `Event::InventoryItemCreated`), and the
`BulkUpdateInventory` arm (`methods.rs:2467`). The login skeleton seeds the map
at `methods.rs:1216-1226` via `skeleton_folder` (`conversions.rs:957`), which
already copies the authoritative `version` per folder; and
`cache_inventory_folder` already preserves an existing version when a fold
carries `0` (the seed of the A2 authoritative-version rule). Current accessors:
`inventory_root` (`methods.rs:7812`, typed), `inventory_folder`/`inventory_item`
(`:7848`/`:7855`, typed lookup), the raw `inventory_folders`/`inventory_items`
(`:7861`/`:7867`, `&BTreeMap<Uuid, …>`, to be deprecated in B4), and
`inventory_children` (`:7876`, returns `(Vec<&InventoryFolder>,
Vec<&InventoryItem>)` by an O(tree) `parent_id`/`folder_id` scan — the index in
A2 replaces this). The domain types live in `types/inventory.rs`
(`InventoryFolder` ll.13-25 with `version: i32` + `folder_type: i8`,
`InventoryItem` ll.29-69 with `item_type`/`inv_type: i8`, `sale_type: u8`,
`asset_id: Uuid`, `Permissions5`); the typed keys are newtypes in
`sl-types/src/key.rs` (`InventoryKey` l.306, `InventoryFolderKey` l.435,
`InventoryItemOrFolderKey` l.995, `OwnerKey` l.471) — all reused unchanged.

**Two cross-references for A9/B7 discovered while enumerating.** (1) The login
response carries the library identifiers. `library_root` was already typed
(`Option<InventoryFolderKey>`) but `library_owner` was a bare `Uuid` — fixed in
this commit to `Option<AgentKey>` in both `sl-wire` `LoginSuccess`
(`login.rs:393`) and `sl-proto` `LoginAccount` (`avatar_profile.rs:353`): the
wire field is `inventory-lib-owner`/`agent_id`, always an avatar (never a
group), so `AgentKey` is the tight fit (`OwnerKey` would admit an impossible
`Group`
arm). The held model's `library_owner: Option<OwnerKey>` widens it to `OwnerKey`
only where the fetch path wants a uniform owner type. (2) A library skeleton
event **already exists**:
`Event::LibraryInventory` is emitted at `methods.rs:1213` alongside
`InventorySkeleton`; A9/B7 fold its folders into the model under
`owner = Library` rather than introducing a new event.
