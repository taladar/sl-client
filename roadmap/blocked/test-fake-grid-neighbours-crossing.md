---
id: test-fake-grid-neighbours-crossing
title: Neighbour child agents and a scripted region crossing
topic: test
status: blocked
origin: test-harness plan (2026-08-30); the CrossedRegion follow-up of viewer-fake-grid-teleport
points: 8
refs: [viewer-fake-grid-teleport]
blocked_by: [test-fake-grid-npc-avatars, test-fake-grid-timeline]
---

Context: [context/testing.md](../context/testing.md).

The fake grid has no movement authority, so a walked border crossing must
be scripted, mirroring `teleport_session`:

- `RegionConfig::neighbours: NeighbourPolicy { Adjacent (default), None,
  Named }`; on a root `AgentArrived`, `announce_neighbours` prepares a
  child session per adjacent region (role `Child`, its own burst on
  `CircuitOpened`: a region-stamped marker object, terrain, objects, no
  own avatar) and enqueues `EnableSimulator` +
  `EstablishAgentCommunication`.
- `FakeGrid::cross_agent(&agent, region, position, velocity)`: reject a
  non-adjacent target; reuse or announce the child; set its arrival
  position; `enqueue_crossed_region` (extend the LLSD with `Position`,
  `LookAt`, `RegionSizeX/Y`); wait for the destination's `AgentArrived`;
  the source kills its avatar object and becomes a child; non-adjacent
  old children are retired; a `CrossingNotice` is published.

The continuity test will force the viewer-side work: re-basing existing
entities when the current region handle changes, exactly one avatar
entity per agent across circuits, attachments re-parenting, the camera
keeping its world pose, terrain keyed by handle left alone. Assert the
subject prim's projected centroid within two pixels before and after,
ground on both sides of the border, and no teleport/disconnect events.
