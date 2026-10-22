---
id: gridspec-inventory-mutations
title: Inventory item and folder operations and their pushes on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-inventory-service,
  server-fake-grid-inventory-udp-mutations, gridspec-inventory-offers,
  test-asset-save-mutation-survey]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Done (2026-10-22)

The `inventory-mutations` census (2av, aditi and OpenSim) sends every item and
folder operation raw — UDP with the circuit probe on, AIS3 with `reqwest` —
reads each back through the folder and item fetches, and records what arrived
after it by name and road (`book/src/gridspec/inventory.md` § Changing
inventory). Held to the last two runs of each grid. Findings:

- UDP item operations behave alike on both grids, but for the answers:
  Second Life answers a copy with `BulkUpdateInventory` over the event queue,
  OpenSim with `UpdateCreateInventoryItem`; an `UpdateInventoryItem` is
  answered by nothing on either. `ChangeInventoryItemFlags` applies on
  neither (OpenSim has no handler). A created item's masks differ
  (`0x7fffffff` / `0x0009e000`), and a notecard made on Second Life names no
  asset, which neither road links to.
- **Second Life refuses the UDP `LinkInventoryItem` in every folder**, not
  only the Current Outfit Folder; OpenSim takes it anywhere.
- The Trash gates a folder removal and a purge on both grids, an item removal
  on neither; `RemoveInventoryObjects` outside the Trash removes the item and
  leaves the folder. Nothing is pushed after a removal.
- AIS3 removes and empties outside the Trash; every answer carries the object
  and `_created_…` / `_updated_…` / `_removed_…` keys and
  `_updated_category_versions` (a rename's names the parent); nothing is
  pushed after any of them. **AIS3 mints a new folder's id and ignores one the
  request names.** A whole-item `PATCH` — the reference's `asLLSD`, unchanged
  `parent_id` included — is taken.
- A no-copy give and give-back (OpenSim): the giver's copy is gone at offer
  time. On aditi the primary account files no accepted gift at all, in the
  census and in `give-inventory` with its roles swapped — handed to
  [[gridspec-inventory-offers]].

Viewer (a scope change the user asked for: AIS3 wherever the reference uses
it): on Second Life both runtimes now create folders (`CreateInventoryFolder`,
`CreateInventoryCategory`), rename them, update items (the whole item, as the
reference) and remove `RemoveInventoryObjects`' folders and items through
AIS3; creates, copies, flags and moves stay UDP, as the reference has them.
Because AIS3 mints a folder's id, a create is answered on every grid with the
new `Event::InventoryFolderCreated { requested, folder }` (the session matches
an AIS3 answer to its request by parent and name), the viewer's deep copy goes
a level at a time on it and New Folder starts its rename on it (a choice the
user made over keeping the UDP create). `sl_proto::ais_item_update_body` and
`update_inventory_item_message` are new; the fake grid's AIS3 `PATCH` now
refuses only a *changed* `parent_id`. Three full-stack tests hold the create,
the rename and update and the delete on both fake flavours. Census cases that
measure a UDP or capability road (`inventory-fetch`) send it raw now, and
those that chain on a new folder (`inventory-item-ops`, `upload-http`) wait
for its id (`support::create_folder`).

Fake grid: large — [[server-fake-grid-inventory-udp-mutations]] (now ready,
with the measured differences); the census runs on the live grids until it
holds the fakes. The wearable save's item update is still UDP on Second Life
where the reference uses AIS3: [[test-asset-save-mutation-survey]].

## Known already

Purge / remove are Trash-gated; OpenSim UDP mutations get no reply; SL rejects
UDP links into the COF; the fake grid drops about eight UDP mutation messages.

Since `gridspec-inventory-fetch` (2026-10-21, `book/src/gridspec/inventory.md`
§ Fetching): Second Life's AIS3 refuses every move (`PATCH` of `parent_id`,
folder or item, `400`; `MOVE`, `405`) and the client now moves over UDP on
every grid; UDP folder create, rename, move and removal work on both grids
(the version steps are in the chapter) and the fake grid serves them; an AIS3
`DELETE` of a folder outside the Trash removes it on Second Life. Still
unmeasured: the pushes after each mutation, the Trash gating of a UDP remove
or purge, and the AIS3 mutation replies' own shapes.

## Discover

`ais3-folder-lifecycle`, `inventory-item-ops` on aditi, recording the reply /
push shape per op; server-pushed removals after an object give.

## Document

`book/src/gridspec/inventory.md` § Changing inventory.

## Fake grid

Large — [[server-fake-grid-inventory-udp-mutations]].

## Viewer

AIS on SL, UDP on OpenSim; check SL accepts the UDP ops the viewer still sends.

## Seen on the way (2026-10-12)

[[gridspec-task-inventory]] made a notecard, a script, a gesture and a
material with `CreateInventoryItem` on both grids and recorded two things it
does not hold the fake grid to. An item made on OpenSim has base and owner
masks `0x0009e000`, on Second Life `0x7fffffff`; the fake grid gives
`0x7fffffff` on both flavours. And a script made on Second Life names **no
asset** until it is saved, where OpenSim's — and both fake flavours' — has
one from the start.
