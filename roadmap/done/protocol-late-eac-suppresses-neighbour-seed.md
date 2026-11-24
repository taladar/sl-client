---
id: protocol-late-eac-suppresses-neighbour-seed
title: A late EstablishAgentCommunication may suppress a neighbour's next seed
topic: protocol
status: done
origin: mixed-channel race audit (2026-11-23)
---

Context: [context/protocol.md](../context/protocol.md).

Plausible and not reproduced. Specific to Second Life, because it depends on a
neighbour reusing its seed.

## Bad order

Event-queue `EstablishAgentCommunication` lands after UDP `DisableSimulator`
for the same neighbour.

## Cause

The handler in `sl-proto/src/session/methods.rs` stores the seed for a child
that no longer exists. When the neighbour returns with the same seed, no
`NeighborSeed` is emitted.

## Effect

That neighbour's capability map is never fetched again.

## Fix

Emit a held seed again when `announce_neighbour` opens a new child.

## Fix (2026-11-24)

Fixed differently from the plan above: the late seed is never held. The
`EstablishAgentCommunication` handler now drops a seed for a simulator with no
open circuit, which is what Firestorm does for an unknown region
(`LLEstablishAgentCommunication::post` in `llworld.cpp`).

- A teleport destination is always a child circuit before its seed matters, so
  it is unaffected.
- Second Life repeats the event every few seconds, so a seed that arrives before
  `EnableSimulator` opens the child is resent anyway.

Test: `a_seed_behind_its_neighbours_retirement_is_dropped`
(`sl-proto/tests/lifecycle.rs`).
