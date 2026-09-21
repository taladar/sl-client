---
id: server-lsl-lib-task-inventory
title: Library tranche — task inventory, giving, rezzing and notecards
topic: server
status: ideas
origin: LSL-on-the-fake-grid audit (2026-09-20)
blocked_by: [server-lsl-vm-execution, server-fake-grid-script-engine-wiring]
refs: [server-lsl-state-and-events, test-fake-grid-rez-from-inventory]
---

Context: [context/lsl.md](../context/lsl.md).

What a prim carries and what it does with it. The fake grid has the
store already — `SceneFixtures::task_inventories` keyed by region-local
id, with the contents serial that is "the whole observable", plus
`RequestTaskInventory` / `UpdateTaskInventory` / `RezScript` /
`MoveTaskInventory` / `RemoveTaskInventory` handling and a rez-from-
inventory path ([[test-fake-grid-rez-from-inventory]]). This tranche is
the script-facing half of it.

- **Reading**: `llGetInventoryNumber`, `llGetInventoryName`,
  `llGetInventoryType`, `llGetInventoryKey`, `llGetInventoryCreator`,
  `llGetInventoryAcquireTime`, `llGetInventoryPermMask`,
  `llSetInventoryPermMask`. The `INVENTORY_*` type constants and the
  *ordering* rule (alphabetical within a type) are observable.
- **Writing**: `llRemoveInventory`, `llAllowInventoryDrop`,
  `llSetRemoteScriptAccessPin` / `llRemoteLoadScriptPin` — and every one
  of them must advance the contents serial, or a viewer keeps a stale
  listing, which is precisely the contract `TaskInventory::write`
  already enforces by being the only way in.
- **Giving**: `llGiveInventory`, `llGiveInventoryList`. To an avatar
  this is an inventory offer over IM (the grid already models offers);
  to an object it is a drop into its task inventory, gated by
  `llAllowInventoryDrop`, raising `changed(CHANGED_INVENTORY)`.
- **Rezzing**: `llRezObject`, `llRezAtRoot`, `llRezObjectWithParams`,
  the `object_rez(key id)` event on the rezzer and
  `on_rez(integer start_param)` on the rezzed object, with
  `llGetStartParameter` reading it. `llDie` is the inverse and is the
  one function that must leave the VM immediately and never return.
- **Notecards**: `llGetNotecardLine`, `llGetNumberOfNotecardLines`,
  `llGetNotecardLineSync`, each answering over the `dataserver` event
  with a request key. `sl-notecard` already decodes the asset format;
  the async request/response shape is modelled on OpenSim's
  `Plugins/Dataserver.cs`.
- **Scripts in the inventory**: `llResetOtherScript`,
  `llGetScriptState`, `llSetScriptState`,
  `llGetScriptName`, `llScriptDanger`, `llRemoteLoadScriptPin`'s
  receiving half.

Acceptance: a scripted rezzer rezzes a prim carrying its own script,
passes it a start parameter, and the rezzed script reports it back on
chat; a notecard read returns its lines over `dataserver` in order; a
`llRemoveInventory` advances the serial and the viewer's Contents
floater refreshes.
