---
id: test-throttle-set
title: apply a Throttle preset and confirm it is accepted
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 1 — Session lifecycle & circuit `[both] 1av`
---

Context: [context/test.md](../context/test.md).

`throttle-set` — apply a `Throttle` preset and confirm it is accepted.
`AgentThrottle` is fire-and-forget (no protocol reply), so acceptance is the
*absence* of failure: the reliable packet is acked by the sim rather than
retransmitted to exhaustion (which would close the circuit). The case applies
the 500 kbps preset and watches the circuit past the retransmit budget (~9 s)
via keep-alive pings; a healthy ping past that point plus no `AgentThrottle`
reply-missing diagnostic confirms acceptance. Green on both grids.
