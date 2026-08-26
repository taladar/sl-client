---
id: test-audit-fake-grid-session-teardown
title: sl-fake-grid never prunes sessions, and its tasks outlive shutdown
topic: test
status: bugs
origin: static code audit (2026-08-26)
points: 5
refs: [test-audit-fake-grid-conformance-grid]
---

Context: [context/test.md](../context/test.md).

`sl-fake-grid/src/runtime.rs:228`, `:409` — `GridCore::sessions` is **never
pruned on logout or disconnect**. `remove_session` (`:428`) has exactly two
callers, both in `teleport.rs` (`:170`, `:191`).

Each stale entry holds an `Arc<UdpSocket>` (a leaked fd), a deep clone of the
scenario asset store, and the stock 851968-byte terrain RAW — roughly 1 MB plus
one fd per login, forever, in the long-running `sl-fake-grid` binary.

Consequence beyond the leak: `GridCore::root_session_of` (`:434-444`) can return
a **closed** session, because `SimSession`'s `agent_presence` is never reset.
After a relogin two entries report `is_root_agent() == true` and `HashMap` order
decides which one a lure teleport targets.

Tasks that outlive `FakeGrid::shutdown()`:

- `teleport.rs:372-412` — `run_teleport_responder` awaits only `events.recv()`
  and exits on `Closed`, which can never fire because the task itself owns
  `shared` (and therefore `events_tx`, `driver.rs:74`);
- `http_service.rs:49-59` — `serve_connection` tasks are spawned with the handle
  dropped and never observe `shutdown_rx` (only the accept loop does, `:61-66`),
  so a held `EventQueueGet` poll survives teardown by up to 30 s. There is also
  **no read/idle/header timeout and no connection cap**, so a peer that connects
  and sends nothing pins a task and an fd indefinitely;
- `driver.rs:255-262` — a UDP receive error is `continue`d with no backoff, so a
  persistently-failing socket spins the pump at 100% CPU. (Whether tokio
  surfaces `ECONNREFUSED` here on loopback is unverified.)

Scope: hook `remove_session` to the driver's `closed_tx` flip (`driver.rs:176`)
— that fixes the leak and the stale-`root_session_of` lure bug in one change —
and give the responder and connection tasks a shutdown branch.

Minor, same crate: `login_endpoint.rs:68-97` — `prepare_session` binds a UDP
socket, consumes a session sequence number, builds a full `SimSession` and deep
clones the scenario **before** `LoginServer::respond` (`:97`) checks the
password or the gates, so a wrong-password POST does all that work and discards
it — and, given the leak above, contributes to it.

For the record: the fake grid does **not** duplicate `sl-proto`'s server side.
It is one `SimSession` plus one `SimCaps` per circuit, with acks, resends,
throttles and dispatch all living in `sl-proto`. The one real duplication is the
teleport ordering contract (`teleport.rs:118-163`), hand-rolled three more times
in `sl-proto/tests/sim_session.rs` — two independent authorities for a sequence
that can drift.
