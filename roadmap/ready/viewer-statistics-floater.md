---
id: viewer-statistics-floater
title: Statistics floater (viewer + sim stats, lag meter)
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-widget-scaffold]
refs: [viewer-debug-consoles]
---

Context: [context/viewer.md](../context/viewer.md).

The statistics floater (the reference's Ctrl+Shift+1): collapsible stat
groups with per-stat sparkline history —

- **Viewer**: FPS, frame time, bandwidth in/out, packet loss, ping, texture
  / mesh / asset queue depths (the pipeline-status API already tracks the
  fetch pipelines), draw calls / triangle counts, VRAM use (fold the FS
  per-object VRAM floater's totals in here).
- **Simulator** (`SimStats`, decoded since `missing-batch-1`): time
  dilation, sim FPS, physics FPS, agents / child agents, active scripts and
  script time, net stats, spare time — the numbers every "is it me or the
  sim" diagnosis needs.
- A condensed **lag meter** view (the reference's traffic-light
  client/network/server summary) as a compact mode of the same floater.

Reference (Firestorm, read-only): `llfloaterstats`, `llstatgraph`,
`llfloaterlagmeter`, `floater_scene_load_stats.xml`.

Builds on: `SimStats` decode, the pipeline-status API (`render_priority.rs`
/ `diagnostics.rs`), Bevy frame diagnostics.

## Parity-audit addendum (2026-08-19)

Addition from the audit: the OpenSimExtras `SimulatorFPS`,
`SimulatorFPSFactor`, `SimulatorFPSWarnPercent` and
`SimulatorFPSCritPercent` values (`lfsimfeaturehandler.cpp`) scale the
sim-FPS lag-meter thresholds on OpenSim grids whose simulators do not
run at SL's nominal 45 fps; the body has no OpenSim sim-FPS scaling
mention. Wire decode already exists in `sl-wire/src/sim_features.rs`.
