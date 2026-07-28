---
id: server-search-service
title: Search/directory service
topic: server
status: ideas
origin: user request (2026-07) — size what a real server would involve
---

Context: [context/server.md](../context/server.md).

The `DirFindQuery` family's backend: people search (accounts), groups
search, places/parcel search (fed by parcels flagged show-in-search,
with the maturity filtering rules), land-for-sale listings, events, and
classifieds — each a paginated query surface with the per-type reply
messages the client already decodes.

Mostly an indexing job over the other services' data (accounts, groups,
the simulators' parcel tables) plus the event/classified stores that
exist only here. OpenSim ships this as the optional OpenSimSearch
module; SL's is part of the closed backbone.
