---
id: server-world-update-scheduling
title: Batch world changes into per-tick update bursts
topic: server
status: blocked
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 8
blocked_by: [server-world-heartbeat, server-world-ecs-store]
refs: [server-lsl-lib-prim-state, server-world-agent-movement]
---

Context: [context/lsl.md](../context/lsl.md).

Today one mutation is one message. A session that edits an object
publishes a `RegionChange::Updated(Box<Object>)` and every other
session's watcher turns it into a **full** `ObjectUpdate`. That is right
for a viewer-driven edit, where changes arrive at human speed and each
one is a whole record anyway. It is wrong the moment a script runs:

- `llSetPos` in a loop, `llTargetOmega`, a moving vehicle or a
  particle-emitting prim produce changes per tick, and a full
  `ObjectUpdate` per change is two orders of magnitude more bytes than
  the `ImprovedTerseObjectUpdate` a real simulator sends for a move;
- a script that sets position, then rotation, then colour in one event
  handler should produce **one** update, not three;
- and there is no ordering guarantee at all today between a change and
  the tick it belongs to.

Wanted: the heartbeat collects the tick's changes from the store's change
detection ([[server-world-ecs-store]]) and emits, per session:

- a **terse** update for entities whose only change is placement /
  velocity / rotation / angular velocity — which is the common case and
  the one the viewer's dead-reckoning is built for;
- a **full** `ObjectUpdate` for a first sighting or a change to anything
  the terse form cannot carry (texture entry, shape, flags, text,
  particles, name-value);
- `KillObject` for what left;
- all of it **coalesced per entity per tick**, in a stable order
  ([[server-world-determinism-contract]]).

Two existing behaviours must survive: the properties push stays a
*subscription* (it goes only to sessions whose selection names the
object, which `driver.rs` already gets right), and the change stream's
`source` rule stays — the session that made a change is not sent it
again, because it was told in the same breath.

Out of scope here, but the shape must not preclude it: per-agent
interest management (cull by distance, prioritise by angular size) and
the client's `AgentThrottle`, which the grid already receives and
ignores. Leave the seam.

Acceptance: a script moving a prim every tick produces one terse update
per tick per session and no full updates; a script setting three
properties in one handler produces one full update; and the offline
conformance cases that count updates still pass.
