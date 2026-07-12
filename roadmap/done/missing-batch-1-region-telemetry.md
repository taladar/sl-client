---
id: missing-batch-1
title: region telemetry
topic: missing
status: done
origin: MISSING_ROADMAP.md
---

Context: [context/missing.md](../context/missing.md).

## Batch 1 — region telemetry (closes issue 2)

- **`SimStats` (Low 140)** → `Event::SimStats(Box<RegionStats>)`:
  `RegionStats { grid_coordinates: GridCoordinates, region_flags: u32,
  object_capacity: u32, region_flags_extended: u64, stats: Vec<(SimStatId,
  f32)> }`, where `SimStatId` is an enum over the known stat ids with an
  `Unknown(u32)` fallback. (Implemented as `GridCoordinates`, not the
  originally-sketched `RegionCoordinates`: the `RegionX` / `RegionY` fields
  carry the region's map-tile indices, not a region-local position — confirmed
  against OpenSim `RegionInfo.RegionLocX = WorldLocX / RegionSize`.) Stat-id
  meanings
  (TimeDilation=0, SimFPS=1, PhysicsFPS=2, Agents=13, ActiveScripts=15, …) are
  enumerated in `~/devel/3rdparty/opensim/OpenSim/Framework/SimStats.cs`
  (`StatsID` enum) and the Firestorm `LLViewerStats` sim-stat ids.
- **`SimulatorViewerTimeMessage` (Low 150)** → `Event::SimulatorTime(Box<…>)`
  with `usec_since_start: u64, sec_per_day: u32, sec_per_year: u32,
  sun_direction: Vector, sun_phase: f32, sun_ang_velocity: Vector`.
