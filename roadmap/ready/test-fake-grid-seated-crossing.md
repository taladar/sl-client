---
id: test-fake-grid-seated-crossing
title: Crossing a border seated on a vehicle, alone and with other riders
topic: test
status: ready
origin: review of test-fake-grid-neighbours-crossing (2026-09-03)
points: 8
refs: [test-fake-grid-neighbours-crossing, test-fake-grid-npc-avatars, viewer-seated-region-crossing, viewer-crossing-movement-locks-up]
---

Context: [context/testing.md](../context/testing.md).

[[test-fake-grid-neighbours-crossing]] crosses an avatar **on its own**.
The case that actually breaks in the wild is the other one: an avatar
*seated on a vehicle* that crosses, because then the thing being handed to
the neighbour is an **object** with riders attached to it, and every id
involved changes at the border.

Three cases, in order of what each adds:

1. **Seated, alone.** The agent sits on a prim; the prim crosses; the
   agent crosses with it.
2. **Seated, with other riders.** The same, with one or more NPCs
   ([[test-fake-grid-npc-avatars]]: `NpcFixture::seated_on` already
   models a rider) on the same vehicle, so the test can tell "my seat
   retargeted" from "every seat retargeted".
3. **Standing on the vehicle** is *not* in scope — there is no physics
   here to stand on anything with.

## What the grid does not do yet

There is no **object handover**. A crossing today moves the agent; the
vehicle would simply be an object of the source region that the
destination has never heard of. A real simulator kills the object in the
source and rezzes it in the destination under that region's own id space,
which is the whole reason the viewer's seat retarget is interesting: a
`ScopedObjectId` is `circuit + local id`, so the vehicle's **region-local
id changes across the border** while its grid-wide `ObjectKey` does not.

So this task's grid-side half is: hand an object (and its riders) to the
neighbour — `KillObject` in the source, a full `ObjectUpdate` in the
destination with fresh local ids and the riders re-parented — either as a
`FakeGrid` method beside `cross_agent` or as a scripted sequence a test
drives through `with_sim`. Decide which when the shape is clear; a method
is worth it only if more than this task calls it.

## What to assert

Client side (tokio): `Session::seat()` survives the crossing and names the
**new** scoped id; the sit-implied in-world grants survive (the
`sl-proto` `region_crossing_preserves_seat_and_inworld_grants` invariant,
now against a real grid rather than a loopback pair); each rider's avatar
update carries the new vehicle's `ParentID`; no `TeleportStarted` and no
disconnect.

Pixels (the full-stack tier): the rider is still **on** the vehicle after
the crossing — its projected position relative to the vehicle unchanged —
which is the claim `Session::seat()` alone cannot make. The `border` scene
is the fixture to extend; a vehicle prim near the border, ridden.

## Why it is worth the points

[[viewer-seated-region-crossing]] says its end-to-end check "can only be
exercised on aditi", because OpenSim has no scripted vehicle to sit on and
carry across a border. That is true of a *scripted* vehicle; it is not
true of the handover itself, which is what actually goes wrong. This task
gives that one an offline harness, and gives
[[viewer-crossing-movement-locks-up]] — where a stand-up animation played
after a crossing nobody was sitting through — a place to reproduce a
seat-state bug without a live grid.
