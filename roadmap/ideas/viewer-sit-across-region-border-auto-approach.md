---
id: viewer-sit-across-region-border-auto-approach
title: Sit on an object across a region border by moving into its region first
topic: viewer
status: ideas
origin: user suggestion while reframing the neighbour-region sit bug (2026-09-17)
refs: [viewer-sit-on-neighbour-object-uses-root-circuit]
---

Context: [context/viewer.md](../context/viewer.md).

The simulator refuses a sit from a child agent, and Firestorm simply relays
that refusal ("Try moving closer. Can't sit on object because it is not in
the same region as you."). The viewer could do better: when the sit target is
in a neighbour region, **get the avatar into that region first**, then send
the sit request there once the crossing has completed.

A sketch of the flow, all of it needing live confirmation:

1. Detect the case up front: the target's circuit
   (`Session::circuit_for_object`) is a child, not the root.
2. **Stand up first** if the avatar is seated in the current region — a seated
   avatar cannot walk across the border, and a crossing while seated on
   something in the old region is its own can of worms.
3. Move to a point just inside the target region, nearest the object: the
   server-side autopilot (`GenericMessage` `autopilot`, the
   `Session` walk-to) or the viewer's own walk, then let the normal crossing
   promote the child circuit to root.
4. On arrival (`CrossedRegion` handled, the target now on the root circuit),
   send `AgentRequestSit` as an ordinary same-region sit.

## Things that decide whether this is worth it

- **Failure paths must end in a clear message**, never a stranded avatar:
  crossing refused (parcel ban / access list / no-entry), no path (a wall, a
  cliff, water the avatar cannot walk), a timeout, the target moved or
  vanished meanwhile, the user moving or clicking during the approach (which
  should cancel it, as the reference's autopilot does).
- **Flying / vehicles:** an avatar already riding a vehicle or flying needs
  its own handling; do not silently drop it out of a vehicle.
- **Surprise:** standing the user up and walking them across a border is a
  bigger action than a click usually implies — likely a confirmation or a
  setting (off by default mirrors the reference).
- Depends on the refusal path itself working first
  ([[viewer-sit-on-neighbour-object-uses-root-circuit]]), which also gives
  the fallback message when the approach is declined or fails.
