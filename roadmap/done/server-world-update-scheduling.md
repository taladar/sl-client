---
id: server-world-update-scheduling
title: Batch world changes into per-tick update bursts
topic: server
status: done
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 8
blocked_by: [server-world-heartbeat, server-world-ecs-store, gridspec-circuit,
  gridspec-object-update-stream]
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
detection and emits, per session:

- a **terse** update for entities whose only change is placement /
  velocity / rotation / angular velocity — which is the common case and
  the one the viewer's dead-reckoning is built for;
- a **full** `ObjectUpdate` for a first sighting or a change to anything
  the terse form cannot carry (texture entry, shape, flags, text,
  particles, name-value);
- `KillObject` for what left;
- all of it **coalesced per entity per tick**, in a stable order
  ([[server-world-determinism-contract]]).

The store [[server-world-ecs-store]] built (`sl_fake_grid::scene_store`) is
a `bevy_ecs` world, and every write to a prim goes through `bevy_ecs`'s
`Mut` (`ObjectStore::get_mut` / `for_each_mut`), so the world's change ticks
already say which prims changed since a tick; spawns and despawns are
`push` and `remove` / `retain`. What is this task's: reading them once a
tick (and clearing them, `World::clear_trackers`), and telling a placement-only
change — terse — from any other, which the `Body` component does not say
on its own (a placement component split out of it, or a comparison in
`ObjectMut`'s `Drop`).

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

What the live grids do with a throttle, and with object state nobody
acknowledged, is measured in `book/src/gridspec/session.md` § Circuits
([[gridspec-circuit]]): OpenSim holds a category to its rate and resends
the same packet until it is acknowledged; Second Life sends less when
asked for less, never retransmits an object update, and sends the state
again as a new packet instead. The fake grid does neither yet — it accepts
a throttle and ignores it, and retransmits whatever it sent.

## Done (2026-11-04)

`sl_fake_grid::updates` brackets the region's script tick
(`scripts::run_tick`): `begin` records every prim written since the last tick
as shown (a session wrote it and told the region itself), and `finish` merges
the tick's announced object changes with the prims the store says were written
during it (`ObjectStore::take_written`, the `Body` change ticks), one entry per
prim, into a single `RegionChange::Burst`. Each prim is compared with a `Shown`
snapshot component of what the region's viewers last saw: placement and state
only → terse, a first sighting or anything else → full, unchanged → not sent
unless announced (a link re-sends its whole set). Each session's watcher sends
one `ObjectUpdate`, one `ImprovedTerseObjectUpdate` and one `KillObject` per
burst, filtered by `seating::shown_to` — the seam for interest management and
the `AgentThrottle`, both still out of scope. A write the grid keeps quiet (a
child's inherited touch flag on Second Life) amends the snapshot so the burst
keeps it quiet too. Session-made changes still publish at once, as before, with
the source rule unchanged.

Tests: the scheduler's unit tests (a prim moved every tick goes out terse once
a tick, three changes in a tick are one full update, a session's change is not
sent again, announcement order and quiet writes) and
`tests/scripts.rs`'s `one_tick_is_one_update_*`, where a script's drop flag and
the region's touch and scripted flags set in one tick reach a real client as one
full `ObjectUpdate` (two before).

No library function could move a prim, so `llSetPos` came with this task
(the rest of [[server-lsl-lib-prim-state]] stays there): `tests/scripts.rs`'s
`a_moving_prim_goes_out_terse_*` moves a prim once a tick (`llSetPos` costs
0.2 s, the region's step there) and the client sees five terse updates for five
moves and no full update, then a move into the ground stops on it.
`sl-lsl-runtime`'s `vm/tests/placement.rs` holds the cap per flavour and the
delay.
