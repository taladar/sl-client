---
id: server-lsl-script-persistence
title: Script state that survives a take, a rez and a region restart
topic: server
status: ideas
origin: LSL-on-the-fake-grid audit (2026-09-20)
blocked_by: [server-lsl-vm-execution, server-lsl-state-and-events]
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
