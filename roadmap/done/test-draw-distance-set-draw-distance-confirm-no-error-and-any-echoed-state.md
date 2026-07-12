---
id: test-draw-distance
title: set draw distance; confirm no error and any echoed state
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 1 — Session lifecycle & circuit `[both] 1av`
---

Context: [context/test.md](../context/test.md).

`draw-distance` — set draw distance; confirm no error and any echoed
state. The draw distance rides the `Far` field of the unreliable keep-alive
`AgentUpdate` (no reply), so the simulator folds it into the agent's interest
list and enables the neighbouring regions it reaches, each surfaced as
`Event::NeighborDiscovered`. The case applies a 512 m draw distance (double
the 256 m default), then observes the circuit for a window: a keep-alive ping
that still round-trips is the "no error" signal, and the neighbour
announcements are the echoed state. OpenSim is a 2×2 block of adjacent
regions, so 512 m always reaches its neighbours — green with
`neighbors_count = 3`. Aditi's landing region had no neighbours within reach,
recorded `partial` (`neighbors_count = 0`) with the circuit healthy.
