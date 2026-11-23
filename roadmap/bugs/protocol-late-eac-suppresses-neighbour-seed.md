---
id: protocol-late-eac-suppresses-neighbour-seed
title: A late EstablishAgentCommunication may suppress a neighbour's next seed
topic: protocol
status: bugs
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
