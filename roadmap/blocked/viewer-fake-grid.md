---
id: viewer-fake-grid
title: A loopback fake grid over SimSession
topic: viewer
status: blocked
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
