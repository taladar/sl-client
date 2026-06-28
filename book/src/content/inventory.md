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

## The held inventory model

Beyond firing fetches and reacting to events, the `Session` *holds* a live
two-level inventory model in memory and keeps it current. One `Inventory` value
holds both trees — the agent's own inventory and the read-only shared
**Library** — each tagged by `InventoryOwner` (`Agent` / `Library`) with its own
root. It holds **structure and metadata only**: folders, items, and their
relationships. Asset *bytes* are out of scope (an item carries its asset id, not
the asset). The model is fed from every source at once and reconciled into a
single picture: the login skeleton, folder-contents fetches (UDP or CAPS), the
agent's own mutations, and simulator pushes all fold into it.

Each folder tracks the fetch state of its *contents* separately from its
version, as a `FolderState`:

- **`Unknown`** — the folder is known to exist (named in the skeleton, or as a
  child in some other folder's descendents reply) but its own contents have not
  been fetched.
- **`Fetching`** — a descendents request for it is in flight.
- **`Loaded { version }`** — its contents are present, fetched at that version.

This is deliberately distinct from `InventoryFolder::version`: a skeleton folder
carries a known, authoritative version yet `Unknown` contents until it is
fetched in its own right. A parent→children index resolves a folder's immediate
sub-folders and items in `O(children)` without scanning the whole model. A
folder can even be *known to exist* before its own metadata arrives — the entry
then tracks its state and child links with an absent payload until the metadata
lands.

## Reading the held model

The roots are `Session::inventory_root` and `Session::library_root` (each an
`Option<InventoryFolderKey>`), with `Session::library_owner` for the id Library
fetches are addressed to. From a folder id, two complementary read surfaces walk
the tree:

- **Borrowed, zero-copy.** `Session::inventory_children(folder_id)` yields a
  `Child<'_>` iterator — the folder's sub-folders first (in key order), then its
  items — borrowing straight out of the model. This is the cheapest tree walk;
  the bevy runtime reads it directly through `&Session`. Single lookups are
  `Session::inventory_folder(id)` and `Session::inventory_item(id)`, returning a
  borrowed `&InventoryFolder` / `&InventoryItem`.
- **Owning, paginated.** `Session::inventory_folder_page(folder_id,
  before, limit)` returns one bounded page of *owning* snapshots —
  `Vec<FolderInfo>`, `Vec<ItemInfo>`, and an `Option<InventoryCursor>` for the
  next page (`None` at the end). A page is a window over the *combined* child
  sequence (sub-folders then items), so one page can straddle the folder/item
  boundary of a mixed folder. Pass `None` for `before` to start, then feed each
  returned cursor back.

`FolderInfo` and `ItemInfo` are the resolved view types: typed keys and resolved
enums (`FolderType`, `AssetType`, `InventoryType`, `SaleType`, a paired
`Option<(SaleType, LindenAmount)>` sale, a `FolderState`) instead of the raw
`i8` / `u8` wire fields of `InventoryFolder` / `InventoryItem`. They are cheap
to clone and `Arc`-share, which is what makes them the payload of the
pull-bridge below. `Session::folder_fetch_state(id)` reports a folder's
`FolderState`, and `Session::inventory_owner(id)` says which tree it belongs to.

The older flat accessors `Session::inventory_folders()` and `inventory_items()`
still return every cached folder / item as a loose iterator, but the typed
`inventory_children` / `inventory_folder_page` walk is the preferred surface —
it preserves tree structure, paginates, and resolves the wire bytes into enums.

## Background fetch

Populating the whole tree is opt-in. `Session::set_background_inventory_fetch(
true)` enables an automatic breadth-first crawl over `Unknown` folders; it is
**disabled by default**, so a consumer that never reads inventory issues no
folder fetches and pays nothing. While enabled, the runtime drives the crawl by
calling `Session::next_inventory_fetch_batch(max_in_flight)` each tick — it
returns the next batch of folders to fetch (bounded by `max_in_flight` minus
those already in flight, `INVENTORY_FETCH_MAX_IN_FLIGHT` being the conventional
bound) and flips each to `Fetching`. The shell POSTs a
`FetchInventoryDescendents2` per folder; each reply folds in, flips the folder
`Loaded`, and seeds its children `Unknown` for the next sweep.
`Session::inventory_fully_loaded(owner)` is the completion signal — true once no
folder of that tree is `Unknown` or `Fetching`.

The explicit pulls work regardless of the flag:
`Session::request_folder_contents(id)` (UDP) and
`Command::FetchInventoryFolders(ids)` (CAPS) fetch on demand.

## The Command / Event pull-bridge

The channel-based runtimes (the tokio client, and any consumer talking to the
`Session` over a channel rather than borrowing it) cannot hold a `&Session` to
walk the borrowed surface, so the held model is exposed over the
`Command`/`Event` bridge. These queries are answered **locally** — they
synthesize a reply from the in-memory model with no wire send:

- `Command::QueryInventoryRoots` → `Event::InventoryRoots { agent_root,
  library_root }`. Both keys are `Copy`, so the reply is trivially cheap.
- `Command::QueryInventoryFolder { folder, before, limit }` →
  `Event::InventoryFolderPage { folder, folders, items, prev }`, built from
  `inventory_folder_page`. If the folder's contents are still `Unknown` the
  runtime also schedules its on-demand fetch, so a later query sees them loaded.

The page payloads are `Arc<[FolderInfo]>` / `Arc<[ItemInfo]>` — an `Arc` clone
crosses the channel, never a deep copy of the window, keeping the copy budget to
a refcount bump. A bevy reader skips the bridge entirely and borrows `&Session`
to call `inventory_children` / `inventory_folder_page` / the root accessors
directly (zero copy).

## The disk cache

The held model is persisted between runs in a **Firestorm-compatible** on-disk
cache so a returning client need not refetch the whole tree. Each tree is one
file under the per-account cache directory: `<agent-uuid>.inv.llsd.gz` for the
agent's own inventory and `<agent-uuid>.lib.inv.llsd.gz` for the Library. The
format matches Firestorm exactly — a 4-byte big-endian version header
(`INVENTORY_CACHE_VERSION`, currently 5, = its `sCurrentInvCacheVersion`)
followed by a gzipped binary-LLSD `{ categories, items }` map. Only fully
`Loaded` folders and their items are written.

Loading is **version-gated and reconciled against the skeleton**. A file whose
header is not the expected version is treated as cold and ignored, forcing a
full refetch. Otherwise the runtime loads the cache *before* the login skeleton
arrives (marking every cached folder `Loaded` at its stored version), then
`merge_skeleton` reconciles the two: a cached folder whose skeleton version
matches keeps its contents and stays `Loaded`; a version mismatch (or a skeleton
folder missing from the cache) drops the stale contents and marks it `Unknown`
for refetch; a cached folder the skeleton no longer lists was deleted
server-side and is removed with its subtree.

The cache is **grid-level** — it is the avatar's whole inventory, not a
region's, so it survives teleports and region crossings untouched; only
login/logout and the dirty/idle persist tick load and store it. The directories
come from `ClientDirectories` (`agent_cache_dir`, supplied once at runtime
construction); the whole feature is opt-in via `InventoryCacheConfig` (**default
off**, with an independent `cache_library` toggle for the large, rarely-changing
Library file). The sans-I/O `Session` only (de)serialises bytes — the runtime
shells own the gzip envelope and the atomic temp-and-rename write.

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
>   (`FetchInventoryDescendents2`), `CAP_FETCH_LIBRARY` (`FetchLibDescendents2`,
>   the read-only shared Library fetched with the Library owner id),
>   `CAP_INVENTORY_API_V3` (`InventoryAPIv3`), `CAP_LIBRARY_API_V3`
>   (`LibraryAPIv3`), `CAP_CREATE_INVENTORY_CATEGORY`, in
>   `sl-proto/src/session.rs`. The AIS3 URL/body helpers are in
>   `sl-wire/src/inventory.rs` (`ais_category_url`,
>   `build_ais_update_item_body`, …).
> - The agent tree and the shared Library tree are held in one model
>   (`sl-proto/src/session/inventory.rs`) tagged by `InventoryOwner` (`Agent` /
>   `Library`), each with its own root and owner (`Session::inventory_root` /
>   `library_root` / `library_owner`) and its own version-gated disk cache; a
>   Library descendents reply folds back under `InventoryOwner::Library`, and no
>   mutation command targets the Library. The per-folder contents state is
>   `FolderState` (`Unknown` / `Fetching` / `Loaded { version }`);
>   `merge_skeleton` reconciles a loaded cache against the login skeleton.
> - The held read API lives on `Session` in `sl-proto/src/session/methods.rs`:
>   `inventory_folder` / `inventory_item` (borrowed), `inventory_children`
>   (the `Child<'_>` zero-copy walk), `inventory_folder_page` (`FolderInfo` /
>   `ItemInfo` / `InventoryCursor` snapshots), `folder_fetch_state`,
>   `inventory_owner`, plus the background-crawl methods
>   `set_background_inventory_fetch` / `next_inventory_fetch_batch` /
>   `inventory_fully_loaded` (`INVENTORY_FETCH_MAX_IN_FLIGHT`). The view types
>   `Child`, `FolderInfo`, `ItemInfo`, `InventoryCursor` are in
>   `sl-proto/src/types/inventory.rs`.
> - The pull-bridge is `Command::QueryInventoryRoots` /
>   `Command::QueryInventoryFolder` (`sl-proto/src/command.rs`) answered by the
>   locally-synthesized `Event::InventoryRoots` / `Event::InventoryFolderPage`
>   (`sl-proto/src/types/event.rs`, `Arc<[…]>` payloads).
> - The disk cache: the pure (de)serialise/merge core is
>   `sl-proto/src/session/inventory_cache.rs` (`INVENTORY_CACHE_VERSION`); the
>   runtime gzip+atomic-write shells are
>   `sl-client-tokio/src/inventory_cache.rs` and
>   `sl-client-bevy/src/inventory_cache.rs`, configured by `ClientDirectories` /
>   `InventoryCacheConfig` (`sl-proto/src/chat_log.rs`).
> - The HTTP fetch driver is `sl-client-tokio/src/inventory.rs`; the worked
>   examples are `sl-client-tokio/examples/inventory_edit.rs` and
>   `sl-client-tokio/examples/inventory_cache.rs`.
> - Events: `InventorySkeleton`, `LibraryInventory`, `InventoryDescendents`,
>   `InventoryItemCreated`, `InventoryBulkUpdate` in
>   `sl-proto/src/types/event.rs`.
