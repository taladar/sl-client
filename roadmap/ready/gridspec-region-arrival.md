---
id: gridspec-region-arrival
title: Region handshake identity, the arrival burst and region telemetry on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, fake-grid-own-attachments-and-region-moves,
  viewer-region-entry-maturity-gate, server-world-heartbeat]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

OpenSim sets `RegionProtocols` bit 63; SL's bits are unmeasured beyond
`region_protocols: 1`. An aditi mainland handshake carries four nil
`TerrainDetail` ids and `product_name = "Mainland / Full Region"`. OpenSim
sends `RegionHandshake` on child circuits. `AgentStateUpdate` is SL-only.
`SimStats` ids 1000+ are OpenSim-only; aditi sends
`SimulatorViewerTimeMessage`. The fake grid's identity is fixed
("Fake Region", flags 0) and time / stats are timeline-only.

## Discover

A `region-handshake-survey` conformance case (or extend `login-handshake`)
dumping the whole identity (flags, extended flags, `CPURatio`, `ColoName`,
`ProductSKU`, channel version), `AgentMovementComplete`, and every
unsolicited message in the first 30 s with its order and cadence (time,
stats, `AgentStateUpdate`, coarse locations); both grids, one avatar.

## Document

`book/src/gridspec/region-arrival.md`; link from `content/region.md`.

## Fake grid

Small — in this task: per-flavour identity defaults, burst order, periodic
`SimulatorViewerTimeMessage` / `SimStats` with each grid's cadence and id
set. A real region clock belongs to [[server-world-heartbeat]].

## Viewer

About-window region fields, absent `RegionInfo5`, unknown SL-only pushes
tolerated; `e2e_live_checks` on both fake flavours.
