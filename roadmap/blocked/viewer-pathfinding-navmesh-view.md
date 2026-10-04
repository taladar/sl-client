---
id: viewer-pathfinding-navmesh-view
title: Render and test the region's navmesh
topic: viewer
status: blocked
origin: protocol-reference-capabilities triage (2026-10-04)
blocked_by: [viewer-pathfinding-floaters]
refs: [protocol-reference-capabilities, viewer-pathfinding-floaters]
---

Context: [context/viewer.md](../context/viewer.md).

Second Life grants `RetrieveNavMeshSrc`: POST an empty map → `{navmesh_version,
navmesh_data}`, fetched when `NavMeshGenerationStatus` reports a new version.
Firestorm decodes the data only through the closed Havok library; its
open-source build cannot show the navmesh at all. Decode the format ourselves
and draw it (the "View / test" pathfinding window, with the walkability
coefficients A–D shaded).

Every capability adopted here goes into `REQUESTED_CAPABILITIES`
(`sl-proto/src/session.rs`), is used where the region grants it, keeps the
existing path where it does not, and is served by the fake grid per flavour as
`seed-capabilities` measured it (`book/src/gridspec/capabilities.md`). Shapes
and Firestorm references: `book/src/comms/caps-reference.md`.
