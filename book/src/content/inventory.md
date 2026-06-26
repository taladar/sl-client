# Inventory

Inventory is the avatar's personal store of assets — clothing, objects, scripts,
notecards, landmarks — organized into a tree of folders. It is a good example of
the protocol's dual nature: there is an old [UDP](../comms/lludp-transport.md)
path and a modern [CAPS](../comms/caps.md) path, and a real client prefers the
latter but must understand the former.

## The model: folders and items

- A **folder** (category) has an id, a parent id, a name, a *folder type* (the
  system folders — Clothing, Objects, Trash, … — have well-known types), and a
  **version** number that increments on every change.
- An **item** has an id, the **asset id** it points at, an asset/inventory type,
  a permissions set (base / owner / group / everyone / next-owner masks), and
  sale info (type and price).

A subtlety worth internalizing: an *item* is a reference, and the *asset* is the
underlying data. Two items can point at the same asset; copying an item need not
copy the asset.

## The skeleton, then the contents

[Login](login.md) returns the **inventory skeleton**: every folder's id, name,
parent, type, and version — but no items. This lets a client render the folder
tree instantly and fetch contents lazily, only descending into folders the user
opens. The library (shared, read-only inventory) arrives as a separate skeleton.

## Fetching contents — two paths

- **Modern (CAPS / AIS3).** The `FetchInventoryDescendents2` capability fetches
  a folder's descendants over HTTP as [LLSD](../comms/llsd.md); the full
  read/write **AIS3** API (`InventoryAPIv3`, with `CreateInventoryCategory` for
  folder creation) is the modern way to mutate inventory. This path is reliable,
  batched, and the default.
- **Legacy (UDP).** Older messages request a folder's contents over the circuit
  and stream the descendants back. Still present for compatibility and on
  servers without the caps.

The `version` number is how a client knows its cached copy of a folder is
current without re-fetching.

## Mutating inventory

Commands cover the full set of operations, available in both legacy and AIS3
forms:

- **Folders** — create, rename/update, move, remove
  (`CreateInventoryFolder` / `UpdateInventoryFolder` / `MoveInventoryFolder` /
  `RemoveInventoryFolders`, and the `Ais3CreateFolder` / `Ais3RenameFolder` /
  `Ais3MoveFolder` / `Ais3RemoveFolder` / `Ais3PurgeFolder` equivalents).
- **Items** — create, update, move, copy, remove, change flags, purge a folder's
  descendants (`CreateInventoryItem`, `UpdateInventoryItem`,
  `MoveInventoryItem`, `CopyInventoryItem`, `RemoveInventoryItems`,
  `ChangeInventoryItemFlags`, `PurgeInventoryDescendents`, plus the `Ais3*Item`
  forms).
- **Giving** — hand an item or folder to another avatar
  (`GiveInventory` / `GiveInventoryFolder`); incoming offers arrive as
  [instant messages](chat.md#instant-messages) and are accepted/declined with
  `AcceptInventoryOffer` / `DeclineInventoryOffer`.
- **Links** — `LinkInventoryItem` creates a *link* item that points at an
  existing item **or** folder (the wire `OldItemID`), filed in a destination
  folder. The payload is a `NewInventoryLink`; the simulator allocates the link
  item's real id, so `link_inventory_item` returns an `InventoryCallbackId` to
  correlate the confirming `Event::InventoryItemCreated`.

When the simulator creates an item, it allocates the real id and confirms via
`Event::InventoryItemCreated`. Changes the server makes (including ones it made
on your behalf) arrive as `Event::InventoryBulkUpdate`.

## Rezzing an inventory item into the world

`Command::RezObjectFromInventory { params }` takes an inventory item and rezzes
it into the world as a new in-world object (`RezObject`). The `RezObjectParams`
describes the ray placement, the permission masks the new object is created
with, and the source inventory item (shared with the `RezRestoreToWorld`
restore path). ([Wearing](attachments.md) an item onto the avatar instead is a
separate command, and [dropping a script](scripts.md) into an existing object is
`RezScript`.)

## Server-pushed inventory changes

The simulator can change inventory on its own — an item deleted from another
session, moved to trash, or re-parented — and pushes the change so a client
mirroring inventory stays in sync. None has a reply; a mirror simply applies it:

- `Event::InventoryItemsRemoved { items }` — these items no longer exist.
- `Event::InventoryFoldersRemoved { folders }` — these folders (and their
  descendants) no longer exist.
- `Event::InventoryObjectsRemoved { folders, items }` — a mixed removal of both
  in a single message.
- `Event::InventoryItemsMoved { stamp, moves }` — each `InventoryItemMove`
  re-parents an item into a folder, optionally renaming it; `stamp` says whether
  the simulator re-timestamped the moved items.

---

> **In this codebase**
>
> - Core types are in `sl-proto/src/types/inventory.rs`: `InventoryFolder`,
>   `InventoryItem`, `NewInventoryItem`, `NewInventoryLink` (the
>   `LinkInventoryItem` payload, pointing at an `InventoryItemOrFolderKey`),
>   `InventoryItemMove` (the `InventoryItemsMoved` relocation), `InventoryType`,
>   plus `InventoryOffer` for offers in IM binary buckets. `RezObjectParams`
>   (the `RezObjectFromInventory` payload) is in
>   `sl-proto/src/types/editing.rs`.
> - The capability names are `CAP_FETCH_INVENTORY`
>   (`FetchInventoryDescendents2`), `CAP_INVENTORY_API_V3` (`InventoryAPIv3`),
>   `CAP_CREATE_INVENTORY_CATEGORY`, in `sl-proto/src/session.rs`. The AIS3
>   URL/body helpers are in `sl-wire/src/inventory.rs` (`ais_category_url`,
>   `build_ais_update_item_body`, …).
> - The HTTP fetch driver is `sl-client-tokio/src/inventory.rs`; the worked
>   example is `sl-client-tokio/examples/inventory_edit.rs`.
> - Events: `InventorySkeleton`, `LibraryInventory`, `InventoryDescendents`,
>   `InventoryItemCreated`, `InventoryBulkUpdate` in
>   `sl-proto/src/types/event.rs`.
