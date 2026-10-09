---
id: gridspec-task-inventory
title: Task inventory reads and writes on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, gridspec-object-properties,
  test-phase-z-deferred-04,
  viewer-task-inventory-open-and-save-back, test-asset-save-mutation-survey]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

`RezScript` works on both grids (the long-standing "SL drops it" was the
harness scripting a stranger's prim, [[test-phase-z-deferred-04]]); SL leaves
`group_owned` out of a contents listing unless it is true and needs the
listing's `Xfer` to name remote path `LL_PATH_CACHE`; current viewers fetch the
listing over the `RequestTaskInventory` capability instead
([[protocol-request-task-inventory-cap]]). OpenSim mints a fresh item id on
copy-in. The fake grid ignores `RezScript`,
`MoveTaskInventory`, `RemoveTaskInventory`.

The serial-push half of an `UpdateTaskInventory` drop is measured
([[gridspec-object-properties]], `book/src/gridspec/objects.md` § Properties,
2026-10-09): Second Life sends the record with its advanced serial to every
session holding the prim selected, the writer only if it is one; OpenSim to
the writer alone, selected or not. The drop itself lands on Second Life. Left
for this task: the same question for `RezScript`, a remove and a move.

## Discover

Run `task-inventory` on both grids; add remove / move-to-agent legs; the
`UpdateTaskInventory` drop on SL; the serial-push half
with a second avatar.

## Document

`book/src/gridspec/building.md` § Contents.

## Fake grid

Small in this task: `RezScript`, remove, move; an SL drop imitation once the
cause is known.

## Viewer

The Contents tab survives a write that does not land.

## Capabilities done in this task

[[protocol-request-task-inventory-cap]] (task inventory over
`RequestTaskInventory`) and [[protocol-cap-get-metadata]] (a script's
experience over `GetMetadata`). Also the protocol halves of
`UpdateGestureTaskInventory` (both grids; the editor is
[[viewer-gesture-management-ui]]) and `UpdateMaterialTaskInventory` (Second
Life; the editor half is [[viewer-material-save-to-object]]).
