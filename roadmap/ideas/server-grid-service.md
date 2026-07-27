---
id: server-grid-service
title: Grid service — region registry and simulator discovery
topic: server
status: ideas
origin: user request (2026-07) — size what a real server would involve
---

Context: [context/server.md](../context/server.md).

The registry that makes "multiple simulators on potentially separate
hosts" possible: region name / grid coordinates / region handle →
simulator host, port, and internal service endpoint.

- Simulators register their regions at startup and heartbeat; stale
  registrations expire.
- Lookups serve login (start region), the world map (MapBlock backends),
  teleports (resolve destination region → owning simulator), and
  neighbour queries (which simulators host the adjacent regions, for
  child agents).
- Owns the grid-wide invariants: no two simulators claiming the same
  coordinates, region size metadata (var-regions if ever supported),
  default/fallback regions.

OpenSim's GridService + region registration protocol is the reference
shape.
