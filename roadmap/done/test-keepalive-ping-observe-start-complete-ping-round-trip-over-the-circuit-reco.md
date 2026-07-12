---
id: test-keepalive-ping
title: observe start/complete ping round-trip over the circuit; record RTT
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 1 — Session lifecycle & circuit `[both] 1av`
---

Context: [context/test.md](../context/test.md).

`keepalive-ping` — observe start/complete ping round-trip over the
circuit; record RTT. The session now sends a periodic `StartPingCheck` on
every circuit — root and child (the reference viewer's ~5 s circuit ping) —
and surfaces each `CompletePingCheck` as `Event::Ping { sim, child, rtt }`.
The case asserts the root ping (`child: false`, the "ping to sim"); recorded
RTT ≈ 1.2 ms on loopback OpenSim, ≈ 170 ms on Aditi.
