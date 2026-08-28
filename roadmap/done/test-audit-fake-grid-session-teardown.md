---
id: test-audit-fake-grid-session-teardown
title: sl-fake-grid never prunes sessions, and its tasks outlive shutdown
topic: test
status: done
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

## Resolution

**A session is a lifetime, not an entry.** The table held sessions because
nothing owned the question "is this one still alive?". Now every activation
spawns a fourth task beside the pump, the timer and the teleport responder — a
reaper that waits on the same per-session closed watch the flush rule already
flips (logout, inactivity, retirement, abandonment) and then removes the entry.
The two hand-placed `remove_session` calls in `teleport.rs` still stand and are
simply redundant: they run at the moment the source is retired rather than a
scheduling hop later, which the teleport's own ordering wants.

**A closed circuit still says it is the root agent.** `SimSession` never resets
`agent_presence` on close, so pruning alone would still leave a window — and
`root_session_of` is what a lure with an opaque id resolves against. It now
skips a closed session outright, which makes the answer right *before* the
reaper runs rather than eventually. The crate test drives a real circuit
handshake (`UseCircuitCode` + `CompleteAgentMovement`) to root the agent, then
abandons it and asserts both halves: `is_root_agent()` is still true, and
`root_session_of` refuses it anyway.

**A task that owns its own sender can never hear the channel close.** The
teleport responder awaited only `events.recv()`, and it holds the `SharedSim`
that owns `events_tx`, so `Closed` was unreachable; it now selects the closed
and shutdown watches alongside. The same applies one level up: a held
`EventQueueGet` poll waits on the shutdown watch as well as its hold deadline
and returns its 502 re-poll answer immediately, an in-flight teleport stops
waiting out `TELEPORT_ARRIVAL_TIMEOUT`, and a connection task is shut down
gracefully with a one-second grace before it is dropped. Teardown is bounded
instead of "up to `eq_hold`".

**Nothing unbounded, nothing spinning.** Connections are capped (256) and must
send their request head within 15 s (hyper's `header_read_timeout`, which is
inert without a timer — the timer is why it never applied), and both the accept
loop and the UDP pump back off after a failure instead of retrying a broken
descriptor as fast as the runtime allows.

**A refused login costs a check.** `LoginServer::respond` grew a `rejection`
half — the same checks, in the same order, without the success facts — so the
endpoint runs the password, gate and MFA checks *before* it binds a socket,
consumes a session number and deep clones the scenario. One authority for the
ordering, and a test asserts the first accepted login is still session 1 after
three refusals.

The teleport-ordering duplication noted above is left alone: deduplicating it
would mean `sl-proto`'s tests depending on `sl-fake-grid`, which depends on
`sl-proto`.
