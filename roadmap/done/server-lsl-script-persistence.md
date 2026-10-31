---
id: server-lsl-script-persistence
title: Script state that survives a take, a rez and a region restart
topic: server
status: done
origin: LSL-on-the-fake-grid audit (2026-09-20)
blocked_by: [server-lsl-vm-execution, server-lsl-state-and-events,
  gridspec-lsl-persistence]
refs: [server-fake-grid-script-engine-wiring, server-lsl-lib-task-inventory]
---

Context: [context/lsl.md](../context/lsl.md).

On a real grid a script's *running state* is part of the object, not of
the process: take a running vendor into inventory and rez it a week
later and it is still in the state it was in, with its globals, its
timer and its queued events intact. `llResetScript` is how you
deliberately lose that. OpenSim persists it as an XML blob per script
(`ScriptInstance.GetXMLState` / `SetXMLState`) which travels with the
object in an OAR and in an inventory asset.

Whether the fake grid needs this is a genuine question, and the answer
is "less than a real grid, but not none":

- **Not needed** for the common case: a scenario is built fresh from a
  seed on every run, and a script that always starts at `default`
  is more reproducible, not less.
- **Needed** for the take/rez round trip, which the grid already
  supports and already has conformance coverage for
  (`object-rez-derez`, `object-asset-format`): taking a scripted object
  and rezzing it must not silently reset it, or content that keeps
  configuration in globals behaves differently here than on any real
  grid — a divergence a test would attribute to the wrong thing.
- **Needed** for `sl-object-asset`: the object asset XML the grid serves
  for a taken object already carries the task inventory; whether it
  carries script state decides whether our serialiser matches
  OpenSim's, which [[test-fake-grid-served-object-asset-xml]] cares
  about.

The design question to settle: serialise the **VM's own state**
(globals, stack, PC, queue, state index) into a compact form of our own,
or serialise into OpenSim's XML shape so an object taken on the local
grid could in principle be rezzed here. The first is far easier and the
second is the only one that ever gets a cross-check; the recommendation
is the first, with the XML shape read-only and lossy if it is ever
wanted.

Acceptance (when picked up): a running script taken to inventory and
re-rezzed resumes with its globals and its timer; a deliberate
`llResetScript` clears them; and the serialised form round-trips through
a unit test with no grid.

## Measured by gridspec-lsl-persistence (2026-10-26)

`book/src/gridspec/lsl.md` § Persistence is the behaviour to build to, per
flavour. A take and a rez: Second Life keeps globals, state, permissions,
timer, listen and the queue, lets a sleep in progress run its course, keeps a
stopped script stopped and delivers the queued events after `on_rez`; OpenSim
under YEngine the same but ends the sleep at the take and delivers the queue
before `on_rez`; XEngine resets the script and starts the stopped one again.
A region restart (OpenSim only so far): both engines keep globals, state,
permissions, timer, listen and the queue and send `changed(32768)` then
`CHANGED_REGION_START`, no `state_entry` or `on_rez`; YEngine ends the sleep
at the shutdown and keeps the script time, XEngine loses the sleeping handler,
starts the script time again and runs the stopped script again.

## Done (2026-10-31)

A script's running state saves and comes back, in `sl-lsl-runtime`'s
`vm::persistence` (book `simulator/lsl-engine.md`, "As built:
persistence"). The design question is settled the recommended way: the
**VM's own state in a compact form of our own** (`SavedScript::encode` /
`decode`: a mark, a version, the fields in order, floats by their bits), not
OpenSim's XML.

- **`SavedScript`** holds everything of an instance but its program and the
  region's accounting: globals, state, run flag and fault hold, the queue
  with its detected blocks, the body in progress (program counter, frames,
  operand stack), a requested state change, the timer, listens, permissions
  and the questions waiting, the start parameter, the memory limit,
  low-water mark and profile. A sleep and the timer are kept as time left,
  not as ticks. The restore checks it against the program the host hands
  back and refuses (`RestoreError`) a state, function, handler, global type,
  frame or queued event the program does not have, and another engine.
- **The engine does each grid's take, rez and restart**:
  `Engine::take`, `Engine::rez_saved(id, program, saved, away, start)`,
  `Engine::shut_down` and `Engine::start_up(scripts, down)`. Second Life
  keeps everything, counts the time away against the sleep and the timer,
  and runs `on_rez` ahead of the queued events; YEngine ends the sleep at the
  take or shutdown, counts nothing away, and runs `on_rez` behind the queue;
  XEngine resets on the rez (a stopped script running again) and drops the
  body in progress at a shutdown. A restart raises
  `changed(CHANGED_REGION_START)`, after OpenSim's own `changed(32768)` there.
  Second Life's restart is unmeasured (no estate rights on aditi,
  [[gridspec-aditi-test-land]]); it follows the take's rule.

**Proof** (`vm/tests/persistence.rs`): the probes are not replayed — they
read `llGetTime`, `llResetTime`, `llGetUnixTime`, the prim's key and
description and a repeating sensor, none of which runs yet — so the probe's
script with those taken out holds each grid's rows: Second Life's woken
handler after exactly eight seconds with the three away among them, then
`on_rez` with every global, the 1040 grant and its granter, then the three
link messages, the listen heard and the timer ticking on; YEngine's handler
at the next tick, the link messages, then `on_rez`; XEngine's reset of both
scripts; a restart on all three flavours; the timer counting the time away
on Second Life only; a deliberate reset being what loses the globals; the
saved form round-tripping a sleeping script with a queued touch, every value
type and a NaN; and the refusals of a foreign form and a foreign program or
engine.

Handed on:

- **The grid's half** — keeping the encoded state with the taken item and in
  the object asset, the rez, the restart and their timing — to
  [[server-fake-grid-script-engine-wiring]].
- **Script time through a take and a restart** (`llGetTime` counts on in
  Second Life and YEngine, starts again on XEngine's restart) to
  [[server-lsl-lib-time-timers]], which owns `llGetTime`; a script time kept
  in `SavedScript` belongs with it.
- **The probes' stopped script** is stopped with `llSetScriptState`, which
  [[server-lsl-lib-task-inventory]] implements; the tests stop it through the
  host until then.
