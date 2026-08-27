---
id: protocol-audit-sim-session-lifecycle
title: SimSession accepts identity rebinding, never validates a session id, and frees nothing on close
topic: protocol
status: bugs
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
