---
id: server-lsl-lib-land-region-env
title: Library tranche — parcel, region, estate and environment queries
topic: server
status: ideas
origin: LSL-on-the-fake-grid audit (2026-09-20)
blocked_by: [server-lsl-vm-execution, server-world-agent-movement,
  gridspec-lsl-land-region-env]
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

## Measured (gridspec-lsl-land-region-env, 2026-10-26)

The tables are in `book/src/gridspec/lsl.md` § Parcel, region, estate and
environment queries; what the library must hold to beyond the list above:

- **`llGetEnv` keys differ per grid.** Second Life answers `region_name`,
  `region_rating`, `grid`, `allow_damage_adjust`, `damage_limit` and
  `agent_reserved` too, `dynamic_pathfinding` as `enabled`, and no
  `region_up_time`; OpenSim answers `region_up_time`, a constant `0` for
  pathfinding, `1` for the CPU ratio and `OpenSim` for the SKU. Second Life's
  keys are case-sensitive, OpenSim lowers them.
- **`PARCEL_DETAILS_*` 7, 8 and 10 mean different things per grid** (Second
  Life: prim capacity, prims used, landing look-at; OpenSim: two sound flags
  and the claim date, plus 64 for dwell), and OpenSim answers 9–13 with `0`.
  An unknown code is left out on Second Life with a `DEBUG_CHANNEL` error and
  answered `0` on OpenSim.
- **A position outside the region** is clamped to the edge parcel on Second
  Life; OpenSim answers no parcel, and its `llGetParcelFlags` throws there —
  whether the OpenSim flavour imitates the exception is the flavour's call.
- **`llGetRegionFlags` is masked on Second Life** to the `REGION_FLAG_*`
  bits; OpenSim hands out the raw region flags.
- **`llRequestSimulatorData`**: names are case-insensitive on Second Life;
  `DATA_SIM_RELEASE` is refused (`NULL_KEY`) there; an unknown region's
  position is never answered and the empty name's status is `crashed`.
  OpenSim answers its own region's position in region units (a bug in its
  own-region branch).
- **The environment**: `llGetEnvironment`, `llGetSimStats`,
  `llGetRegionTimeOfDay` and `llGetClosestNavPoint` are Second Life's only;
  the OpenSim flavour refuses to compile them. Texture rules are refused to
  a script whose owner does not own the setting.
- **The write half on land the owner does not own** (`llSetParcelMusicURL`)
  is silently ignored on both grids; `llGetParcelPrimOwners` answers `[]` on
  Second Life and the full list on OpenSim.
