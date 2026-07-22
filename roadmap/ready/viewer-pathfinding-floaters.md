---
id: viewer-pathfinding-floaters
title: Pathfinding floaters — console, characters, linksets
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

The pathfinding tool set, including its protocol slice (the receive-side
agent-state events landed in `missing-eq-batch-1`; the caps have not):

- **Caps**: `NavMeshGenerationStatus` / `RetrieveNavMeshSrc`,
  `ObjectNavMeshProperties`, `AgentPreferences` pathfinding bits — add the
  pairings in `sl-proto` first (small, mirrors the other caps pairings).
- **Linksets floater**: list linksets with their pathfinding attributes
  (walkable / static obstacle / dynamic / phantom, walkability
  coefficients), edit + apply.
- **Characters floater**: list pathfinding characters (creatures), with
  beacon/track.
- **Console**: navmesh status display, rebake-region button
  (`NavMeshRebake`), and — optional, render-heavy — the navmesh
  visualisation overlay; if the overlay proves large, split it out when
  fleshing begins.

Reference (Firestorm, read-only): `llfloaterpathfindingconsole`,
`llfloaterpathfindinglinksets`, `llfloaterpathfindingcharacters`,
`llpathfindingmanager`.

Builds on: `missing-eq-batch-1` events + the caps layer.
