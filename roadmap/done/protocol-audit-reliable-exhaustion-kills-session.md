---
id: protocol-audit-reliable-exhaustion-kills-session
title: One exhausted reliable packet of any kind closes the session as HandshakeFailed
topic: protocol
status: done
origin: static code audit (2026-08-26)
points: 2
---

Context: [context/protocol.md](../context/protocol.md).

`sl-proto/src/session/methods.rs:5030` calls
`self.close(DisconnectReason::HandshakeFailed)` for **any** reliable packet past
`MAX_RESEND_ATTEMPTS` (6 attempts at a flat 1500 ms, `session.rs:41-43`). A lost
`ObjectSelect` or a chat message therefore tears down the whole session and
reports it as a handshake failure — and `types/session.rs` documents the reason
as "a reliable **handshake** packet".

Two things to separate here: per-message severity (only the handshake-critical
packets should be fatal), and the resend policy itself. The timer never backs
off and never adapts, even though `record_ping_reply` (`circuit.rs:523`)
measures RTT and throws it away. `circuit.rs:398` also starts the retransmit
clock at **enqueue** rather than at transmit (`send()` records `sent_at: now`
while the datagram only enters `self.out`), so a backed-up driver makes every
packet look overdue and triggers spurious resends.

## Fixed (2026-08-27)

All three parts, on the client `Session`. The simulator fixture
(`sim_session.rs`) still has the flat timeout, the enqueue-time clock and the
close-on-any-exhaustion — noted on [[protocol-audit-sim-session-lifecycle]]
rather than fixed here.

**Severity.** A new `ReliableSeverity` rides on each unacked packet, set from
the message at `send()` time: `SessionCritical` for the three packets that
establish the agent on a circuit (`UseCircuitCode`, `CompleteAgentMovement`,
`RegionHandshakeReply`), `BestEffort` for everything else. Only the first kind
closes the session, so `DisconnectReason::HandshakeFailed` finally means what
its doc says; a lost chat line or selection is reported as its
`ExpectedReplyMissing` diagnostic and the session carries on. This matches the
reference viewer, where `LLCircuitData::resendUnackedPackets` only invokes the
packet's own failure callback (`LL_ERR_TCP_TIMEOUT`) and leaves the circuit
alone — a dead link is the inactivity timeout's job to declare.

**Adaptive timeout.** The circuit now keeps `ping_average`, the reference
viewer's fast-attack / slow-decay relaxation of the measured round trip
(`setPingDelay`: jump to any worse sample, then relax by `α = 0.2`, clamped to
100 ms..2 s, starting at 1 s). `record_ping_reply` feeds it instead of
discarding the measurement, and the retransmission timeout is
`max(1 s, 5 × ping_average)` — `LL_MINIMUM_RELIABLE_TIMEOUT_SECONDS` and
`LL_RELIABLE_TIMEOUT_FACTOR`. `MAX_RESEND_ATTEMPTS` drops from 6 to 4, the
reference's first transmission plus `LL_DEFAULT_RELIABLE_RETRIES`.

The reference's other adaptation is `getPingInTransitTime`, which inflates the
averaged ping *while* pings go unanswered. Reading it needs `now`, and
`poll_timeout` has none — a `now`-dependent timeout there would also let
`next_resend_deadline` disagree with `process_resends` and spin the driver. So
the same signal is folded in at a point that does have `now`: when the
keep-alive timer fires with the previous ping still outstanding, its time in
flight is recorded as a sample. A simulator that stops answering therefore
widens the timeout (to the 10 s its 2 s ceiling implies) instead of drawing ever
more retransmissions onto a struggling link.

**Transmit-time clock.** The outbound queue carries `Outbound { sequence,
payload }` and an unacked packet carries `queued`. `poll_transmit` — via the new
`Circuit::pop_outbound` — clears `queued` when the datagram is actually handed
to the driver; until then `process_resends` holds `sent_at` at the latest `now`
and `next_resend_deadline` ignores the packet entirely. Time in the queue thus
never counts as silence from the simulator. Keeping the stamp inside the two
methods that already take `now` is what avoids widening the public
`poll_transmit()` signature (and the ~640 test call sites of the drain helper
behind it) for a clock the session is already told about every driver
iteration.

Four tests in `tests/lifecycle.rs`: an exhausted chat is reported but leaves the
session running, an exhausted `UseCircuitCode` still closes it as
`HandshakeFailed`, a queued datagram is never retransmitted and starts its clock
only once drained, and a slow measured round trip widens the timeout past the
five seconds an unmeasured circuit waits.

The two conformance cases that inferred "the simulator acked it" from the
circuit *surviving* the retransmit budget — `throttle-set` and
`group-accounting` — now assert the absence of the exhaustion diagnostic
instead, since survival no longer proves anything, and their accept window grows
to cover the worst-case 4 × 10 s budget.
