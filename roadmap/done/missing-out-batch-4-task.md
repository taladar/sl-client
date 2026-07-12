---
id: missing-out-batch-4
title: task
topic: missing
status: done
origin: MISSING_ROADMAP.md
---

Context: [context/missing.md](../context/missing.md).

**Out batch 4 — task (object) inventory.** `RequestTaskInventory`,
`UpdateTaskInventory`, `MoveTaskInventory`, `RemoveTaskInventory`: read and
mutate the inventory contents of an in-world object (the outbound side of the
inbound batch-6 `ReplyTaskInventory`).

Implemented as `Session::request_task_inventory`,
`Session::update_task_inventory` (taking a `TaskInventoryKey` and a
`&RestoreItem`), `Session::move_task_inventory` (taking a destination
`InventoryFolderKey` and the item's `InventoryKey`), and
`Session::remove_task_inventory`. All four name the target prim by its
[`ScopedObjectId`] (the region-local `LocalID` the wire blocks carry, scoped
to its circuit) rather than a bare `u32`, matching the `rez_script` pattern;
`request_task_inventory`'s reply arrives as the already-handled
`Event::TaskInventoryReply`. `UpdateTaskInventory` reuses the same full-item
[`RestoreItem`] payload as `RezScript`/`RezObject` (its `InventoryData` block
is field-for-field identical) instead of 20 raw wire fields, and a new typed
[`TaskInventoryKey`] enum (`Item`/`Asset`, LL's `TASK_INVENTORY_*_KEY`)
replaces the raw `Key` byte. Wired as four new `Command` variants through the
tokio and bevy runtimes, the `command_name` formatter, and the matching REPL
tokens (`update_task_inventory` reuses the existing `restore_item_from_args`
helper and a new `parse_task_inventory_key`). Covered by one pack-the-wire
lifecycle test and four REPL parse tests. Live-testable on OpenSim (task
inventory read/edit/move/remove all work against the local grid).
