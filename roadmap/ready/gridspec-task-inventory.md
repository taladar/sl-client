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

SL drops our `RezScript` / `UpdateTaskInventory` writes (serial stays 0);
OpenSim mints a fresh item id on copy-in. The fake grid ignores `RezScript`,
`MoveTaskInventory`, `RemoveTaskInventory`.

## Discover

Run `task-inventory` on both grids; add remove / move-to-agent legs; the
SL write path via [[gridspec-lsl-aditi-script-carrier]]; the serial-push half
with a second avatar.

## Document

`book/src/gridspec/building.md` § Contents.

## Fake grid

Small in this task: `RezScript`, remove, move; an SL drop imitation once the
cause is known.

## Viewer

The Contents tab survives a write that does not land.
