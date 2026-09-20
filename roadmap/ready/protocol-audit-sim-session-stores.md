---
id: protocol-audit-sim-session-stores
title: SimSession's 54 fields mix the driver's serving stores with the circuit
topic: protocol
status: ready
origin: static code audit (2026-08-26)
points: 8
refs: [protocol-audit-session-god-object, protocol-audit-extract-lludp-transport]
---

Context: [context/protocol.md](../context/protocol.md).

The server half of [[protocol-audit-session-god-object]], left for its own
round. `SimSession` (`sl-proto/src/sim_session.rs`) is ~54 fields, about 35 of
them **driver-populated serving stores** — what this simulator would answer
with (`region_materials`, `object_media`, `object_costs`, `environments`,
`parcels`, `experiences`, the two inventory trees, …) — sharing one struct with
the live circuit state (its [`ReliableLink`], the arrival/teleport bookkeeping,
the timers) and with the outbound queues.

The two halves have different lifetimes and different owners: a serving store
is written by the driver before or between sessions and read on request, while
the circuit state is the session's own and dies with it. They read as one flat
struct today, so nothing says which is which.

Shape the client side settled on (`WorldCache`, `Transfers`): extract a **type**
per group that owns the fields *and* the operations that have to touch all of
them at once, and leave the I/O — the events, the diagnostics, the sends — on
the session. Candidate groups, to be confirmed against the code:

- the serving stores the driver seeds (one type, or one per subsystem where a
  group has real operations of its own rather than being a map);
- the per-agent / per-circuit session state the simulator keeps for the client
  it is talking to.

Note the constraint that shapes this: a per-area module split of
`impl SimSession` is not available (see "Decomposing a god object here" in
[context/protocol.md](../context/protocol.md)), so the win has to come from the
types, not from moving methods between files.

`SESSION_FLOW_COVERAGE` and its pinned-table test must keep passing unchanged —
it is the parity contract, not an implementation detail.
