---
id: test-handover-mock-grid-harness
title: Mock-grid simulator harness for deterministic handover/timeout testing
topic: test
status: ready
origin: user request (2026-08-07) — timeouts and lost-message races can't be
  deterministically provoked against a real grid
refs: [protocol-teleport-deferred-teardown-handover, protocol-teleport-timeout-strands-child-circuits]
---

Context: [context/viewer.md](../context/viewer.md).

The existing `sl-proto/tests/lifecycle.rs` teleport/crossing tests hand-craft
one server response per scenario. To test the handover **state machine**
comprehensively — timeouts, lost `AgentMovementComplete`/`RegionHandshake`, and
overlapping transfers — we need to drive the real sans-IO `Session` against a
small mock grid with fault injection, since none of that can be provoked
on-demand against a live simulator.

## Scope

A test-only `MockGrid` harness (in `lifecycle.rs`'s test module, reusing
`server_message` / `region_handshake_msg` / `enable_neighbour_b` /
`teleport_finish_to_sim_b` / `decode`):

- A small grid of regions, each with a `sim_addr`, region handle, and neighbour
  list; routes the client's `poll_transmit()` datagrams by `sim_addr` and feeds
  back the correct server replies (`AgentMovementComplete` + `RegionHandshake`
  to a `CompleteAgentMovement`; `TeleportFinish` -> a chosen dest to a
  `TeleportLocationRequest`; neighbour `EnableSimulator`), over the **UDP** path
  to avoid LLSD construction (the CAPS path decodes into the same handlers).
- **Fault-injection knobs**: drop/delay `TeleportFinish`,
  `AgentMovementComplete`, `RegionHandshake` (per-region), to force each timeout
  / lost-message case.
- A bounded `pump(session, now)` loop that drives client<->grid to quiescence,
  and a clock the tests advance to trip `handle_timeout`.

Validate the harness first against the **current** `Session` (a normal
fresh/promote teleport succeeds), then reuse it to drive the
[[protocol-teleport-deferred-teardown-handover]] refactor TDD-style.

`SimSession` (the server side) is a mature single-region building block but does
not emit cross-region `TeleportFinish`, model a multi-region grid, or inject
faults — hence a focused purpose-built harness rather than composing
`SimSession`s.
