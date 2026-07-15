---
id: viewer-world-map-search
title: World-map floater — region-name search
topic: viewer
status: blocked
origin: user request (2026-07); split from viewer-world-map
blocked_by: [viewer-world-map-floater]
---

Context: [context/viewer.md](../context/viewer.md).

Region-name search on the world-map floater: type a region name, issue
`request_map_by_name`, list the matching results, and recentre the map surface
([[viewer-world-map-floater]]) on a selected result.

Reference (Firestorm, read-only): `llfloaterworldmap`, `llworldmapmessage`.

Builds on: `protocol-12` map data and the map floater surface.

Deps: [[viewer-world-map-floater]].
