---
id: api-g4
title: Search / Directory system
topic: api
status: done
origin: SL_API_ROAD_MAP.md
---

Context: [context/api.md](../context/api.md).

## G4 — Search / Directory system

Full search surface: `DirFindQuery` (people/groups/events/classifieds),
`DirPlacesQuery`, `DirLandQuery`, `DirClassifiedQuery` and their replies
(`DirPeopleReply`, `DirGroupsReply`, `DirEventsReply`, `DirClassifiedReply`,
`DirPlacesReply`, `DirLandReply`); `AvatarPickerRequest`/`Reply` (name
autocomplete); `PlacesQuery`/`PlacesReply` (user holdings). New query-flag and
per-category result types. OpenSim-testable (search module).

- [x] G4 directory/search queries and replies. New
  `sl-proto/src/types/directory.rs`: the `DirFindFlags` (`DFQ_*`) and
  `LandSearchType` (`ST_*`) query-flag bitfields, plus per-category result types
  (`DirPeopleResult`, `DirGroupResult`, `DirEventResult`, `DirClassifiedResult`,
  `DirPlaceResult`, `DirLandResult`, `AvatarPickerResult`, `PlacesResult`).
  Commands `DirFindQuery` (the unified people/groups/events query — flags pick
  which), `DirPlacesQuery`, `DirLandQuery`, `DirClassifiedQuery`,
  `AvatarPickerRequest`, `PlacesQuery`; events `DirPeopleReply`,
  `DirGroupsReply`, `DirEventsReply`, `DirClassifiedReply`, `DirPlacesReply`,
  `DirLandReply`, `AvatarPickerReply`, `PlacesReply` decoded in the dispatch
  path. Server: each query surfaces as a matching `ServerEvent` and `SimSession`
  gains a `send_*_reply` encoder for all eight replies. Both runtimes + REPL
  (`dir_find_query`, `dir_places_query`, `dir_land_query`,
  `dir_classified_query`, `avatar_picker_request`, `places_query`) + tests (3
  directory-type unit, 2 lifecycle encode/decode, 2 loopback round-trips
  covering all 6 queries and all 8 replies) + new book chapter
  `content/search.md`. **Scope note:** `DirFindQuery` is one wire message reused
  for people/groups/events (selected by `DirFindFlags`), modelled as one command
  with three reply events, exactly as the viewer/sim use it; the `*Backend`
  (sim→dataserver) variants are internal trusted messages with no
  viewer/`SimSession` role and are not wrapped.
