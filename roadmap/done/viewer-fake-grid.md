---
id: viewer-fake-grid
title: A loopback fake grid over SimSession
topic: viewer
status: done
origin: user request (2026-07) — full client testing without a real server
points: 8
refs: [viewer-world-test-harness, protocol-sim-login, protocol-sim-http-misc,
  protocol-sim-udp-flows]
blocked_by: [protocol-sim-caps-framework]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-proto`'s `SimSession` is a sans-I/O simulator session (handshake,
reliability, decode, ~70 send helpers, CAPS event-queue builders),
`sl-wire/src/login.rs` is a full login *server*, and
`sl-proto/tests/sim_session.rs` already runs the in-memory loopback. Build
the missing glue as a workspace test-support crate (`sl-fake-grid`):

- an HTTP login endpoint over `LoginServer::respond`
  ([[protocol-sim-login]] raises its fidelity);
- CAPS seed + `EventQueueGet` long-poll HTTP via the `SimCaps` dispatch
  from [[protocol-sim-caps-framework]];
- a localhost UDP pump around `SimSession`;
- scriptable content fixtures (region, objects, inventory).

Everything in-process on ephemeral ports so tests stay parallel. Two
consumers by design: integration tests, and a standalone binary an
unmodified viewer (ours or Firestorm) can log into — the highest-fidelity
manual-testing tool this workspace can have short of a grid. Grows richer
as each `protocol-sim-caps-*` cluster lands (refs, not blockers, beyond
the framework).

Done (2026-08-21): the `sl-fake-grid` crate landed — hyper HTTP glue
(login at `/` serving both XML-RPC and LLSD keyed on Content-Type,
per-session CAPS under `/sim/<n>/cap/<token>`, the 30 s `EventQueueGet`
hold woken by enqueues or answered 502), one loopback UDP socket plus a
pump/timer task pair per logged-in session around `SimSession`, and
`Scenario` fixtures (setup closure, on-arrival hook, an
`InMemoryAssetSource`). The design invariant worth keeping: every
mutation path funnels through one flush rule (drain events → broadcast,
collect transmits, republish the timer deadline, wake the event queue)
with socket I/O strictly outside the state lock. `FakeGridBuilder` →
`FakeGrid` / `FakeAgent` (`with_sim`, `events()`) is the test API; the
`sl-fake-grid` binary serves an unmodified viewer (grid manager URI
`http://127.0.0.1:<port>/`). Fixed en route: the XML-RPC login builder
never emitted `agent_appearance_service`. Tests: `tests/http_glue.rs`
(reqwest + the client-direction codecs: credentials, both login codecs,
seed idempotence, the full EQG contract) and `tests/client_end_to_end.rs`
(the real `sl-client-tokio` stack: login → circuit → chat both ways →
a CAPS event through the long-poll; two grids in parallel). Book:
`book/src/tools/fake-grid.md`. The region model is N-region from the
start, but only single-region sessions are exercised; the teleport
helper and inter-region visibility are follow-ups, as is growing the
stock scenario against what Firestorm actually requests. Unblocks
[[viewer-fake-grid-login-smoke]] (moved to ready).
