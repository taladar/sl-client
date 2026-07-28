---
id: server-architecture
title: Grid architecture — topology, service protocol, deployment
topic: server
status: ideas
origin: user request (2026-07) — size what a real server would involve
refs: [protocol-sim-caps-framework, protocol-sim-login]
---

Context: [context/server.md](../context/server.md).

The umbrella design idea for an actual grid: which processes exist, what
they own, and how they talk.

- **Topology**: grid-level services (login, grid/region registry,
  assets, inventory, accounts/identity, presence, friends, groups,
  message routing, map, economy, search) vs **simulators** (one process
  hosting one or more regions), deployable on separate hosts. Standalone
  mode (everything in one process, OpenSim-style) should fall out of the
  same service traits behind in-process implementations.
- **Service-to-service protocol + auth**: how a simulator authenticates
  to the asset/inventory/presence services and how services trust each
  other (OpenSim uses REST + shared secrets; we would design this
  deliberately — likely HTTP/LLSD reusing `sl-wire`, with proper
  service credentials).
- **Data layer**: per-service storage choice (SQLite for standalone,
  something server-grade for multi-host), migrations, backups.
- **Deployment**: configuration for multi-host grids, service discovery
  (the grid service doubles as the registry), health/heartbeat.

Deliverable if promoted: an architecture document in `book/` plus the
service-trait skeleton crate the other `server-*` tasks would fill in.
