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

## Which OpenSim engine `FakeOpensim` runs (gridspec, 2026-10-26)

The probe corpus measures OpenSim under both of its engines
(`probes/results/opensim-yengine/` and `opensim-xengine/`, the engine picked
per script by its `//YEngine:` / `//XEngine:` first line), but the fake grid
has one OpenSim flavour and one results directory, `fake-opensim`. The
gridspec tasks so far model `FakeOpensim` on **YEngine**, OpenSim's default
(e.g. [[server-lsl-memory-and-limits]]), and record XEngine's divergences as
XEngine rows in `book/src/gridspec/lsl.md` — a 0.5 s timer floor, an event
queue without a limit, a faulted script that runs on, a script reset on
take-and-rez, no memory accounting. Decide here whether the wiring takes an
engine knob on `FakeOpensim` (so a fake run can be held to either column) or
stays YEngine-only; a fake run is compared against `opensim-yengine` until
then.

## From server-lsl-runtime-errors (2026-10-28)

The runtime now speaks a script's error lines itself: implement `Host::say`
onto the region's chat fan-out (a `Speech` on `DEBUG_CHANNEL` reaches the
viewer as `ChatType::DebugChannel` from the prim, its owner in the owner
field; `to_scripts: None` means no `listen` hears it) and
`Host::whereabouts` (the prim's name and key, the link set's key for the flood
count, the script's inventory name, the prim's region position). Set
`EngineConfig::flavour` from the grid's flavour: `SecondLife` for a
Second-Life-flavoured grid, `YEngine` (the local OpenSim's engine) for an
OpenSim one.

## From server-lsl-memory-and-limits (2026-10-30)

Three more `Host` methods' worth of wiring, and the region-level view:

- **`Host::owner_say`** tells the object's owner alone (`llOwnerSay`'s
  reach). YEngine's exhausted heap is reported this way, four lines;
  `Whereabouts` now also carries the script's item key (`item`) and the
  region's name (`region`) for them.
- **The top-scripts report.** `LandStatRequest`'s top scripts are answered
  from `ObjectCost` fixtures today, "because a fake region cannot measure".
  Once scripts run, answer it from `Engine::top_scripts` — each script's
  `OBJECT_SCRIPT_MEMORY` figure and its time (`ScriptUsage`), busiest
  first, summed per object — and keep the fixture as the fall-back for an
  object with no scripts, and for a scenario that states a cost it wants in
  the report (the comment on `SceneFixtures::object_costs`).
- **`SimStats`' `ActiveScripts`** is `Engine::active_scripts` (Second Life's
  running average, OpenSim's count of scripts present); the fake grid sends
  `0` today. `llGetObjectDetails`' `OBJECT_SCRIPT_MEMORY`,
  `OBJECT_SCRIPT_TIME` and the script counts belong to
  `server-lsl-lib-prim-state`, from the same `ScriptUsage` and the
  instances' run flags.

## From server-fake-grid-script-compile-on-upload (2026-10-30)

The fake grid compiles every script upload (`sl_fake_grid::script_compile`,
`CapsDispatch::ScriptUpload`) and records what the compile leaves of the
script in the prim's `TaskInventory`: `running` as the upload asked after a
clean compile, removed after a failed one, and on the OpenSim flavour the
item in `uncompiled` (no instance, `GetScriptRunning` unanswered). It keeps
no `Program`: nothing runs it. Once the engine is wired:

- **A save is the recompile, and the instance restarts.** A clean upload
  replaces the item's instance with one of the new program and runs
  `state_entry` when the upload asked it to run (`v3_again` in the census:
  the same source over a running script restarts it on every grid); a
  failed one ends the instance — stopped on Second Life, gone on OpenSim.
  `answer_upload` is the place: it has the compile result and the world.
- **Compile once.** `answer_upload` compiles with
  `sl_lsl_runtime::compile_for`; hand that `Program` to the engine rather
  than compiling the stored source a second time, and compile a fixture's
  script (which nobody uploaded) when its prim is loaded.
- **Run the `script-compile` census offline.** Its legs listen to the script
  they upload (`*_heard`, `*_after_running`), which is why it is live-only
  today; its tests hold the fake grid's compile answers meanwhile. Once
  scripts run, add it to `OFFLINE_CASES` for both flavours.
