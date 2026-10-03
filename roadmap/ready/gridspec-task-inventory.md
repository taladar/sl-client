---
id: gridspec-task-inventory
title: Task inventory reads and writes on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey,
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
