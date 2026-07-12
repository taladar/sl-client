---
id: missing-eq-batch-1
title: pathfinding agent state
topic: missing
status: done
origin: MISSING_ROADMAP.md
---

Context: [context/missing.md](../context/missing.md).

**EQ batch 1 — pathfinding agent state (closes issue 3).**
`AgentStateUpdate` (body `{ "can_modify_navmesh": bool }` — whether the agent
may rebake this region's navmesh; Firestorm `llpathfindingmanager.cpp`) and
`NavMeshStatusUpdate` (navmesh dirty/baking status). SL-only — OpenSim emits
neither, so this only ever shows up against a real grid.

Implemented as `Event::AgentStateUpdate { can_modify_navmesh: bool }` (an
inline variant — a single flag warrants no domain struct) and
`Event::NavMeshStatus(NavMeshStatus)`, where `NavMeshStatus { region_id:
Uuid, version: u32, status: NavMeshBuildStatus }` lives in
`types/pathfinding.rs`. `NavMeshBuildStatus` is an enum over the four wire
tokens (`Pending`/`Building`/`Complete`/`Repending`) with a `from_wire`
parser that maps any unrecognised or missing value to `Complete`, mirroring
the reference viewer's `LLPathfindingNavMeshStatus`. `region_id` stays a raw
`Uuid` — this crate has no dedicated region-key newtype and represents region
ids as `Uuid` everywhere (see `RegionIdentity`, `EnvironmentSettings`).
