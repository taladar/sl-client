---
id: server-lsl-lib-ossl-land-writers
title: Library tranche — OpenSim's land, terrain, wind and environment writers
topic: server
status: ready
origin: split from server-lsl-lib-land-region-env (2026-11-07)
blocked_by: [server-lsl-lib-land-region-env]
refs: [server-lsl-lib-ossl]
---

Context: [context/lsl.md](../context/lsl.md).

`server-lsl-lib-land-region-env` implemented the OSSL readers that sit on what
the land tranche's host already describes (`osGetParcelDwell`,
`osGetCurrentSunHour`, `osGetMapTexture`, `osGetRegionMapTexture`,
`osWindActiveModelPluginName`) and the two parcel writers that are land changes
it already applies (`osSetParcelMusicURL`, `osSetParcelMediaURL`). The other 28
of OpenSim's land-shadowing functions each need state or a push the fake grid
does not keep yet. They are compiled by OpenSim's dialect, gated by
`vm::ossl::admit`, and listed `later` under this id in
`sl-lsl-runtime/src/library/opensim-scope.txt`:

- **Terrain**: `osSetTerrainHeight`, `osTerrainSetHeight`, `osTerrainFlush`,
  `osSetTerrainTexture`, `osSetTerrainTextureHeight` — a mutable heightfield
  and composition in the region, and the `LayerData` / `RegionHandshake`
  resend a flush sends.
- **Water and sun**: `osSetRegionWaterHeight`, `osSetRegionSunSettings`,
  `osSetEstateSunSettings` (a no-op in OpenSim's source: its body is commented
  out), `osSetSunParam`, `osSunSetParam`.
- **Wind**: `osGetWindParam`, `osSetWindParam` — per-plugin parameters
  (`SimpleRandomWind`'s `strength`, `ConfigurableWind`'s five), and the C#
  exception an unknown plugin or parameter throws, which needs an exception
  carrying its own message.
- **Environment and LightShare**: `osReplaceAgentEnvironment`,
  `osReplaceParcelEnvironment`, `osReplaceRegionEnvironment`,
  `osResetEnvironment`, `lsGetWindlightScene`, `lsSetWindlightScene`,
  `lsSetWindlightSceneTargeted`, `lsClearWindlightScene` — writes into the
  region's `EnvironmentSettings` and the environment push viewers follow; the
  `ossl/errors` replay still writes `lsGetWindlightScene`'s answer in.
- **Parcels**: `osParcelJoin`, `osParcelSubdivide`, `osParcelSetDetails`,
  `osSetParcelDetails`, `osSetParcelSIPAddress` — joining and splitting the
  parcel bitmap, the overlay resend, and the voice module's SIP address.
- **Region**: `osGetRegionStats` (OpenSim's `STATS_*` layout of its last
  `SimStats`), `osRegionNotice` (both overloads), `osRegionRestart`.

Port each from `OSSL_Api.cs` / `LS_Api.cs`, register it in
`library/registry.rs`, move its scope line to `in`, and drop the replay's
written-in answer where one exists.
