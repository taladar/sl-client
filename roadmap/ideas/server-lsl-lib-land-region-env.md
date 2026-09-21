---
id: server-lsl-lib-land-region-env
title: Library tranche — parcel, region, estate and environment queries
topic: server
status: ideas
origin: LSL-on-the-fake-grid audit (2026-09-20)
blocked_by: [server-lsl-vm-execution, server-world-agent-movement]
refs: [server-lsl-lib-avatar-control, server-fake-grid-scripted-scenario]
---

Context: [context/lsl.md](../context/lsl.md).

The tranche with the least new machinery behind it: the fake grid
already models parcels with their flags and access lists, an estate
record, region limits, terrain, a day cycle and an environment
(`parcel_edits`, `estate`, `terrain`, and the `RegionLimits` /
`RegionTerrainComposition` the estate floater writes). Most of these
functions are readers over state that already exists.

- **Parcel**: `llGetParcelFlags`, `llGetParcelDetails`,
  `llGetParcelMaxPrims`, `llGetParcelPrimCount`,
  `llGetParcelPrimOwners`, `llGetParcelMusicURL`,
  `llSetParcelMusicURL`, `llOverMyLand`, `llEjectFromLand`,
  `llAddToLandPassList`, `llAddToLandBanList`,
  `llRemoveFromLandPassList`, `llRemoveFromLandBanList`,
  `llResetLandBanList`, `llResetLandPassList`,
  `llParcelMediaCommandList`, `llParcelMediaQuery`,
  `llManageEstateAccess`. The write half needs the permission rule — a
  script may only manage land its owner owns — which the fake grid can
  state simply (every account owns the region's content) but should
  state *somewhere*, not by omission.
- **Region**: `llGetRegionName`, `llGetRegionCorner`,
  `llGetRegionFlags`, `llGetRegionFPS`, `llGetRegionTimeDilation`
  (real, once [[server-world-heartbeat]] measures it),
  `llGetRegionAgentCount`, `llGetSimulatorHostname`, `llGetSimStats`,
  `llRequestSimulatorData` over `dataserver`, `llEdgeOfWorld`,
  `llGetClosestNavPoint`.
- **Ground and weather**: `llGround`, `llGroundNormal`,
  `llGroundSlope`, `llGroundContour`, `llWater`, `llWind`, `llCloud` —
  all readable from the heightfield and the environment, and the last
  three may honestly be constants on a fake grid as long as the constant
  is stated.
- **Environment**: `llGetEnv` (the `"agent_limit"`,
  `"dynamic_pathfinding"`, `"estate_id"`, `"region_max_prims"`,
  `"simulator_hostname"`, … keys), `llGetDayLength`, `llGetDayOffset`,
  `llGetSunDirection`, `llGetMoonDirection`, `llGetRegionDayLength`,
  `llGetRegionSunDirection` — over the day cycle the scenario already
  carries, which a recent commit taught to advance between frames.
- **Damage and combat**: `llSetDamage`, `llGetHealth` and the combat
  surface — almost certainly out of scope; list them, stub them, say so.

Acceptance: `llGetParcelDetails` and `llGetParcelFlags` agree with what
the About Land floater shows for the same parcel in the same run;
`llGround` matches the terrain the viewer draws under the prim;
`llGetRegionTimeDilation` tracks a deliberately overloaded region; and a
ban-list write is visible in the parcel access reply.
