---
id: gridspec-circuit
title: Circuit behaviour: acks, resends, pings, inactivity, packet quirks and throttles
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-simulator-core, server-world-update-scheduling,
  protocol-variable-block-lists-over-255, viewer-disconnect-screen,
  viewer-statistics-ping, gridspec-object-update-stream]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Done (2026-10-06)

Measured and written up in `book/src/gridspec/session.md` § Circuits.

- **Discover.** The session can now be told to probe its own circuits
  (`Command::ProbeCircuits`, `probe_circuits` in the REPL): report every
  inbound datagram as a `Diagnostic::Datagram`, withhold every
  acknowledgement, or transmit nothing at all. Three cases use it —
  `keepalive-ping` (both ends' ping cadence, root and child),
  `circuit-unacked-resend` and `circuit-silence` — and `throttle-set` now
  measures a re-requested burst of objects under two presets. Each ran on
  aditi and on OpenSim.
- **Findings.** Both grids ping every circuit about every five seconds
  (5.1 s and 5.28 s), and go on pinging a client that answers none. Second
  Life sends an unacknowledged packet four times, a second apart, and gives
  it up; OpenSim resends it every 0.32 s without limit (140 times in 45 s).
  Second Life retransmits almost nothing — 6 of 5,844 reliable packets — and
  sends unacknowledged object state again as new packets instead. A silent
  client is dropped by Second Life after 100 s without a word, and kicked by
  OpenSim after 60 s ("Simulator logged you out due to connection
  timeout."); a login five seconds later gets in on both, one made in the
  same instant was refused by OpenSim as `presence`. OpenSim holds a burst
  to the throttle's task rate (9.2 kbps of the 10 asked for); Second Life
  sends less when asked for less (288 kbps against 68) but not as little as
  asked.
- **Fake grid.** `ImitatedGrid::circuit_policy`: each flavour's inactivity
  timeout, resend budget and timeout, and timeout kick, through the new
  `sl_proto::LinkTuning` and `SimSession::set_timeout_kick`. `FakeSl` sends
  every `ObjectUpdate` zero-coded with Second Life's short tail.
  `FakeGridBuilder::link_latency` delays every datagram each way. All three
  circuit cases and `throttle-set` are in the offline list for both
  flavours.
- **Viewer.** Two e2e logins over a 170 ms round trip, one per flavour
  (`e2e_login.rs`). `Command::RequestObjects` for more than 255 objects
  failed to encode and ended the session; it is split across messages now,
  and the same limit in the other object-list requests is
  [[protocol-variable-block-lists-over-255]].
- **Not done here.** The fake grid does not honour throttles
  ([[server-world-update-scheduling]], as planned) and does not imitate
  Second Life's way of sending unacknowledged object state again as new
  packets (noted there too). No e2e test sits through a simulator going
  silent: it takes the session's 45-second inactivity timeout, and what the
  viewer shows then is [[viewer-disconnect-screen]]. The viewer displays no
  ping at all ([[viewer-statistics-ping]]).

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
