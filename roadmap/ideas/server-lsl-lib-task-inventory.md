---
id: server-lsl-lib-task-inventory
title: Library tranche — task inventory, giving, rezzing and notecards
topic: server
status: ideas
origin: LSL-on-the-fake-grid audit (2026-09-20)
blocked_by: [server-lsl-vm-execution, server-fake-grid-script-engine-wiring,
  gridspec-lsl-task-inventory]
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

## Measured (gridspec-lsl-task-inventory, 2026-10-27)

The tables are `book/src/gridspec/lsl.md` § Task inventory, giving, rezzing
and notecards. What the acceptance above should hold to:

- **Order** is by name with case folded, and Second Life compares runs of
  digits as numbers (`9` before `10`); OpenSim does not. A lookup by name is
  exact (case matters).
- **`llRemoveInventory` raises no `changed` in its own prim on Second Life**
  (OpenSim raises `CHANGED_INVENTORY`); a give into another prim raises it
  there on both. The contents serial must still advance.
- **Notecards**: a final newline starts an empty last line (eleven lines for
  ten and a newline), an empty notecard has one line, line −1 is `EOF`, a
  line is cut at 1024 bytes on a character boundary, and the answer comes a
  frame after the 0.1 s forced delay. A missing notecard says
  `Could not find notecard '…'.` and returns a request key that never
  answers. `llGetNotecardLineSync` is `NAK` until the notecard has been read.
- **Rezzing**: `object_rez` reaches the rezzer before the child's report
  even though the child's `on_rez` ran first; the child runs `on_rez` alone
  (no `state_entry`), keeps its taken state, and is not `DIE_AT_EDGE`
  (OpenSim sets it). Past 10 m, or a notecard's name, rezzes nothing,
  silently. The forced delay is 0.1–0.2 s; twenty in a row all rezzed.
- **Giving**: a name the target holds already gets `Gift 1`, `Gift 2`; a given
  script arrives stopped; `llGiveInventoryList` to a prim gives the items one
  by one; a wrong `llRemoteLoadScriptPin` says `Task '<name>' trying to
  illegally load script onto task '<name>'!`.
- **`llResetOtherScript` of a stopped script** leaves it where it was:
  started again, it carries on without `state_entry` (OpenSim runs it).

## Measured (gridspec-lsl-throttles, 2026-10-28)

No rez throttle on either grid at 300 `llRezObject` calls in six waves of
fifty: Second Life rezzed all of them at about 6.3 a second (its 0.1 s forced
delay reads 0.1–0.4 s a call), OpenSim at exactly 10. `llGiveInventory` to an
avatar sleeps 2 s on Second Life and 3 s on OpenSim, where the keyword file
says 0 ([[server-lsl-forced-delays-per-flavour]]).
