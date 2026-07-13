---
id: viewer-minimap-worldmap-ui
title: Minimap / radar & world-map UI
topic: viewer
status: ideas
origin: reference-viewer feature-cluster survey (2026-07)
blocked_by: [viewer-ui-framework]
---

Context: [context/viewer.md](../context/viewer.md).

Three related surfaces: the **net / mini map** (nearby avatars and objects,
parcel overlay, north-up vs camera-up rotation, click-to-target), the
Firestorm-style **avatar radar** list (who's nearby, ranges, entry/exit), and
the full **world-map** floater (region tiles, teleport-by-map, search,
landmark / friend / event beacons).

The map **protocol** already exists (`protocol-12`: MapBlock / MapItem /
MapName) and coarse locations arrive as `CoarseLocationUpdate`; this stub is the
**UI / rendering** on top, including fetching and caching region tile imagery.

Reference (Firestorm, read-only): `llnetmap`, `llfloatermap`,
`llfloaterworldmap`, `llworldmap(message)`, `llworldmipmap`, `fsradar`,
`fsfloaterradar`.

Builds on: `protocol-12` map data, `CoarseLocationUpdate`, and `sl-map-tools`
for tile fetch.

Deps: [[viewer-ui-framework]].
