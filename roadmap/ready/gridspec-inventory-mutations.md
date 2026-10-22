---
id: gridspec-inventory-mutations
title: Inventory item and folder operations and their pushes on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-inventory-service]
---

Context: [context/gridspec.md](../context/gridspec.md).

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
