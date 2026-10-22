---
id: server-fake-grid-inventory-udp-mutations
title: Fake grid — the UDP inventory mutations, Trash gating and pushes
topic: server
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, gridspec-inventory-mutations]
blocked_by: [gridspec-inventory-mutations]
---

Context: [context/server.md](../context/server.md).

## What

About eight UDP inventory mutation messages are dropped. Decode and apply them,
gate purge / remove on Trash, push what each grid pushes, refuse what SL
refuses.

## How

Implement the behaviour the discovery task measured and the book's
`gridspec` chapter records, per `ImitatedGrid` flavour (a row in
`sl-fake-grid/src/imitates.rs` for every divergence), and hold it to the
measurement with the discovery task's conformance cases run against
`FakeSl` and `FakeOpensim`. See `roadmap/context/gridspec.md`.

## Since done (2026-10-21)

`gridspec-inventory-fetch` serves the four UDP folder messages —
`CreateInventoryFolder`, `UpdateInventoryFolder`, `MoveInventoryFolder`,
`RemoveInventoryFolder` — with the version steps both grids were measured
giving, per `InventoryDialect` (a removal steps the Trash on Second Life and
not on OpenSim), because the client now moves folders and items over UDP on
every grid: Second Life's AIS3 refuses a move (`book/src/gridspec/inventory.md`
§ Fetching). Still dropped: `CopyInventoryItem`, `ChangeInventoryItemFlags`,
`RemoveInventoryItem`, `RemoveInventoryObjects`, `PurgeInventoryDescendents`.
The `inventory-fetch` census's item legs need the copy (its `FAKE_EXEMPT`
rows name this task). Trash gating and pushes are unmeasured.

## Measured (2026-10-22)

`gridspec-inventory-mutations` measured every operation on both grids
(`book/src/gridspec/inventory.md` § Changing inventory). The census
`inventory-mutations` runs on the live grids only; this task adds
`Grid::FakeSl` / `Grid::FakeOpensim` to its `grids()`, a `FAKE_EXEMPT` list for
what stays out, and the case to the offline list. A run of each flavour
against the census on 2026-10-22 differed from its live grid in:

- **`CopyInventoryItem`**: not served. OpenSim answers
  `UpdateCreateInventoryItem` (UDP, callback id echoed), Second Life
  `BulkUpdateInventory` over the event queue; the folder +1.
- **`LinkInventoryItem` over UDP**: not served. OpenSim makes it
  (`UpdateCreateInventoryItem`, no callback id, the folder +1) in any folder,
  the Current Outfit Folder included; Second Life refuses it in every folder
  with the `AlertMessage` "Cannot create requested inventory.".
- **Folder versions after item operations**: a create, a copy, an
  `UpdateInventoryItem` and a link step the item's folder by 1 on both grids
  (the fake steps none); `ChangeInventoryItemFlags` steps it on Second Life
  only and applies nothing on either (OpenSim has no handler).
- **The answer to `UpdateInventoryItem`**: nothing on both grids; the fake
  answers `UpdateCreateInventoryItem`.
- **Masks of a created item**: OpenSim `0x0009e000`, Second Life `0x7fffffff`
  (the fake: `0x7fffffff` on both); a notecard made on Second Life names no
  asset.
- **Trash gating**: `RemoveInventoryFolder` and `PurgeInventoryDescendents`
  of a folder outside the Trash are ignored on both grids (the fake removes
  the folder); `RemoveInventoryItem` removes anywhere (item folder +1, or the
  Trash +1); `RemoveInventoryObjects` outside the Trash removes the item and
  leaves the folder; a purge in the Trash steps the folder by 1. Nothing is
  pushed after any removal.
- **`MoveInventoryItem`**: both grids push `BulkUpdateInventory` over the
  event queue naming the item.
- **Gives** (OpenSim): B's gift is filed in A's Notecards; A's no-copy copy is
  gone at offer time when given back; A hears `InventoryAccepted`. The fake
  relays no offer between two of its accounts in the census.
- **AIS3** (Second Life flavour): every answer carries the object at the top
  level with `_base_uri` / `_links` (the fake's carry only the `_` keys); a
  create names `_created_items` [] too; a rename's
  `_updated_category_versions` names the parent, not the folder; an item
  update names `_updated_items`; removals and purges add
  `_active_gestures_removed`, `_attachments_removed`, `_broken_links_removed`,
  `_wearables_removed` (and `_total_items_removed`); a slam (`PUT
  /category/<id>/links`, a bare array) is `201` — the fake answers `405`; a
  `COPY` of a Library folder is served. A `POST` naming a `category_id`
  gets another id.

## Done (2026-10-22)

Both fake flavours now pass the `inventory-mutations` census, which runs
offline on `FakeSl` and `FakeOpensim` and holds every metric but the gives
(`FAKE_EXEMPT`: the fake relays no inventory offer between two of its
accounts, `server-fake-grid-inventory-offers`). A run of each flavour differs
from its live grid's last run in nothing else.

- **`SimSession`** serves `CopyInventoryItem` (an `UpdateCreateInventoryItem`
  with the callback id on OpenSim, a `BulkUpdateInventory` over the event
  queue on Second Life), `LinkInventoryItem` (made on OpenSim, refused with
  the alert on Second Life), `ChangeInventoryItemFlags` (the folder steps on
  Second Life only), `RemoveInventoryItem` anywhere, and
  `RemoveInventoryFolder`, `PurgeInventoryDescendents` and the folders of
  `RemoveInventoryObjects` only in the Trash or the Lost and Found
  (`SimInventoryTree::is_removable`, OpenSim's `ParentIsTrashOrLost`). A
  `MoveInventoryItem` pushes the moved items over the event queue on both.
  The five messages are typed now, out of the raw-forward ledger.
- **Item creation and update**: a made, copied or linked item steps its
  folder (`SimInventoryTree::add_item`); a made item's masks are the grid's
  (`InventoryDialect::created_item_mask`); a notecard made on Second Life
  names no asset; an `UpdateInventoryItem` that binds no upload steps the
  folder and is not answered.
- **AIS3** answers in Second Life's shape: `201` for a create, a link, a slam
  and a copy, the folder or item at the top level, every `_` key stated; a
  rename steps the parent; a whole-item `PATCH` sets the next-owner mask
  (`AisItemUpdate::next_owner_mask`); `PUT /category/<id>/links` slams
  (`parse_ais_slam_body`, `SimInventoryTree::slam_links`); `COPY` on
  `LibraryAPIv3` copies a Library folder into the `Destination`
  (`CapsRequest::destination`). The fixture Library gained a small
  sub-folder for it, which also closed the `inventory-fetch` census's
  `library_crawl_category_keys` exemption; its three copy exemptions went
  with this task.
- **The folder fetch** lists each item link's target ahead of the folder's
  items, as both grids do.
