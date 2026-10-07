---
id: protocol-neighbor-discovered-repeats
title: NeighborDiscovered is reported again every time Second Life re-announces a neighbour
topic: protocol
status: bugs
origin: gridspec-region-arrival (2026-10-07)
refs: [gridspec-region-arrival, gridspec-neighbours-crossing]
---

Context: [context/protocol.md](../context/protocol.md).

## Observation

Second Life sends `EnableSimulator` (and `EstablishAgentCommunication`) for
each neighbour again about every five seconds: six of each per neighbour in
the first 32 s of a login on aditi (`region-arrival`, 2026-10-07). OpenSim
sends each once.

`Session` handles a repeat correctly as far as the circuit goes —
`open_child_circuit` returns early for a simulator it already holds — but
pushes `Event::NeighborDiscovered` (and `Event::NeighborSeed`) every time. A
consumer that takes the event at its word sees a neighbour "discovered" a
dozen times a minute, and a driver may re-POST the neighbour's seed capability
as often.

## Wanted

Report a neighbour once: when its circuit is opened, or when what was
announced differs from what is held (a new seed capability, a new address
for the same handle). Check what the tokio and Bevy drivers do with a
repeated `NeighborSeed` before deciding whether the seed re-fetch is wanted.
