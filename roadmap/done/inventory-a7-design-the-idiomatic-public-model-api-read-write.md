---
id: inventory-a7
title: Design the idiomatic public model API (read + write)
topic: inventory
status: done
origin: INVENTORY_ROADMAP.md
---

Context: [context/inventory.md](../context/inventory.md).

**A7. Design the idiomatic public model API (read + write).** *Read:*
    borrowed, typed tree-walk accessors on `Session`:
    `inventory_root`/`library_root` (typed `InventoryFolderKey`), a `Vec`-free
    `inventory_children(folder) -> impl Iterator` yielding a `Child` enum
    (`Folder(&InventoryFolder)` / `Item(&InventoryItem)`) or split folder/item
    iterators, a folder/item lookup by typed key,
    `folder_fetch_state(folder)`, and snapshot **view types** (`FolderInfo` /
    `ItemInfo`, owning, `Arc`-friendly, exposing typed keys + resolved enums
    like `AssetType` / `InventoryType` / `SaleType` instead of raw `i8`/`u8`).
    Pagination for large folders via an opaque `InventoryCursor` (the
    `MessageCursor` precedent). Deprecate `inventory_folders()` /
    `inventory_items()` (raw `&BTreeMap<Uuid, …>`). *Write:* make the existing
    mutation surface symmetric with the read side — accept the same resolved
    enums (`FolderType` / `AssetType` / `InventoryType` / `WearableType`) on
    writes instead of raw `i8`/`u8`, surface caller mistakes as `Error` (nil /
    duplicate folder id, a move that would form a cycle or target an unknown
    parent — all O(1) against the B3 index), add focused clobber-free helpers
    (rename / re-type / set-permissions reading the other fields from the
    cache) so an all-fields `UpdateInventory*` cannot accidentally re-parent
    or reset permissions, and pin the optimistic-write → authoritative-reply
    reconciliation policy (the optimistic edit sets state + `dirty` with a
    guessed version, overwritten last-write-wins when the server reply folds
    in). The cache-coherence *mechanics* (index / `FolderState` / version at
    every mutation site) stay owned by A2/B3; A10 owns `dirty`.

## Public model API reference (from A7) — idiomatic read + write

View types in `sl-proto/src/types/inventory.rs`: a borrowed
`enum Child<'a> { Folder(&'a InventoryFolder), Item(&'a InventoryItem) }`;
owning snapshots `FolderInfo` (typed `folder_id` / `parent_id`, `name`, a
resolved `FolderType`, `version`, `FolderState`) and `ItemInfo` (typed `item_id`
/ `folder_id`, `name`, `asset_id`, resolved `AssetType` / `InventoryType`, an
`Option<(SaleType, LindenAmount)>` sale, `Permissions5`, dates/creator/owner) —
typed keys + resolved enums, never raw `i8` / `u8`. An opaque
`InventoryCursor(usize)`. Borrowed accessors on `Session`: `inventory_root()` /
`library_root()`; `inventory_folder(key)` / `inventory_item(key)`;
`inventory_children(folder)` returning `impl Iterator<Item = Child<'_>>`;
`folder_fetch_state(folder)`; and a paged
`inventory_folder_page(folder, before, limit)` returning
`(Vec<FolderInfo>, Vec<ItemInfo>, Option<InventoryCursor>)` (the `history_page`
precedent). The raw `inventory_folders()` / `inventory_items()` accessors become
`#[deprecated]`.

