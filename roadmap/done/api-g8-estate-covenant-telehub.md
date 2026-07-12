---
id: api-g8
title: Estate covenant & telehub
topic: api
status: done
origin: SL_API_ROAD_MAP.md
---

Context: [context/api.md](../context/api.md).

## G8 — Estate covenant & telehub

`EstateCovenantRequest`/`EstateCovenantReply`; `TelehubInfo` (Event) plus the
god/estate telehub-management `EstateOwnerMessage` sub-commands.
OpenSim-testable (estate owner avatar).

- [x] G8 covenant and telehub. New `EstateCovenant` + `TelehubInfo` types
  (`types/map.rs`). Commands `RequestEstateCovenant` (`EstateCovenantRequest` →
  `Event::EstateCovenant`), `RequestTelehubInfo`/`ConnectTelehub`/
  `DisconnectTelehub`/`AddTelehubSpawnPoint`/`RemoveTelehubSpawnPoint` (the
  `EstateOwnerMessage`/`telehub` `info ui`/`connect`/`delete`/`spawnpoint add`/
  `spawnpoint remove` sub-commands → `Event::TelehubInfo`). Circuit encoder
  `send_estate_covenant_request` + `Session` methods reusing the existing
  `send_estate_owner_message` helper; `EstateCovenantReply`/`TelehubInfo`
  decoded in the dispatch path. Server: `EstateCovenantRequest` + the `telehub`
  `EstateOwnerMessage` decode into matching `ServerEvent`s (a
  `telehub_server_event` param parser, mirroring `LLClientView`), plus
  `SimSession::send_estate_covenant_reply`/`send_telehub_info` encoders. Both
  runtimes + REPL (6 commands) + format.rs. Tests: 3 lifecycle client (commands
  encode, covenant-reply decode, telehub-info decode) + 1 loopback round-trip +
  4 REPL registry. Book: extended `content/region.md` Estate/Telehub sections +
  "In this codebase". **Scope note:** modelled telehub as 5 individual commands
  (matching the existing per-`EstateOwnerMessage`-method command convention,
  e.g. `KickEstateUser`/`RestartRegion`), not a single op enum; the
  `SimulatorPresentAtLocation` and other estate sub-commands are out of scope.
  OpenSim-testable (estate-owner avatar) but NOT live-tested this session
  (loopback + lifecycle tests cover both directions). **NEXT = G9** (task-script
  run/stop/reset/sensor).
