---
id: protocol-audit-sim-session-lifecycle
title: SimSession accepts identity rebinding, never validates a session id, and frees nothing on close
topic: protocol
status: done
origin: static code audit (2026-08-26)
points: 5
---

Context: [context/protocol.md](../context/protocol.md).

The server-side session (`sl-proto/src/sim_session.rs`) has several holes. It is
a fixture rather than a production simulator, so severity is bounded — but the
consequence is that the harness cannot test the rejection paths, because there
are none.

- `:7536` — a repeated `UseCircuitCode` on a live circuit sets `agent_id`,
  `session_id` and `circuit_code` **unconditionally**; only the state/ping
  transition is guarded. No handler in the 132-arm dispatcher validates an
  inbound `AgentData.session_id` against `self.session_id`, and
  `handle_datagram:7502` binds `client_addr` to the first parseable sender.
- `:7551` — `CompleteAgentMovement` is honoured in any state, including before
  `UseCircuitCode`. It sets `agent_presence = Root` while `state` stays
  `AwaitingCircuit` and the ping timer is never armed.
- `:9126` — `close()` sets a state and nulls two timers. It frees nothing:
  `unacked`, `out`, `caps_events`, `xfer_*`, `transfer_serves`, `chat_sessions`
  and `script_grants` all survive, and `poll_transmit` (`:9097`) checks only
  `client_addr`, so queued datagrams still go out after `Disconnected`. None of
  the ~90 `send_*` / `enqueue_*` helpers check `is_closed()`.
- `:8582` / `:7379` / `:7913` / `:8664` — four network-driven unbounded stores:
  `SendXferPacket` extends a receive buffer with no size cap, ordering check or
  timeout; `caps_events` is a `Vec` drained only by a long poll; `script_grants`
  is insert-only; `transfer_serves` is reaped only by an explicit abort.
- `:6853` — `SimSitState::ResponseSent` has no timeout and no cancel, so the sit
  handshake can hang forever. The client side has `SIT_TIMEOUT`
  (`session.rs:52`); the pair is asymmetric.
- `:7441` / `:9083` — the resend policy is the one the client shed in
  [[protocol-audit-reliable-exhaustion-kills-session]]: a flat 1500 ms timeout
  that never adapts, a clock started at enqueue rather than at transmit, and
  `close(ServerEvent::Disconnected)` for *any* exhausted reliable packet
  whatever it carried. The fixture measures no round trip at all, so porting the
  client's ping-averaged timeout means giving it one.

## Fixed (2026-08-28)

All six parts, on `SimSession`.

**Identity.** `UseCircuitCode` binds the agent/session/circuit triple exactly
once. A repeat carrying the *same* triple is the client re-sending a packet it
believes was lost and is answered again; a repeat carrying a different one is
refused as `RejectionReason::CircuitRebind` and changes nothing. The circuit's
UDP endpoint is claimed by that same opening packet rather than by the first
parseable datagram from anywhere, so an unrelated host that gets in first no
longer takes the address the circuit then answers on.

**Session ids.** Rather than 132 hand-written checks that would drift, the
check is derived from the message template: `sl-wire`'s generator now emits
`AnyMessage::agent_session_id()`, the `SessionID` of a message's single
`AgentData` block (237 of the 483 messages have one). `handle_datagram`
consults it once, before dispatch, and refuses a message asserting any session
but the circuit's (`SessionIdMismatch`). `UseCircuitCode` carries its ids in a
`CircuitCode` block, so it is naturally exempt — it establishes what the others
are checked against. Anything arriving before the circuit is open is refused at
the endpoint bind (`NoCircuit`), and `CompleteAgentMovement` additionally has
to find the circuit `Active`.

Every refusal is surfaced as `ServerEvent::Rejected { message, reason }`, so
the harness can finally assert on a rejection path instead of inferring it from
silence.

**Close.** `close()` now frees the per-connection stores (`unacked`,
`pending_acks`, `caps_events`, the three `xfer_*` maps, `transfer_serves`,
`chat_sessions`, `script_questions`, `script_grants`, `offline_messages`, the
two pending uploads) and `send()` — the single funnel all ~90 typed helpers go
through — refuses once closed. The outbound queue is deliberately *not*
dropped: a clean logout and a retired circuit both queue their goodbye packet
and then close, so the queue drains and, with nothing able to queue behind it,
stays drained.

**Bounds.** `SendXferPacket` is checked against the expected packet number (an
out-of-order packet is refused rather than concatenated) and against a 16 MiB
ceiling (past which the pull is aborted); both directions of `Xfer` are reaped
after `XFER_STALL_TIMEOUT`. `caps_events` is capped at 4096 with the oldest
dropped and logged. `script_grants` is capped at 4096. `transfer_serves` now
carries a 60 s serve deadline, past which the simulator answers the request
itself with an `UnknownSource` `TransferInfo`.

**Sit.** A `ResponseSent` offer arms a 15 s deadline — the mirror of the
client's `SIT_TIMEOUT` — and is withdrawn as `ServerEvent::SitOfferExpired` if
the completing `AgentSit` never comes.

**Resend policy.** The client's policy, ported: a `SimReliableSeverity` per
unacked packet (`RegionHandshake` and `AgentMovementComplete` are
session-critical, everything else best-effort, reported as
`ServerEvent::ReliableGiveUp` with the session left running); a ping-averaged
timeout (`max(1 s, 5 x ping_average)`) fed by measuring the round trip of the
periodic `StartPingCheck` the fixture previously discarded, with an unanswered
ping's time in flight folded in when the next one is due; and a transmit-time
retransmission clock, so a datagram waiting on a backed-up driver does not have
its queue time counted as silence from the client. `MAX_RESEND_ATTEMPTS` drops
from 6 to 4, matching the client.

Twelve tests in `tests/sim_session.rs` covering each of the above, with an
`idle_sim` helper that stands in for a live but idle client (keep-alives,
acknowledgements, ping answers) so a timeout can be reached without the
inactivity timer firing first.

The same unbounded-buffer hole on the *client's* `Xfer` download is filed
separately as [[protocol-audit-client-xfer-download-bounds]].
