---
id: server-lsl-lib-land-region-env
title: Library tranche — parcel, region, estate and environment queries
topic: server
status: done
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

## From server-lsl-lib-ossl (2026-11-01)

OpenSim's own functions that shadow this tranche's are this tranche's to
implement, on the OpenSim flavours only (Second Life does not compile them):
LightShare's four functions (`lsGetWindlightScene` is what `ossl/errors` reads,
its water colour answered by the test as the grid did), the wind, sun and
environment readers and writers, the parcel and estate writers, the terrain
writers and textures, the region map texture and statistics, and the region
restart and notice. They are already in the library table with their signatures,
compiled by OpenSim's dialect, and **gated**: `vm::ossl::admit` checks the
region's `[OSSL]` settings before the function is looked for, so a denial is
already right and only the allowed call is missing. Port each from `OSSL_Api.cs`
(or `LS_Api.cs`), register it in `library/registry.rs` and move its line in
`sl-lsl-runtime/src/library/opensim-scope.txt` to `in`, which the coverage test
then holds; `ossl/available` and `ossl/errors` stop needing the answers their
replay writes in for it (`vm/tests/ossl.rs`). The 35: `lsClearWindlightScene`,
`lsGetWindlightScene`, `lsSetWindlightScene`, `lsSetWindlightSceneTargeted`,
`osGetCurrentSunHour`, `osGetMapTexture`, `osGetParcelDwell`,
`osGetRegionMapTexture`, `osGetRegionStats`, `osGetWindParam`, `osParcelJoin`,
`osParcelSetDetails`, `osParcelSubdivide`, `osRegionNotice`, `osRegionRestart`,
`osReplaceAgentEnvironment`, `osReplaceParcelEnvironment`,
`osReplaceRegionEnvironment`, `osResetEnvironment`, `osSetEstateSunSettings`,
`osSetParcelDetails`, `osSetParcelMediaURL`, `osSetParcelMusicURL`,
`osSetParcelSIPAddress`, `osSetRegionSunSettings`, `osSetRegionWaterHeight`,
`osSetSunParam`, `osSetTerrainHeight`, `osSetTerrainTexture`,
`osSetTerrainTextureHeight`, `osSetWindParam`, `osSunSetParam`,
`osTerrainFlush`, `osTerrainSetHeight`, `osWindActiveModelPluginName`.

## Done (2026-11-07)

The tranche runs on both flavours. `vm/land.rs` holds the records a host lends
(`Parcel`, `Region`, `SimData`, `Environment`, `ParcelMedia`) and the
`LandChange` it applies; `library/land.rs` every function above with each
grid's codes, keys, refusals and edges — `llGetEnv`'s two key sets,
`PARCEL_DETAILS_*` 7–13 and 64 per grid, Second Life's edge parcel and
OpenSim's `NullReferenceException` outside the region, the masked region
flags, `llRequestSimulatorData`'s release, unknown and empty names, the
environment rules and their texture refusal, `llGetSimStats`'s unknown codes —
and the write half behind the rule that a script changes only land its owner
owns (or, for the estate, manages). The OpenSim dialect refuses the four
Second Life-only functions and the environment constants. The fake grid
(`sl-fake-grid/src/scripts/land.rs`) answers from its parcel records and
listings, region limits and estate, neighbours, heightfield, terrain wind and
day cycle, and applies a change at once: the parcel's stream (its record sent
again to the avatars on it), passes and bans in its access lists, the estate's
lists, and the parcel's media with its update and command. OpenSim's readers
over the same state are in too: `osGetParcelDwell`, `osGetCurrentSunHour`,
`osGetMapTexture`, `osGetRegionMapTexture`, `osWindActiveModelPluginName`,
`osSetParcelMusicURL`, `osSetParcelMediaURL`. Held by `vm/tests/land.rs` (the
five `land/*` replays line for line, but for the deviations named there) and,
end to end, `a_prim_reads_its_land_and_region` in
`sl-fake-grid/tests/scripts.rs`.

## Left open

- **The other 28 OSSL land functions** — terrain, water, sun, wind, the
  environment and LightShare writers, parcel join and split, region stats,
  notices and restart — are [[server-lsl-lib-ossl-land-writers]].
- **`llGetRegionTimeDilation` is a constant `1.0`.** The heartbeat does not
  measure an overloaded region yet, so the acceptance row about tracking one is
  not met.
- **`llEjectFromLand` changes nothing**: the fake grid sends nobody off a
  parcel. **`llGetParcelPrimOwners` is `[]` on both flavours**: it keeps no
  prim counts by owner (OpenSim answers the list).
- **`llGetSimStats`** reports the frame rate and the avatars; every time the
  fake grid does not measure is `0.0`.
- **The ban-list write is not checked end to end** against a viewer's
  access-list reply, though both read the same list.
- **`llSetDamage` and `llGetHealth` are stubs** (the stub notice), the combat
  surface out of scope.
- Measured quirks not imitated: Second Life's ground a hair above the clamped
  edge beyond the region, and its parcel and region suns read a moment apart.