**Verified against the code (anchors for B4).** Four A7 accessors **already
exist** on `Session` (added under #30, the inventory cache):
`inventory_root() -> Option<InventoryFolderKey>` (`methods.rs:7812`),
`inventory_folder(InventoryFolderKey) -> Option<&InventoryFolder>` (`:7848`),
`inventory_item(InventoryKey) -> Option<&InventoryItem>` (`:7855`), and
`inventory_children(InventoryFolderKey) -> (Vec<&InventoryFolder>,
Vec<&InventoryItem>)` (`:7876`). So B4 **refines** rather than building from
scratch: keep the two by-key lookups verbatim and **change**
`inventory_children`'s return from the `Vec` tuple to the `Vec`-free
`impl Iterator<Item = Child<'_>>`. That signature change is clean —
grep-confirmed there are **no external callers** of `inventory_children` /
`inventory_folders()` / `inventory_items()` anywhere in the workspace (the
runtimes call only the mutation `remove_inventory_*`: tokio `lib.rs:538`/`:556`,
bevy `:688`/`:724`). The two raw map accessors to `#[deprecated]` are
`inventory_folders() -> &BTreeMap<Uuid, InventoryFolder>` (`:7861`) and
`inventory_items() -> &BTreeMap<Uuid, InventoryItem>` (`:7867`).

**`library_root` is not on `Session` yet.** The library root from the login
response is folded into `LoginAccount.library_root: Option<InventoryFolderKey>`
(`avatar_profile.rs:350`, set at `methods.rs:1202`) and reachable only via
`login_account().library_root` (`:7820`). B4 adds `Session::library_root() ->
Option<InventoryFolderKey>` reading from the stored login account — no new state
(A9/B7 later holds the library *tree*; A7's accessor only needs the root id).

**Resolved-enum converters verified (anchors for B4).** `ItemInfo` resolves its
three raw `i8` / `u8` fields with existing converters, all `#[non_exhaustive]`
enums with a fallback arm already exercised elsewhere (`chat.rs:459`,
`methods.rs:2649`): `AssetType::from_code(i32)` (`asset.rs:102`) on
`i32::from(item.item_type)`; `InventoryType::from_code(i32)` (`asset.rs:286`) on
`i32::from(item.inv_type)`; `SaleType::from_code(u8)` (`editing.rs:169`) on
`item.sale_type`. The sale field pairs that `SaleType` with the existing
`InventoryItem.sale_price: Option<LindenAmount>` (already `None` when not for
sale) into `Option<(SaleType, LindenAmount)>`. `Permissions5`, `creation_date`,
and `creator_id` / `owner` / `group` copy across unchanged (already typed).

**`FolderType` does not exist — B4 must add it.** `FolderInfo`'s resolved
`folder_type` has **no** converter today (grep-confirmed: `FolderType` appears
only in doc comments). B4 adds a `FolderType` enum to `sl-proto` modelled on LL
`LLFolderType::EType` (`indra/llinventory/llfoldertype.h:39-108`). It is **not**
`AssetType`: folder preferred-types add folder-only codes and one collides
(`AT_CATEGORY = 8` vs `FT_ROOT_INVENTORY = 8`), so reusing `AssetType` would
resolve wrongly. Cover at least the protected/system types — `Texture = 0` …
`Bodypart = 13` (shared with assets), plus `RootInventory = 8`, `Trash = 14`,
`LostAndFound = 16`, `Favorite = 23`, `CurrentOutfit = 46`, `Outfit = 47`,
`MyOutfits = 48`, `Inbox = 50`, `Outbox = 51`, `MarketplaceListings = 53`,
`Settings = 56`, `Material = 57` — with `None = -1` and an `Other(i8)` fallback,
plus `from_code(i8)` / `to_code() -> i8` mirroring the existing enums' pattern.

**Cursor + pagination shape.** `InventoryCursor(usize)` mirrors `MessageCursor`
(`chat_session.rs:391`) exactly: crate-private `new` / `consumed` for in-crate
paging plus `pub from_consumed` / `consumed_count` so A8's channel runtimes can
carry it across the `Command` / `Event` boundary. `inventory_folder_page(folder,
before: Option<InventoryCursor>, limit) -> (Vec<FolderInfo>, Vec<ItemInfo>,
Option<InventoryCursor>)` mirrors `history_page` (`methods.rs:5057`) for the
*cursor* shape, but returns **owning** view-type `Vec`s (not borrowed iterators)
because `FolderInfo` / `ItemInfo` are snapshots. One cursor walks the
**combined** child sequence (folders first, then items, in the deterministic
parent→children-index order) so a single page can span the folder/item boundary
of one mixed folder. Zero-copy borrowed walking stays available through
`inventory_children` (the `Child` iterator, for bevy's `&Session` reader);
`inventory_folder_page` is the owning / `Arc`-friendly read behind A8's pull
bridge.

**Ordering.** `FolderState` (the `Unknown` / `Fetching` / `Loaded { version }`
type) and the parent→children index land in **B3** (model + fetch-state), before
B4 — so `folder_fetch_state(folder) -> Option<FolderState>` and
`FolderInfo.state: FolderState` have their type ready when B4 builds the read
API on top.

**Write side — symmetric with the read side (anchors for B4).** A full mutation
surface already exists on `Session` and already does
**optimistic local cache updates** with typed keys: `create_inventory_folder`
(`methods.rs:7962`), `update_inventory_folder` (`:7995`),
`move_inventory_folder(s)` (`:8032`/`:8049`), `remove_inventory_folders`
(`:8076`), `create_inventory_item` (`:8100`), `link_inventory_item` (`:8121`),
`update_inventory_item` (`:8140`), `move_inventory_item(s)` (`:8160`/`:8178`),
`copy_inventory_item` (`:8212`), `remove_inventory_items` (`:8239`),
`change_inventory_item_flags` (`:8260`), `purge_inventory_descendents`
(`:8281`), `remove_inventory_objects` (`:8299`). The cache-coherence *mechanics*
(route every one through the model so the index / `FolderState` / authoritative
version stay consistent) are already owned by A2/B3 (the move/remove/seed sites
are enumerated there); A10 owns the `dirty` flag set by each. A7 adds only the
**idiomatic-ergonomics** layer so the write side stops being the raw twin of the
now-typed read side:

- **Typed enum params instead of raw `i8`/`u8`** (mirror the read views): folder
  create/update take `FolderType` (not `folder_type: i8`); `NewInventoryItem`
  (`inventory.rs:79`) takes `AssetType` / `InventoryType` / `WearableType` (the
  last already exists, `appearance.rs:19`) for its `asset_type` / `inv_type` /
  `wearable_type`; `NewInventoryLink` (`:121`) takes `AssetType` (`AT_LINK` /
  `AT_LINK_FOLDER`) / `InventoryType`; each `.to_code()`s for the wire builder.
  Keep `flags: u32` a bitfield (out of scope).
- **Folder-id allocation stays caller-supplied** — `sl-proto` is sans-IO and
  generates **no** UUIDs (grep-confirmed: no `new_v4` / `rand` / `getrandom`),
  so the runtime shell mints the fresh v4 id, not `Session`. The protocol
  asymmetry is inherent (client allocates *folder* ids, the sim allocates *item*
  ids and echoes a callback id, `:8104`/`:8126`) — document it rather than hide
  it. The footgun fix is **validation, not generation**:
  `create_inventory_folder` returns an `Error` for a nil or already-present
  `folder_id` instead of silently clobbering the cache, and returns the new
  `InventoryFolderKey` for symmetry with the read accessors.
- **Local guards off the B3 index (O(1)):** `move_inventory_folders` rejects a
  move whose target is the folder itself or one of its descendants (a cycle) and
  a move to a parent not in the model — surfaced as `Error` before the wire
  send, not silent corruption (the in-place re-parent at `:8062` currently
  trusts the caller).
- **Focused, clobber-free updates:** `update_inventory_folder` (`:7995`) /
  `update_inventory_item` (`:8140`) are all-fields `UpdateInventory*` overwrites
  (the wire shape) — a caller editing one attribute can accidentally re-parent
  or reset permissions/owner. Add convenience wrappers —
  `rename_inventory_folder` / `rename_inventory_item` (and a re-type /
  set-permissions equivalent) — that read the untouched fields from the cached
  folder/item (now reachable via the read model) and submit the full message, so
  single-attribute edits can't clobber the rest. The raw all-fields methods stay
  for power users.
- **Optimistic → authoritative reconciliation policy (B3/A10 mechanics, named
  here):** an optimistic edit updates the model + sets `dirty` immediately with
  a *guessed* version (create uses `1`, `:7983`); the folder stays `Loaded`
  (contents are known). When the authoritative reply folds in —
  `BulkUpdateInventory` (`methods.rs:2467` → `Event::InventoryBulkUpdate`),
  `UpdateCreateInventoryItem` (`:2432` → `Event::InventoryItemCreated`, carrying
  the sim-allocated item id correlated by the `InventoryCallbackId`), or a later
  descendents refetch — it **overwrites** the optimistic guess (server is
  authoritative, last-write-wins), and `cache_inventory_folder`'s existing
  preserve-on-`version 0` guard (`:7898-7903`) keeps a real version from being
  clobbered by a child fold.
