---
id: gridspec-neighbours-crossing
title: Child agents, EnableSimulator and region crossing on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, test-handover-distant-and-vehicle-aditi,
  viewer-seated-region-crossing]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

OpenSim sends no kill to the crossing agent's own viewer, and delivers
`EnableSimulator` / `CrossedRegion` / `TeleportFinish` over the event queue.
The fake grid crosses only when scripted. SL's unsit/resit on a vehicle
crossing is assumed, not measured.

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
