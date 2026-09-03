---
id: test-fake-grid-seated-crossing
title: Crossing a border seated on a vehicle, alone and with other riders
topic: test
status: done
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

## Done (2026-09-03)

Three tests, and the grid work they needed. Cases 1 and 2 are covered;
case 3 (standing on a vehicle) stays out of scope for the stated reason.

### The grid could not seat an agent at all

That came first. `world::answer_world_request` now answers
`ServerEvent::SitRequested` with an `AvatarSitResponse` and, on
`SitConfirmed`, re-sends the agent's own avatar parented to the seat —
which is what a seated avatar *is* on the wire: a `ParentID` and a
position that is the offset from the seat rather than a place in the
region. `StoodUp` puts it back in region space.

The seat offset is a fixed `world::SIT_TARGET_OFFSET`, not the point the
client clicked: a real vehicle sets an `llSitTarget`, so riders snap to
the seat, and a fixture that honoured the click would seat two riders in
one place only by luck.

### The handover primitives

- `FakeAgent::with_world` — mutate a session's fixtures **and** send,
  under one lock. A change that is not sent is invisible; a send the
  fixtures disagree with is undone by the next refetch.
- `FakeAgent::seat_on` — re-send this session's own avatar seated on a
  region-local id, and record the sit grid-side.
- `SimSession::seat_agent` — a destination inheriting an agent that
  arrives already riding something, with no handshake, because there
  isn't one: the sit came with the agent data.

The handover itself stayed a **test helper** rather than a `FakeGrid`
method, per this task's own rule — the only callers are here. If the
timeline's `Action::CrossRegion` wants it, that is when it earns a method.

### The fixture is the interesting part

`border_with_vehicle(side, ridden)` dresses one region for one side of a
border. The vehicle keeps its `ObjectKey` and changes its region-local id
across the line (`0x310` → `0x340`), which is the renumbering a rider's
seat has to survive.

**A bug caught in review:** the first version gave both regions the same
region-local vehicle position, so the two copies stood 256 m apart and the
"handover" was a jump across a whole region rather than a step over a
border. Both tests still passed, because each measured the rider relative
to *its own* vehicle — a pass that hid a scenario that was not the one
being claimed. The fix is the `BorderSide` enum: `Leaving` places the
vehicle against its region's **east** edge, `Arriving` against the next
region's **west** edge, and each side derives its own ids *and* position,
so they cannot be mispaired. `the_vehicle_sits_against_the_border_on_both_sides`
computes the world-space gap and fails if it is not a few metres.

### The tests

- `the_agent_sits_on_a_prim_and_rides_it` — the prerequisite handshake.
- `a_seated_avatar_crosses_with_its_vehicle` — the agent alone.
- `other_riders_cross_on_the_same_vehicle` — with a scripted rider, so a
  seat that was re-found can be told from *every* seat being re-found.
- `a_rider_stays_on_its_vehicle_across_a_border` (full-stack) — the claim
  `Session::seat()` cannot make: the body is still **drawn on the deck**.
  Measured as the avatar's pixel offset from its vehicle, read from the
  anchor's transform (where the viewer *put* it) rather than from the
  wire. The camera is re-aimed between captures, and must be — the subject
  rides over the border, so a fixed world pose would be looking at where it
  used to be. The relative measure is what the re-aiming leaves alone.

### Two things about waiting, both found the hard way

**A wait consumes what it reads.** `other_riders_cross_on_the_same_vehicle`
first waited for the neighbour marker and then for the rider's update —
but the marker *closes* the child burst, so the rider's update had already
gone past, and the test hung rather than failed.

**Messages from two simulators have no order at all.** The handover kills
the vehicle on the source circuit and re-seats the rider on the
destination's; UDP is ordered per circuit and says nothing across
circuits, so `wait_for_the_handover` watches for both halves in one pass.

Every multi-observation wait in these tests now accumulates in a single
pass, and everything added here uses `wait_until` (one deadline for the
whole wait) rather than `wait_for` (whose per-event timeout never fires
while pings keep arriving, so a missing message hangs instead of failing).
`wait_until`'s doc now says all of this.

### Verified by mutation

Sending a seated avatar without its `ParentID`, and having the destination
rez an arriving rider as if it were standing, each fail the tests that
claim otherwise — and after the wait rework they fail in ten seconds with
the event tail, where before the rework one of them hung past sixty.
