# Server topic — an actual grid/simulator implementation (exploratory)

Status: **exploratory**. The user is not (yet) committed to building a
server; the `server-*` idea tasks exist to map what a real implementation
would involve, at subsystem granularity, so the effort can be sized and
the protocol-surface work ([context/protocol.md](protocol.md), the
`protocol-sim-*` tasks) can be steered toward genuine reuse.

## Boundary with the protocol topic

The `protocol` topic owns the **wire surface**: bidirectional codecs,
sans-I/O sessions (`Session`/`SimSession`), server-direction CAPS/login
builders, flow state machines. This topic owns everything the protocol
topic deliberately excludes: **world authority, persistence,
multi-client broadcast, service processes, and the socket/event-loop
I/O** — the running grid. `sl-fake-grid` (the loopback test grid) is the
half-way point: real I/O glue, scripted content, no authority.

## Reference architecture

Two primaries, both in the read-only third-party trees:

- **OpenSim** (`~/devel/3rdparty/opensim`, `opensim-core`): the ROBUST
  service split (login, grid, assets, inventory, accounts, presence,
  friends, groups, IM/offline-IM, map, estate, …) with simulators as
  separate processes registering regions against the grid service, and
  simulator-to-simulator agent handoff for teleport/crossing. Closest
  existing model for "multiple simulators on potentially separate
  hosts".
- **Second Life's grid** (observed behaviour + Firestorm sources): the
  login server is a separate endpoint from the simulators; assets,
  bakes, display names, experiences, etc. are separate HTTP services
  fronted by per-region CAPS grants.

## Conventions for this topic

- One idea task per major subsystem; bodies stay high-level (what it
  does, what it stores, what talks to it, what we already have) — no
  implementation detail until a task is deliberately promoted out of
  `ideas/`.
- `server-architecture` is the umbrella: topology, service-to-service
  protocol/auth, deployment. Read it first.
- Everything reuses the sans-I/O core: a service speaks the wire via
  `sl-wire`/`sl-proto`; new service-internal protocols are a design
  decision recorded in `server-architecture`, not per-task improvisation.
