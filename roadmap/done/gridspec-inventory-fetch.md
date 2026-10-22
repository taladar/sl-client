---
id: gridspec-inventory-fetch
title: Inventory skeleton, fetch caps, AIS3, the library and cache versions on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, protocol-ais3-nested-embedded,
  protocol-ais3-library-cap, protocol-fetch-inventory-items-request,
  server-fake-grid-inventory-persists-across-logins,
  server-fake-grid-inventory-udp-mutations, gridspec-inventory-mutations]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Done (2026-10-21)

The `inventory-fetch` census (1av, live and fake grids; grown over eight
aditi and as many OpenSim runs, the table generated from the last two of
each and held on all four grids) reads the login skeleton against a raw crawl
of both trees, asks the folder fetch, the item fetch, the UDP fetches and
AIS3 raw, and walks a folder through a UDP create, a capability create, an
AIS3 create, a UDP rename, move into the Trash and removal, the AIS3 move and
`MOVE`, an item move both ways, and a relogin
(`book/src/gridspec/inventory.md` § Fetching). Findings:

- The skeleton is the whole folder tree on both grids, at the versions the
  fetch states, and the next login's skeleton states the versions the last
  session read — so a cache keyed on it (`inventory-cache-skip`, passing on
  both) is exact.
- The folder fetch answers a folder nobody has with an empty entry at
  version 0 (no `bad_folders`); the item fetch answers an unknown item with no
  items. Second Life states the agent and every sub-folder and item id as a
  string, OpenSim as UUIDs; the item shapes differ in `metadata`,
  `is_owner_group` and `last_owner_id`. OpenSim lists the Library through the
  agent's capability; Second Life does not. No phantom nil-id folder showed
  in a raw crawl of either tree.
- Second Life answers neither UDP fetch; OpenSim answers both.
- AIS3 nests a level per level and reaches `depth` + 1 levels; every answer
  carries `_base_uri`, `_links` and `agent_id`; errors are an LLSD map naming
  the service's source file, function and line.
- **Second Life's AIS3 moves nothing**: a `PATCH` of `parent_id` is a `400`
  (`Cannot change parent_id.  Use MOVE method.`), folder or item, and `MOVE`
  is a `405`. The UDP `MoveInventoryFolder` / `MoveInventoryItem` work, as the
  reference sends them. A UDP removal steps the Trash's version on Second Life
  and not on OpenSim; a rename steps the parent's, not the folder's.

Viewer: one real bug, fixed. Both runtimes sent folder and item moves to
AIS3 on Second Life, so every move and every delete-to-Trash there changed
only the viewer's own cache (`ais3-folder-lifecycle` had never passed on
aditi; it failed at its first move). Moves now go UDP on every grid, the
explicit `Ais3MoveFolder` / `Ais3MoveItem` commands (and REPL commands) are
removed with a note at each site saying why, and a full-stack test on both
fake flavours
(`a_delete_moves_the_folder_and_the_item_into_the_trash_on_the_grid`) holds
the delete; `ais3-folder-lifecycle` passes on aditi.

Fake grid: an `InventoryDialect` per flavour shapes the folder and item
fetches, answers an unknown folder with the empty entry (and OpenSim's Library
through the agent capability), serves the four UDP folder messages with the
measured version steps and OpenSim's `FetchInventoryReply`; the Second Life
flavour's AIS3 refuses a move as Second Life does, serves `GET
/category/<id>`, dresses every answer with `_base_uri` / `_links` /
`agent_id` and answers an unknown folder with the service's `404` map. The
census is offline on both flavours. Not held on the fakes (each named in the
case's `FAKE_EXEMPT`): the relogin legs
([[server-fake-grid-inventory-persists-across-logins]], new), the item legs
(the UDP copy, [[server-fake-grid-inventory-udp-mutations]]) and the Second
Life flavour's deliberate `FeatureDisabled` for the UDP folder fetch.

The login skeleton entries' field names are now kept by both login codecs
(`LoginSuccess::inventory_skeleton_fields` / `library_skeleton_fields`).

## Known already

SL drops the UDP descendents fetch silently (the fake grid refuses) and nests
AIS3 `_embedded` per depth; OpenSim's library leaves carry a phantom nil
folder.

Since done: the parser folds every nested `_embedded` level
(`gather_ais_embedded` in `sl-proto/src/session/conversions.rs`,
[[protocol-ais3-nested-embedded]]), and both runtimes read the agent tree over
`InventoryAPIv3` and the library over `LibraryAPIv3`, falling back to the
descendents caps and then UDP ([[protocol-ais3-library-cap]]);
`library-tree-fetch` and `inventory-tree-crawl` passed on aditi and OpenSim
on 2026-10-04. The fetch-item and cache-version cases on aditi, and recording
them into the book, are still this task's.

## Discover

Those cases on aditi plus an AIS3 depth probe and an item-fetch probe; skeleton
fields per option; whether versions advance on UDP / other-session changes.

## Document

`book/src/gridspec/inventory.md` § Fetching.

## Fake grid

Small — nested `_embedded` per flavour, OpenSim's phantom folder, version bumps.

## Viewer

No UDP on SL (the nested parser and `LibraryAPIv3` routing are done).
