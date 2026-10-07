---
id: gridspec-neighbours-crossing
title: Child agents, EnableSimulator and region crossing on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, test-handover-distant-and-vehicle-aditi,
  viewer-seated-region-crossing, gridspec-logout,
  protocol-neighbor-discovered-repeats, gridspec-seated-crossing,
  viewer-retired-neighbour-stays-drawn, server-fake-grid-teleport-edges,
  gridspec-sit-stand, server-world-agent-movement, server-agent-transfer]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Done (2026-10-07)

Measured and written up in `book/src/gridspec/teleport.md`, *Neighbours and
crossings*.

- **Discover.** Seven scripted `sl-repl` runs with the trace log on (five on
  aditi, two on OpenSim) read the transports and the event bodies; three
  cases then hold each grid to them. `neighbour-child-circuits` and
  `region-crossing` were fake-grid-only and now run on all four grids;
  `draw-distance` was rewritten to step the distance (100 m, 32 m, 512 m)
  and watch the neighbours go and come back. A shared
  `sl_conformance::crossing` watches the announcements and walks an avatar
  at the nearest border a neighbour shares, and back. All three ran on aditi
  and OpenSim and run offline on both fake flavours.
- **Findings.** Both grids announce over the event queue and retire with a
  `DisableSimulator` down the child circuit, and both decide which
  neighbours to hold from the draw distance — differently. OpenSim adds 64 m
  to it, clamps to 96–255 m, and holds every region a square of that
  half-width around the avatar touches, answering a change within a second.
  Second Life holds a region sharing an edge at 128 m and one touching at a
  corner at 128·√2 m whatever the avatar's position, announces within a
  second and retires **fifty seconds** late. Second Life names all its
  neighbours in one batch and repeats itself (each edge neighbour's
  `EnableSimulator` every minute, the corner neighbour's seed every five
  seconds); OpenSim names one every half second, once. A `CrossedRegion`
  comes over the event queue on both; OpenSim's states the region size and
  a zero `LookAt` at a walk, Second Life's states no size and the direction
  of travel, and its destination greets the agent with a third
  `RegionHandshake`. Neither grid kills the crossing avatar's object on the
  region it left for its own viewer. Second Life's child circuits carry the
  avatars standing in the neighbour, their `ViewerEffect`s included.
- **Client.** `Event::NeighborRetired` (with a `NeighborRetirement` reason),
  which answers the question the logout task left here: the region index
  needed an event, and `sl-client-bevy` now drops a retired neighbour and
  its parcel overlay. A repeated `EnableSimulator` or a seed already held
  raises no event ([[protocol-neighbor-discovered-repeats]], fixed here).
  `ViewerEffect` is read on a child circuit.
- **Fake grid.** `ImitatedGrid::neighbour_policy`: each flavour's
  draw-distance rule and retire delay, driven by the `Far` of the client's
  `AgentUpdate`s; whether an announcement and a crossing state the region
  size; what a crossing's `LookAt` holds; and the handshake Second Life's
  destination sends as a crossing arrives.
- **Viewer.** The full-stack border-crossing check (the scene stays where it
  was and stays drawn) runs against both flavours.
- **Not done here.** The **seated crossing**, on either grid: it needs a
  script uploaded into a prim and an avatar seated on it, and aditi has yet
  to answer a sit request ([[gridspec-seated-crossing]], blocked by
  [[gridspec-sit-stand]]). A crossing that leaves a neighbour out of view,
  a crossing at a corner, and an agent that never completes its movement
  were not provoked: both blocks measured are two by two. The fake grid does
  not model Second Life's repeats or OpenSim's half second between
  neighbours ([[server-fake-grid-teleport-edges]]). The viewer keeps drawing
  a retired neighbour's ground and water
  ([[viewer-retired-neighbour-stays-drawn]]). The aditi avatar could not be
  walked from its landing point and was flown.

## Known already

OpenSim sends no kill to the crossing agent's own viewer, and delivers
`EnableSimulator` / `CrossedRegion` / `TeleportFinish` over the event queue.
The fake grid crosses only when scripted. SL's unsit/resit on a vehicle
crossing is assumed, not measured.

From [[gridspec-logout]] (2026-10-06): at a logout OpenSim sends
`DisableSimulator` down every child circuit within 70 ms; Second Life sends
nothing down them before its `LogoutReply`. The session drops a child circuit
on `DisableSimulator` (its objects and coarse dots go) but reports no event
for the region itself, so `sl-client-bevy`'s region index keeps a retired
neighbour until the next world reset — decide here whether that needs an
event.

## Discover

- Walk across a border: an `sl-repl` `AgentUpdate` drive or an automation
  key-hold verb; the local 2x2 OpenSim has walkable borders; on aditi the
  user picks a region pair (or drives Firestorm while we record).
- Record child-agent radius vs draw distance, `EnableSimulator` timing,
  kills on the source, `CrossedRegion` contents, seated crossing.

## Document

`book/src/gridspec/teleport.md` § Crossing and child agents.

## Fake grid

Small per-flavour details of the scripted crossing in this task;
movement-triggered crossing is large — [[server-world-agent-movement]] and
[[server-agent-transfer]].

## Viewer

Region rebase, coarse dots per region, seat survival; `e2e` crossing on both
flavours.
