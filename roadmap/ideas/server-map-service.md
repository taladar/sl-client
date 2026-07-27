---
id: server-map-service
title: Map service — tiles and map items
topic: server
status: ideas
origin: user request (2026-07) — size what a real server would involve
refs: [protocol-sim-http-misc]
---

Context: [context/server.md](../context/server.md).

The world-map backend:

- **tile generation**: each simulator renders its regions' map tiles
  (terrain + prims at map resolution) and pushes them here on change;
  the service builds the zoom pyramid and serves the
  `map-server-url`-shaped HTTP tree the viewer's tile fetcher consumes
  ([[protocol-sim-http-misc]] covers the wire shape);
- **map items**: agent locations/counts (from presence), telehubs, land
  for sale, events — the `MapItemRequest` backends;
- region-name search for the map UI (`MapNameRequest`), backed by the
  grid service's registry.
