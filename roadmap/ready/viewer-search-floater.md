---
id: viewer-search-floater
title: Search floater — directory search UI
topic: viewer
status: ready
origin: user request (2026-07-22), while shipping viewer-social-profiles
blocked_by: [viewer-ui-widget-scaffold]
refs: [api-g4, viewer-social-profiles, viewer-media-prim-browser]
---

Context: [context/viewer.md](../context/viewer.md).

The **Search floater**: the in-viewer directory search the Vintage skin keeps
(Firestorm's legacy search window) — a query field plus category tabs over
the **directory protocol**, which is fully implemented (`api-g4`):

- **People** (`DirFindQuery` people flag → `DirPeopleReply`) — result rows
  open the profile floater ([[viewer-social-profiles]]).
- **Groups** (`DirFindQuery` groups flag → `DirGroupsReply`) — rows open the
  group profile ([[viewer-social-group-profile]]).
- **Events** (`DirFindQuery` events flags → `DirEventsReply`), with the
  date / category filters the reference offers — result rows open the event
  detail floater ([[viewer-event-details]]).
- **Places** (`DirPlacesQuery` → `DirPlacesReply`) and **Land** sales
  (`DirLandQuery` → `DirLandReply`, price / area sort flags).
- **Classifieds** (`DirClassifiedQuery` → `DirClassifiedReply`) — rows show
  the classified detail (the profile floater's detail panel is the model).

Maturity checkboxes (General / Moderate / Adult → the `DFQ_*` maturity
flags), paging via the query-start offsets, and per-category result counts.

The reference's **web search** (the search *website* in an embedded browser)
is a separate concern blocked on CEF ([[viewer-media-prim-browser]]); this
task is the protocol-backed legacy directory UI, which OpenSim's search
module can exercise locally.

Reference (Firestorm, read-only): `fsfloatersearch.cpp` (legacy search),
`lldirectory*`, Vintage `floater_fs_search.xml`.

Builds on: `api-g4` (directory queries / replies, all decoded).
