---
id: server-fake-grid-script-engine-wiring
title: Wire the script engine into the fake grid
topic: server
status: blocked
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 8
blocked_by: [server-lsl-vm-execution, server-world-heartbeat,
  server-world-ecs-store]
refs: [server-fake-grid-script-compile-on-upload, server-lsl-lib-comms,
  test-fake-grid-lsl-offline-cases]
---

Context: [context/lsl.md](../context/lsl.md).

The join. The runtime crate knows nothing about prims (it talks to a
`Host`); `sl-fake-grid` knows nothing about scripts. This task
implements the `Host` over the region world and gives script instances a
life cycle.

**Life cycle.** A script instance exists for a
`(entity, task inventory item)` pair whose item is an
`AssetType::ScriptText` (`LSL`). It is created when the item appears —
a scenario fixture, a `RezScript`, a `MoveTaskInventory` drop, a rez
from inventory, a take-and-re-rez — compiled from the asset, and started
if its run flag says so. It is destroyed when the item is removed, the
object is derezzed or taken, or the region drops. `llDie` destroys the
whole object from inside one of its scripts, which is the awkward case:
the VM must return control before the entity disappears under it.

**The client commands that already arrive and currently do nothing:**

- `ServerEvent::SetScriptRunning` — start/stop, answered by a
  subsequent query rather than a reply;
- `ServerEvent::ResetScript` — `llResetScript` from outside;
- `ServerEvent::RequestScriptRunning` — answered with
  `send_script_running_reply`, which already exists, but today with a
  fabricated answer rather than a real one;
- `ServerEvent::RezScript` — the Contents floater's "New Script", which
  on OpenSim fills a default body **and starts it**, a behaviour
  `script-running`'s conformance case already depends on;
- `ServerEvent::RemoveTaskInventory`, `UpdateTaskInventory`,
  `MoveTaskInventory` — each of which must stop, start or recompile.

**The `Host` implementation** is the bulk: every world-touching library
call lands here. Keep it a thin translation onto the store and the
region services — a `Host` method with logic in it is a library
function in the wrong crate.

**Ownership and locking.** Script state belongs to the region, not to a
session: a script keeps running when its owner logs out, and two
sessions in one region see one script. So instances live beside the
region world, and the heartbeat runs them under the region lock with
outbound messages collected and sent after release — the same discipline
`flush_locked` / `finish_flush` already impose on the session side.

Acceptance: a fixture prim carrying a script in the `catalogue` scenario
runs it on login; the Contents floater's Running checkbox reflects and
controls the real run state; Reset restarts it; `llDie` removes the
object and the viewer sees the `KillObject`; and the region survives a
script that throws.
