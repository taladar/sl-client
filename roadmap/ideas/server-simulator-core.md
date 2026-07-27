---
id: server-simulator-core
title: Simulator core — scene authority, persistence, broadcast
topic: server
status: ideas
origin: user request (2026-07) — size what a real server would involve
refs: [protocol-sim-caps-framework, protocol-sim-udp-flows]
---

Context: [context/server.md](../context/server.md).

The big one: the simulator process that owns regions. Everything the
protocol topic deliberately leaves to "the consumer" lands here:

- **the scene**: the authoritative object graph (prims/linksets with
  full `ObjectUpdate` state, attachments, avatars), terrain, parcels
  with their flags/access rules, estate settings and enforcement,
  environment (EEP) state;
- **persistence**: region content to a DB (OpenSim's prims/primshapes
  shape), terrain snapshots, parcel/estate tables, crash-safe periodic
  writes, OAR-style import/export for content;
- **multi-client broadcast + interest management**: per-agent view
  culling, update prioritisation, throttles, coarse locations — turning
  one authoritative change into N tailored update streams;
- **the I/O shell**: the per-region LLUDP socket loop over `SimSession`
  instances (one per connected agent, plus child agents), the per-region
  CAPS HTTP server over the `SimCaps` dispatch, timers/heartbeat
  (`SimStats`);
- object interaction semantics: rez/derez/link/edit with the permission
  system applied server-side, task inventory, touch/sit routing into
  script events.

Physics ([[server-physics-integration]]) and scripts
([[server-script-engine]]) plug into this; agent arrival/departure is
[[server-agent-transfer]]'s protocol. `sl-fake-grid` is the embryo: same
wire stack, no authority.
