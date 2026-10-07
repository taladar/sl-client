---
id: gridspec-region-arrival
title: Region handshake identity, the arrival burst and region telemetry on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, fake-grid-own-attachments-and-region-moves,
  viewer-region-entry-maturity-gate, server-world-heartbeat,
  protocol-neighbor-discovered-repeats, gridspec-region-info,
  gridspec-neighbours-crossing]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Done (2026-10-07)

Measured and written up in `book/src/gridspec/region-arrival.md`.

- **Discover.** The new `region-arrival` case watches a login for 32 s with
  the circuits probed from the first datagram (`GridTest::probes_arrival`,
  `Client::set_circuit_probe`, `SlClientPlugin::circuit_probe`) and records
  the handshake's whole identity, the order of the burst and the cadence of
  everything periodic. It ran on aditi from two regions and on OpenSim;
  scripted `sl-repl` logins with the wire and capability traces filled in
  what no event carries.
- **Findings.** Both grids open an arrival with `AgentDataUpdate`,
  `RegionHandshake`, `AgentMovementComplete`. Second Life names the product,
  its SKU and the data centre; OpenSim sends all three empty, so its product
  type is `Unknown`. Second Life sends `SimStats` every 2 s (35 ids) and the
  time every 10 s with a sun direction; OpenSim every 3 s (41 ids) and every
  2.55 s with none. Second Life greets a child circuit with two handshakes,
  announces each neighbour again about every five seconds, and sends an
  arrival a `HealthMessage` and an `AgentStateUpdate`; OpenSim does none of
  the four. No statistic id above 40 appeared on either grid, and
  `AgentStateUpdate` arrives over the event queue, not UDP.
- **Client.** `RegionIdentity::colo_name` (the handshake's `ColoName`, which
  was dropped).
- **Fake grid.** `ImitatedGrid::arrival_policy`: per-flavour product, data
  centre, CPU class and billable factor; the doubled child handshake; the
  `HealthMessage` and `AgentStateUpdate`; and periodic `SimStats` / time
  through the new `SimSession::set_region_telemetry`. Both flavours now send
  the opening `AgentDataUpdate` (`SimSession::send_agent_data_update`) and
  the live grids' stock region flags instead of zero. `region-arrival` is in
  the offline list for both flavours.
- **Viewer.** Two e2e checks of the About window's Product line, one per
  flavour (`e2e_live_checks.rs`).
- **Not done here.** The fake grid does not repeat a neighbour's
  announcement as Second Life does, sends no `CoarseLocationUpdate` on a
  timer and no `CameraConstraint`, and its statistics and sun stand still
  ([[server-world-heartbeat]]). The session reports `NeighborDiscovered`
  again for every repeat ([[protocol-neighbor-discovered-repeats]]).
  `RegionInfo5` is not a handshake block; its chat ranges belong to
  [[gridspec-region-info]].

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
