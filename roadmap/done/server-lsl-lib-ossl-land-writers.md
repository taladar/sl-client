---
id: server-lsl-lib-ossl-land-writers
title: Library tranche — OpenSim's land, terrain, wind and environment writers
topic: server
status: done
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

## Done (2026-11-15)

All 28 functions (30 overloads) are in `library/ossl_land.rs`, their scope
lines `in`. Each makes the second check its OpenSim body makes against the
permissions module — `IsEstateManager`, `IsAdministrator` (a god, the estate
owner by `region_owner_is_god`), `CanIssueEstateCommand`, `CanTerraformLand`,
`CanEditParcelProperties` with the group power named — from `Host::standing`
and `Host::group_powers`. `vm/land.rs` grew `RegionEdit` (the ground's
heights and textures, a flush, the water, the sun, a wind parameter, a
notice, a restart and its abort, a LightShare scene and its clearing),
`LandChange::Join`, `Subdivide` and `Details` (`ParcelDetails`),
`EnvironmentScope` / `EnvironmentChange`, `DayCycle`, `WindParamError`,
`LightShare` (OpenSim's `RegionLightShareData`, its defaults the default),
`Region::environment_override` and `Environment::altitudes`; the `Host` grew
`edit_region`, `wind_param`, `region_stats`, `lightshare`, `day_cycle`,
`change_environment` and `parcel_sip_address`. `Exception` has
`NoWindPlugin`, `NoWindParam` (the wind models' `System.Exception`s, which
`osGetWindParam` lets out) and `NoParcelAt` (the land module's throw off the
region). `osFormatString`'s formatter is `ossl::dotnet_format`, which a
restart notice's message goes through; `money_permissions::opensim_integer_item`
is OpenSim's `GetLSLIntegerItem` for any list. `extract_ossl_checks.py` reads
a check naming its function through a local constant, which gives
`osSetParcelDetails` and `osParcelSetDetails` their `High` (they read `none`
before) and the deprecated one its replacement. The `ossl/available` and
`ossl/errors` replays no longer write in `osGetWindParam`'s or
`lsGetWindlightScene`'s answer: both read from the host.

The fake grid (`scripts/land_writers.rs`, `region_edits.rs`): a live
heightmap (`terrain::Heightmap`, `LiveGround`) every ground reader goes
through, the moved patches sent after the tick (`RegionChange::TerrainPatched`)
and every patch after a flush; textures in a `RegionInfo` and a handshake,
blend heights in a handshake, the water in a `RegionInfo`, the sun kept
silently (its fixed flag in the region's flags); the two wind models'
parameters kept at OpenSim's stock values and not simulated; notices to one
avatar (`AgentAlertMessage`) or all (`AlertMessage`); a restart's first
notice, its remaining alerts kept for `server-fake-grid-estate-actions`, and
its abort. Environments are the region's (`RegionEdits::environments`): a
script's change of the region's, a parcel's or an avatar's, and a viewer's
`ExtEnvironment` change mirrored in, is served by every session and by one
opening later (`RegionChange::EnvironmentChanged`, `AgentEnvironment`); a
region's refreshes each viewer with a `RegionInfo`, a parcel's moves its
environment version and sends the parcel to the avatars on it, and an
avatar's is served by its session in place of every other
(`SimSession::set_agent_environment`, `forget_environment`,
`stored_environment`). LightShare converts both ways as OpenSim's
`ViewerEnvironment` does, its quirks kept (the fog exponent's square root,
the glow size's formulas that are no inverse). Joins and cuts do the land
module's bitmap surgery with the next local id, a new listing, the landing
point and search flag dropped, and resend the overlay; details are kept
silently but for the see-avatars flag, which resends the overlay. The
region's 48 statistics in OpenSim's order; a SIP address throws (WebRTC voice
has none). Runtime tests in `vm/tests/ossl_land.rs`, the LightShare round
trip and the parcel recount in `scripts/land_writers.rs`, and the fake-grid
test `ossl_land_writers_move_the_ground_the_water_the_sky_and_the_parcels`;
the book's lsl-engine chapter says so.

## Left open

- A restart's countdown after its first notice and the restart itself:
  `server-fake-grid-estate-actions` (noted there).
- No wind model is simulated: the wind parameters are kept and read back,
  the wind blows as the fixture says.
- Nothing here was measured against the local OpenSim beyond what the
  `ossl/*` probes already read; the behaviour is ported from the source.
