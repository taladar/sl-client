---
id: gridspec-circuit
title: Circuit behaviour: acks, resends, pings, inactivity, packet quirks and throttles
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-simulator-core]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

Ping round trip about 1.2 ms on local OpenSim, about 170 ms on aditi. SL
sends zero-coded messages whose final zero run expands one byte short
(handled client-side). Both grids accept `AgentThrottle` with no reply.

## Discover

- An `sl-repl --script` probe that goes silent: when does each grid stop
  resending, ping, and drop the circuit (inactivity timeout); resend count and
  interval of an unacked reliable packet; the simulator's `StartPingCheck`
  cadence on root and child circuits. Wire trace via `sl-conformance-trace`.
- Whether the throttle actually changes send rates (low priority; OpenSim's
  token buckets are in source).

## Document

`book/src/gridspec/session.md` § Circuits.

## Fake grid

Small — in this task: per-flavour timeouts / cadence in the `SimSession`
driver; optionally inject SL's short zero-code tail on the SL flavour.
Honouring throttles is large: [[server-world-update-scheduling]].

## Viewer

Survives aditi-like latency and the truncated tail (decoder handles it); a
fake-grid tier with injected latency.
